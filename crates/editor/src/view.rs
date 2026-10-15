//! The view of the editor camera as numbers: the pixel of a point of the room, the
//! ray of a pixel, and the item that a ray hits first (the pick).
//!
//! The pick is done on the CPU, against the meshes that the picture shows, with the
//! one rule of the camera rays of the renderer: a ray does not hit the back side of
//! the room box, so a click goes through the near walls as the view does. A mirror
//! and a glass ball are surfaces: a click selects them, not what shows in them.
//! Why not an id image of the trace pass: the trace pass has three shader forms and
//! a budget, and a click is one ray against some ten thousand triangles.

use bevy_ecs::world::World;
use glam::{Mat4, Vec3};
use maki_assets::{Assets, Mesh};
use maki_core::Transform;
use maki_render::ray_basis;
use maki_scene::{Camera, MeshInstance, RoomItem};

use crate::{Document, Item};

/// Points nearer to the camera than this are not drawn, in meters.
const NEAR: f32 = 0.05;
/// A light and a spawn point are picked as a sphere of at least this radius, in meters.
pub(crate) const MARK_RADIUS: f32 = 0.2;

/// A camera and the size of its picture.
pub struct View {
    pub camera: Camera,
    /// `[right, up, forward]`: see `maki_render::ray_basis`.
    basis: [Vec3; 3],
    /// Of the picture, in pixels.
    pub size: [f32; 2],
}

/// A half line: `origin + t * direction`, with a direction of length 1.
#[derive(Debug, Clone, Copy)]
pub struct Ray {
    pub origin: Vec3,
    pub direction: Vec3,
}

impl View {
    pub fn new(camera: Camera, size: [f32; 2]) -> Self {
        Self {
            camera,
            basis: ray_basis(&camera, size[0] / size[1]),
            size,
        }
    }

    /// The pixel that shows a point, from the top left corner of the picture. It
    /// can be out of the picture. `None`: the point is behind the camera.
    pub fn project(&self, point: Vec3) -> Option<[f32; 2]> {
        let [right, up, forward] = self.basis;
        let to_point = point - self.camera.position;
        let depth = to_point.dot(forward);
        if depth < NEAR {
            return None;
        }
        // The lengths of `right` and `up` hold the field of view.
        let x = to_point.dot(right) / right.length_squared() / depth;
        let y = to_point.dot(up) / up.length_squared() / depth;
        let [width, height] = self.size;
        Some([(x + 1.0) / 2.0 * width, (1.0 - y) / 2.0 * height])
    }

    /// The pixels of the part of a line that is in front of the camera.
    pub fn segment(&self, mut a: Vec3, mut b: Vec3) -> Option<[[f32; 2]; 2]> {
        let forward = self.basis[2];
        let depth = |point: Vec3| (point - self.camera.position).dot(forward);
        let (depth_a, depth_b) = (depth(a), depth(b));
        if depth_a < NEAR && depth_b < NEAR {
            return None;
        }
        // The end that is behind the camera moves to the near plane.
        let cut = a + (b - a) * ((NEAR - depth_a) / (depth_b - depth_a));
        if depth_a < NEAR {
            a = cut;
        } else if depth_b < NEAR {
            b = cut;
        }
        // A little in front of the near plane: rounding must not drop the cut point.
        let project = |point| self.project(point + forward * 1e-4);
        Some([project(a)?, project(b)?])
    }

    /// The ray of the camera through a pixel.
    pub fn ray(&self, pixel: [f32; 2]) -> Ray {
        let [right, up, forward] = self.basis;
        let x = pixel[0] / self.size[0] * 2.0 - 1.0;
        let y = 1.0 - pixel[1] / self.size[1] * 2.0;
        Ray {
            origin: self.camera.position,
            direction: (forward + right * x + up * y).normalize(),
        }
    }

    /// The length in the room, at the distance of `point`, that one pixel shows.
    pub fn meters_per_pixel(&self, point: Vec3) -> f32 {
        let depth = (point - self.camera.position).dot(self.basis[2]);
        2.0 * self.basis[1].length() * depth.max(NEAR) / self.size[1]
    }
}

impl Ray {
    pub fn at(&self, t: f32) -> Vec3 {
        self.origin + self.direction * t
    }

    /// The distance along the ray to a triangle (Moeller and Trumbore). `cull`: the
    /// ray does not hit the back side (the corners go clockwise for the ray).
    fn triangle(&self, [a, b, c]: [Vec3; 3], cull: bool) -> Option<f32> {
        let (ab, ac) = (b - a, c - a);
        let p = self.direction.cross(ac);
        let det = ab.dot(p);
        if det.abs() < 1e-9 || (cull && det < 0.0) {
            return None;
        }
        let from_a = self.origin - a;
        let u = from_a.dot(p) / det;
        let q = from_a.cross(ab);
        let v = self.direction.dot(q) / det;
        let t = ac.dot(q) / det;
        (u >= 0.0 && v >= 0.0 && u + v <= 1.0 && t > 0.0).then_some(t)
    }

    /// The distance along the ray to the first triangle of a mesh at a matrix.
    fn mesh(&self, mesh: &Mesh, matrix: Mat4) -> Option<f32> {
        // The ray in the space of the mesh. The direction keeps its length there,
        // so a distance along it is a distance in the room.
        let inverse = matrix.inverse();
        let local = Ray {
            origin: inverse.transform_point3(self.origin),
            direction: inverse.transform_vector3(self.direction),
        };
        let positions = mesh.positions();
        let triangles = mesh.indices().as_chunks::<3>().0.iter();
        let corners = |triangle: &[u32; 3]| triangle.map(|corner| positions[corner as usize]);
        triangles
            .filter_map(|triangle| local.triangle(corners(triangle), mesh.open_back()))
            .min_by(f32::total_cmp)
    }

    /// True if the ray goes through a box between two corners.
    fn in_box(&self, low: Vec3, high: Vec3) -> bool {
        // The parts of the ray between the two planes of each axis must overlap.
        let (to_low, to_high) = (
            (low - self.origin) / self.direction,
            (high - self.origin) / self.direction,
        );
        let enter = to_low.min(to_high).max_element();
        let leave = to_low.max(to_high).min_element();
        enter <= leave && leave >= 0.0
    }

    /// The distance along the ray to the near side of a sphere.
    pub fn sphere(&self, center: Vec3, radius: f32) -> Option<f32> {
        let to_center = center - self.origin;
        let along = to_center.dot(self.direction);
        let beside = to_center.length_squared() - along * along;
        let half = radius * radius - beside;
        (half >= 0.0 && along - half.sqrt() > 0.0).then(|| along - half.sqrt())
    }

    /// Where the ray meets the plane of a height (Y), and the distance to there.
    pub fn height(&self, y: f32) -> Option<(Vec3, f32)> {
        let t = (y - self.origin.y) / self.direction.y;
        (t.is_finite() && t > 0.0).then(|| (self.at(t), t))
    }

    /// The place on a line where the line is nearest to the ray: the distance from
    /// `point` along `axis` (length 1). 0.0 for a line along the ray.
    pub fn along(&self, point: Vec3, axis: Vec3) -> f32 {
        let to_point = point - self.origin;
        let cos = axis.dot(self.direction);
        let rest = 1.0 - cos * cos;
        if rest < 1e-6 {
            return 0.0;
        }
        (cos * self.direction.dot(to_point) - axis.dot(to_point)) / rest
    }
}

/// The mesh instances of the room: the matrix, the mesh, and the item of each.
/// `None` for the room box.
fn instances(world: &mut World) -> Vec<(Mat4, MeshInstance, Option<RoomItem>)> {
    let mut query = world.query::<(&Transform, &MeshInstance, Option<&RoomItem>)>();
    let instance = |(transform, mesh, item): (&Transform, &MeshInstance, Option<&RoomItem>)| {
        (transform.matrix(), *mesh, item.copied())
    };
    query.iter(world).map(instance).collect()
}

/// The item that a ray hits first. `None`: a wall, the floor, or nothing.
pub fn pick(world: &mut World, document: &Document, ray: &Ray) -> Option<Item> {
    let mut nearest: (f32, Option<Item>) = (f32::MAX, None);
    let mut hit = |t: f32, item: Option<Item>| {
        if t < nearest.0 {
            nearest = (t, item);
        }
    };

    let instances = instances(world);
    let meshes = world.resource::<Assets<Mesh>>();
    for (matrix, instance, item) in instances {
        let Some(mesh) = meshes.get(instance.0) else {
            continue;
        };
        let item = item.map(|item| Item::Object(item.0));
        if let Some(t) = ray.mesh(mesh, matrix) {
            hit(t, item);
        }
    }

    // Things with no mesh, or with a very small one, have a pick shape.
    for item in (0..document.room.objects.len()).map(Item::Object) {
        let Some(light) = document.light(&item) else {
            continue;
        };
        let radius = light.radius.max(MARK_RADIUS);
        if let Some(t) = ray.sphere(light.position.into(), radius) {
            hit(t, Some(item));
        }
    }
    let objects = document.room.objects.iter().enumerate();
    let spawns = objects.filter(|(_, object)| object.is_spawn());
    let spawns = spawns.map(|(index, object)| (Item::Object(index), object.pose.position));
    for (item, position) in spawns {
        let center = Vec3::from(position) + Vec3::Y * MARK_RADIUS;
        if let Some(t) = ray.sphere(center, MARK_RADIUS) {
            hit(t, Some(item));
        }
    }
    for (index, event) in document.events.iter().enumerate() {
        // A little above the floor: the rectangle is in front of the floor.
        let on_floor = ray.height(0.02);
        let inside = |(at, _): &(Vec3, f32)| {
            event
                .area
                .is_some_and(|area| area.distance(at.x, at.z) == 0.0)
        };
        if let Some((_, t)) = on_floor.filter(inside) {
            hit(t, Some(Item::Event(index)));
        }
    }
    nearest.1
}

/// The mirrors of the room, to test if one of them hides a point: the pixels of a
/// mirror have the distance of what the mirror shows (`maki_render::Distances`).
#[derive(Default)]
pub struct Mirrors(Vec<Mirror>);

/// A mirror mesh in the space of the room: the box around it, and its triangles.
struct Mirror {
    low: Vec3,
    high: Vec3,
    triangles: Vec<[Vec3; 3]>,
}

/// A mirror mesh with more triangles is not tested.
// ponytail: each triangle of a mirror in the way of a ray is tested. A tree over the
// triangles if a room has a mirror with many of them.
const MIRROR_TRIANGLES: usize = 4096;

impl Mirrors {
    pub fn of(world: &mut World) -> Self {
        let instances = instances(world);
        let meshes = world.resource::<Assets<Mesh>>();
        let mirrors = instances.into_iter().filter_map(|(matrix, instance, _)| {
            let mesh = meshes.get(instance.0)?;
            let (positions, indices) = (mesh.positions(), mesh.indices());
            let is_mirror = mesh.material() == maki_assets::Material::Mirror;
            if !is_mirror || indices.len() / 3 > MIRROR_TRIANGLES {
                return None;
            }
            let corner = |index: &u32| matrix.transform_point3(positions[*index as usize]);
            let triangles: Vec<[Vec3; 3]> = indices
                .as_chunks::<3>()
                .0
                .iter()
                .map(|triangle| triangle.each_ref().map(corner))
                .collect();
            let corners = || triangles.iter().flatten().copied();
            Some(Mirror {
                low: corners().fold(Vec3::MAX, Vec3::min),
                high: corners().fold(Vec3::MIN, Vec3::max),
                triangles,
            })
        });
        Self(mirrors.collect())
    }

    /// The distance along a ray to the first mirror.
    pub fn hit(&self, ray: &Ray) -> Option<f32> {
        let in_way = self
            .0
            .iter()
            .filter(|mirror| ray.in_box(mirror.low, mirror.high));
        let hits = in_way.flat_map(|mirror| &mirror.triangles);
        hits.filter_map(|triangle| ray.triangle(*triangle, false))
            .min_by(f32::total_cmp)
    }
}

/// The boxes around the meshes of an item: for each, the matrix of the mesh and the
/// two corners of the box in the space of the mesh.
pub fn mesh_boxes(world: &mut World, item: RoomItem) -> Vec<(Mat4, Vec3, Vec3)> {
    let instances = instances(world);
    let meshes = world.resource::<Assets<Mesh>>();
    let of_item = instances
        .into_iter()
        .filter(|instance| instance.2 == Some(item));
    let boxed = of_item.filter_map(|(matrix, instance, _)| {
        let positions = meshes.get(instance.0)?.positions();
        let low = positions.iter().fold(Vec3::MAX, |low, at| low.min(*at));
        let high = positions.iter().fold(Vec3::MIN, |high, at| high.max(*at));
        Some((matrix, low, high))
    });
    boxed.collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> View {
        let camera = Camera {
            position: Vec3::new(0.0, 9.0, 7.0),
            target: Vec3::new(0.0, 0.5, 0.0),
            fov_y_degrees: 45.0,
        };
        View::new(camera, [1280.0, 720.0])
    }

    #[test]
    fn a_point_and_the_ray_of_its_pixel_agree() {
        let view = view();
        // The target is in the center of the picture.
        let center = view.project(view.camera.target).unwrap();
        assert!((center[0] - 640.0).abs() < 1e-3 && (center[1] - 360.0).abs() < 1e-3);
        // Also in a picture of another size and shape: the editor after a resize.
        let tall = View::new(view.camera, [700.0, 1000.0]);
        let center = tall.project(tall.camera.target).unwrap();
        assert!((center[0] - 350.0).abs() < 1e-3 && (center[1] - 500.0).abs() < 1e-3);
        for view in [&view, &tall] {
            for point in [Vec3::new(2.0, 0.0, -1.5), Vec3::new(-3.0, 1.0, 2.0)] {
                let pixel = view.project(point).unwrap();
                let ray = view.ray(pixel);
                let to_point = point - ray.origin;
                assert!(ray.direction.cross(to_point).length() < 1e-3, "{point}");
            }
        }
        // +X of the room is at the right, and up is up.
        assert!(view.project(Vec3::X).unwrap()[0] > 640.0);
        assert!(view.project(Vec3::Y * 2.0).unwrap()[1] < 360.0);
        assert_eq!(view.project(Vec3::new(0.0, 9.0, 8.0)), None);
        // A line from in front of the camera to behind it is cut, not dropped.
        assert!(view.segment(Vec3::ZERO, Vec3::new(0.0, 9.0, 9.0)).is_some());
    }

    #[test]
    fn a_ray_hits_the_front_of_a_triangle_a_sphere_and_the_floor() {
        let down = Ray {
            origin: Vec3::new(0.2, 5.0, 0.2),
            direction: Vec3::NEG_Y,
        };
        // Counter-clockwise from above: the front is up.
        let front = [Vec3::ZERO, Vec3::Z, Vec3::X];
        let back = [Vec3::ZERO, Vec3::X, Vec3::Z];
        assert_eq!(down.triangle(front, true), Some(5.0));
        assert_eq!(down.triangle(back, true), None);
        assert_eq!(down.triangle(back, false), Some(5.0));
        assert_eq!(down.sphere(Vec3::new(0.2, 1.0, 0.2), 0.5), Some(3.5));
        assert_eq!(down.sphere(Vec3::new(2.0, 1.0, 0.2), 0.5), None);
        assert_eq!(down.height(1.0).map(|hit| hit.1), Some(4.0));
        // The X axis passes 0.2 m beside the ray, at x = 0.2.
        assert!((down.along(Vec3::ZERO, Vec3::X) - 0.2).abs() < 1e-5);
        // A box under the ray, and one beside it.
        assert!(down.in_box(Vec3::new(0.0, 1.0, 0.0), Vec3::new(1.0, 2.0, 1.0)));
        assert!(!down.in_box(Vec3::new(1.0, 1.0, 0.0), Vec3::new(2.0, 2.0, 1.0)));
        assert!(!down.in_box(Vec3::new(0.0, 6.0, 0.0), Vec3::new(1.0, 7.0, 1.0)));
    }
}
