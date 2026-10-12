//! The mesh asset and the generators of simple meshes.

use std::f32::consts::{PI, TAU};

use anyhow::{Result, ensure};
use glam::Vec3;
use serde::{Deserialize, Serialize};

/// How a surface sends light back.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Material {
    /// Sends light back equally in all directions, with the base color (paper, plaster).
    Diffuse,
    /// A perfect mirror. The base color tints the reflection.
    Mirror,
    /// Clear glass: a part of the light is reflected, the rest goes through and bends.
    /// The base color tints the light that goes through.
    Glass {
        /// Index of refraction: how much the light bends. 1.5 for window glass, 1.33 for water.
        ior: f32,
    },
    /// A rough mirror (brushed steel, worn brass): the reflection is soft, more so
    /// with more roughness. The base color tints it. No diffuse light.
    Metal {
        /// 0.05 (almost a mirror) to 1.0 (no reflection that shows a shape).
        roughness: f32,
    },
    /// A diffuse surface with the base color under a clear, rough coat (plastic,
    /// paint, wet stone): the coat reflects some percent of the light, more at a flat
    /// angle, with no tint.
    Plastic {
        /// As for `Metal`.
        roughness: f32,
    },
}

impl Material {
    /// The lowest roughness that the renderer uses. Below it, the bright spot of a
    /// small lamp is smaller than a pixel: use `Mirror`.
    pub const MIN_ROUGHNESS: f32 = 0.05;
}

/// A wall of a room, named by the side of the room that it is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wall {
    NegX,
    PosX,
    NegZ,
    PosZ,
}

impl Wall {
    /// The line of the wall on the floor of a room of `width` x `depth` meters with
    /// its center at the origin: `(start, along, length, inward)`. The wall goes from
    /// `start` for `length` meters in the direction `along` (+Z or +X). `inward` points
    /// into the room.
    pub fn frame(self, width: f32, depth: f32) -> (Vec3, Vec3, f32, Vec3) {
        let (x, z) = (width / 2.0, depth / 2.0);
        match self {
            Wall::NegX => (Vec3::new(-x, 0.0, -z), Vec3::Z, depth, Vec3::X),
            Wall::PosX => (Vec3::new(x, 0.0, -z), Vec3::Z, depth, Vec3::NEG_X),
            Wall::NegZ => (Vec3::new(-x, 0.0, -z), Vec3::X, width, Vec3::Z),
            Wall::PosZ => (Vec3::new(-x, 0.0, z), Vec3::X, width, Vec3::NEG_Z),
        }
    }
}

/// An opening in a wall of a room, with a short passage behind it that ends at a wall.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Doorway {
    pub wall: Wall,
    /// Where the center of the opening is along the wall: its X coordinate for a Z
    /// wall, its Z coordinate for an X wall.
    pub offset: f32,
    pub width: f32,
    pub height: f32,
    /// Length of the passage behind the wall, in meters.
    pub depth: f32,
}

impl Doorway {
    /// The opening along a wall of `length` meters, in meters from the start of the
    /// wall: `(from, to)`.
    pub fn span(&self, length: f32) -> (f32, f32) {
        let center = length / 2.0 + self.offset;
        (center - self.width / 2.0, center + self.width / 2.0)
    }
}

/// A triangle mesh with one color and one material. Coordinates are right-handed with +Y up.
/// The front side of a triangle is the side where its corners go counter-clockwise.
///
/// [`Mesh::new`] checks the data, so users (the GPU upload, later the shaders) can
/// trust each mesh: every index is in range and no value is NaN.
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    name: String,
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    indices: Vec<u32>,
    base_color: [f32; 4],
    material: Material,
    emissive: [f32; 3],
}

impl Mesh {
    /// `normals: None` computes smooth normals from the triangles.
    /// `base_color` is linear RGBA, as the base color factor of a glTF material.
    /// The material is [`Material::Diffuse`]; see [`Mesh::with_material`].
    pub fn new(
        name: impl Into<String>,
        positions: Vec<Vec3>,
        normals: Option<Vec<Vec3>>,
        indices: Vec<u32>,
        base_color: [f32; 4],
    ) -> Result<Self> {
        let name = name.into();
        ensure!(!positions.is_empty(), "mesh `{name}` has no vertex");
        ensure!(
            u32::try_from(positions.len()).is_ok(),
            "mesh `{name}` has more vertices than a u32 index can address"
        );
        ensure!(
            !indices.is_empty() && indices.len().is_multiple_of(3),
            "mesh `{name}` has {} indices: not a list of triangles",
            indices.len()
        );
        ensure!(
            indices
                .iter()
                .all(|index| (*index as usize) < positions.len()),
            "mesh `{name}` has an index past its {} vertices",
            positions.len()
        );
        let normals = normals.unwrap_or_else(|| smooth_normals(&positions, &indices));
        ensure!(
            normals.len() == positions.len(),
            "mesh `{name}` has {} positions but {} normals",
            positions.len(),
            normals.len()
        );
        ensure!(
            positions.iter().chain(&normals).all(|v| v.is_finite()),
            "mesh `{name}` has a position or normal that is not a finite number"
        );
        Ok(Self {
            name,
            positions,
            normals,
            indices,
            base_color,
            material: Material::Diffuse,
            emissive: [0.0; 3],
        })
    }

    /// The same mesh with another material.
    pub fn with_material(mut self, material: Material) -> Self {
        self.material = material;
        self
    }

    /// The same mesh with a light of its own (linear RGB, as radiance): it shows with
    /// this color, and bounce rays that find it bring its light to the room.
    pub fn with_emissive(mut self, emissive: [f32; 3]) -> Self {
        self.emissive = emissive;
        self
    }

    /// The same mesh with another base color (linear RGBA).
    pub fn with_base_color(mut self, base_color: [f32; 4]) -> Self {
        self.base_color = base_color;
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn positions(&self) -> &[Vec3] {
        &self.positions
    }

    /// One unit normal for each position.
    pub fn normals(&self) -> &[Vec3] {
        &self.normals
    }

    /// Three indices for each triangle.
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    pub fn base_color(&self) -> [f32; 4] {
        self.base_color
    }

    pub fn material(&self) -> Material {
        self.material
    }

    /// The light that the surface sends out by itself. Zero for most meshes.
    pub fn emissive(&self) -> [f32; 3] {
        self.emissive
    }

    /// An open room: a floor and four walls, no ceiling. The normals point into the room.
    /// The floor is at y = 0 with its center at the origin. `width` is along X, `depth`
    /// along Z, `height` along Y.
    pub fn room(width: f32, depth: f32, height: f32) -> Self {
        Self::room_with_doorways(width, depth, height, &[])
    }

    /// [`Mesh::room`] with openings in its walls. Behind each opening is a passage: a
    /// floor, two sides, a top, and a wall at its end. A doorway that does not fit in
    /// its wall, or that overlaps one before it, is left out with a warning.
    pub fn room_with_doorways(width: f32, depth: f32, height: f32, doorways: &[Doorway]) -> Self {
        let corner = Vec3::new(-width / 2.0, 0.0, -depth / 2.0);
        let mut quads = Quads::default();
        // The normal of a quad is `u x v`.
        quads.add(corner, Vec3::Z * depth, Vec3::X * width); // floor, faces +Y
        let up = Vec3::Y;
        for wall in [Wall::NegX, Wall::PosX, Wall::NegZ, Wall::PosZ] {
            let (start, along, length, inward) = wall.frame(width, depth);
            let mut doors: Vec<_> = doorways.iter().filter(|door| door.wall == wall).collect();
            doors.sort_by(|a, b| a.offset.total_cmp(&b.offset));
            // The wall is built up to this distance from its start.
            let mut built = 0.0;
            for door in doors {
                let (from, to) = door.span(length);
                // `!(..)` is true also for NaN.
                if !(from >= built && to <= length && door.width > 0.0 && door.depth > 0.0)
                    || !(door.height > 0.0 && door.height <= height)
                {
                    tracing::warn!(?door, "the doorway does not fit in its wall: left out");
                    continue;
                }
                if from > built {
                    let piece = along * (from - built);
                    quads.add_facing(start + along * built, piece, up * height, inward);
                }
                let (side, top) = (up * door.height, along * door.width);
                let opening = start + along * from;
                if door.height < height {
                    quads.add_facing(opening + side, top, up * (height - door.height), inward);
                }
                // The passage goes out of the room.
                let out = -inward * door.depth;
                quads.add_facing(opening, top, out, up);
                quads.add_facing(opening, out, side, along);
                quads.add_facing(opening + top, out, side, -along);
                quads.add_facing(opening + out, top, side, inward);
                quads.add_facing(opening + side, top, out, -up);
                built = to;
            }
            if length > built {
                let piece = along * (length - built);
                quads.add_facing(start + along * built, piece, up * height, inward);
            }
        }
        quads.build("room", [0.8, 0.8, 0.8, 1.0])
    }

    /// A cube with its center at the origin. The normals point out.
    pub fn cube(size: f32) -> Self {
        Self::cuboid(Vec3::splat(-size / 2.0), Vec3::splat(size / 2.0))
    }

    /// A box with sides along the axes, from the corner `min` to the corner `max`.
    /// The normals point out. The origin of the mesh can be outside of the box: a door
    /// is a box with the origin on its hinge.
    pub fn cuboid(min: Vec3, max: Vec3) -> Self {
        let size = max - min;
        let mut quads = Quads::default();
        // `a = b x c` for each of the three rotations of the axes.
        for (a, b, c) in [
            (Vec3::X, Vec3::Y, Vec3::Z),
            (Vec3::Y, Vec3::Z, Vec3::X),
            (Vec3::Z, Vec3::X, Vec3::Y),
        ] {
            let (a, b, c) = (a * size, b * size, c * size);
            quads.add(min + a, b, c);
            quads.add(min, c, b);
        }
        quads.build("cube", [0.8, 0.3, 0.2, 1.0])
    }

    /// A capsule that stands on the origin: a cylinder along Y with a half sphere at
    /// each end. `height` is the full height, not less than two times the radius.
    /// `segments` is the number of quads around the Y axis (3 or more), `rings` the
    /// number in each half sphere (1 or more). Smaller values are raised to the minimum.
    pub fn capsule(radius: f32, height: f32, segments: u32, rings: u32) -> Self {
        let (segments, rings) = (segments.max(3), rings.max(1));
        // The centers of the two half spheres are this far from the middle.
        let half = (height / 2.0 - radius).max(0.0);
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        let mut indices = Vec::new();
        // The rows of vertices from the top: the upper half sphere, then the lower one.
        // The band between the two is the cylinder.
        let rows = 2 * (rings + 1);
        for row in 0..rows {
            let (ring, center) = if row <= rings {
                (row, half)
            } else {
                (row - 1, -half)
            };
            let (sin_polar, cos_polar) = (PI * ring as f32 / (2 * rings) as f32).sin_cos();
            for segment in 0..=segments {
                let (sin_around, cos_around) = (TAU * segment as f32 / segments as f32).sin_cos();
                let normal = Vec3::new(sin_polar * cos_around, cos_polar, sin_polar * sin_around);
                positions.push(normal * radius + Vec3::Y * (center + half + radius));
                normals.push(normal);
            }
        }
        for row in 0..rows - 1 {
            // A capsule with no cylinder is a sphere: the band has no area.
            if row == rings && half == 0.0 {
                continue;
            }
            for segment in 0..segments {
                let a = row * (segments + 1) + segment;
                let b = a + segments + 1;
                let (c, d) = (b + 1, a + 1);
                // As for the sphere: one triangle of a quad at a pole has no area.
                if row != rows - 2 {
                    indices.extend([a, c, b]);
                }
                if row != 0 {
                    indices.extend([a, d, c]);
                }
            }
        }
        Self::new(
            "capsule",
            positions,
            Some(normals),
            indices,
            [0.75, 0.7, 0.6, 1.0],
        )
        .expect("the capsule generator makes a valid mesh")
    }

    /// A sphere with its center at the origin. The normals point out.
    /// `segments` is the number of quads around the Y axis (3 or more), `rings` the
    /// number from pole to pole (2 or more). Smaller values are raised to the minimum.
    pub fn sphere(radius: f32, segments: u32, rings: u32) -> Self {
        let (segments, rings) = (segments.max(3), rings.max(2));
        let mut normals = Vec::new();
        let mut indices = Vec::new();
        // The last vertex of each ring is at the position of the first one. The
        // duplicate keeps the grid regular (and gives texture coordinates a seam later).
        for ring in 0..=rings {
            let (sin_polar, cos_polar) = (PI * ring as f32 / rings as f32).sin_cos();
            for segment in 0..=segments {
                let (sin_around, cos_around) = (TAU * segment as f32 / segments as f32).sin_cos();
                normals.push(Vec3::new(
                    sin_polar * cos_around,
                    cos_polar,
                    sin_polar * sin_around,
                ));
            }
        }
        for ring in 0..rings {
            for segment in 0..segments {
                let a = ring * (segments + 1) + segment;
                let b = a + segments + 1;
                let (c, d) = (b + 1, a + 1);
                // At a pole, two corners of the quad are the same point: one of the
                // two triangles has no area and is left out.
                if ring != rings - 1 {
                    indices.extend([a, c, b]);
                }
                if ring != 0 {
                    indices.extend([a, d, c]);
                }
            }
        }
        let positions = normals.iter().map(|normal| *normal * radius).collect();
        let base_color = [0.2, 0.4, 0.8, 1.0];
        Self::new("sphere", positions, Some(normals), indices, base_color)
            .expect("the sphere generator makes a valid mesh")
    }
}

/// Builds a mesh of flat quads. Each quad has its own four vertices, so edges are sharp.
#[derive(Default)]
struct Quads {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    indices: Vec<u32>,
}

impl Quads {
    /// A quad with one corner at `origin` and the sides `u` and `v`. Its front side
    /// and its normal face the direction `u x v`.
    fn add(&mut self, origin: Vec3, u: Vec3, v: Vec3) {
        let first = self.positions.len() as u32;
        self.positions
            .extend([origin, origin + u, origin + u + v, origin + v]);
        self.normals.extend([u.cross(v).normalize(); 4]);
        self.indices
            .extend([0, 1, 2, 0, 2, 3].map(|corner| first + corner));
    }

    /// A quad as `add` makes it, with its front side toward `facing`.
    fn add_facing(&mut self, origin: Vec3, u: Vec3, v: Vec3, facing: Vec3) {
        if u.cross(v).dot(facing) >= 0.0 {
            self.add(origin, u, v);
        } else {
            self.add(origin, v, u);
        }
    }

    fn build(self, name: &str, base_color: [f32; 4]) -> Mesh {
        let Self {
            positions,
            normals,
            indices,
        } = self;
        Mesh::new(name, positions, Some(normals), indices, base_color)
            .expect("the quad generator makes a valid mesh")
    }
}

/// A normal for each vertex: the mean of the normals of its triangles, weighted by
/// triangle area. All indices must be in range.
fn smooth_normals(positions: &[Vec3], indices: &[u32]) -> Vec<Vec3> {
    let mut normals = vec![Vec3::ZERO; positions.len()];
    for triangle in indices.as_chunks::<3>().0 {
        let [a, b, c] = triangle.map(|index| index as usize);
        // The length of the cross product is two times the triangle area.
        let weighted = (positions[b] - positions[a]).cross(positions[c] - positions[a]);
        for index in [a, b, c] {
            normals[index] += weighted;
        }
    }
    // A vertex with no triangle of nonzero area gets an arbitrary unit normal.
    normals
        .iter()
        .map(|normal| normal.try_normalize().unwrap_or(Vec3::Y))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each triangle has area, and its front side agrees with its three vertex normals.
    fn assert_winding_agrees_with_normals(mesh: &Mesh) {
        for triangle in mesh.indices().as_chunks::<3>().0 {
            let [a, b, c] = triangle.map(|index| index as usize);
            let p = mesh.positions();
            let face = (p[b] - p[a]).cross(p[c] - p[a]);
            assert!(face.length() > 1e-6, "{}: empty triangle", mesh.name());
            for index in [a, b, c] {
                let normal = mesh.normals()[index];
                assert!((normal.length() - 1.0).abs() < 1e-5);
                assert!(face.dot(normal) > 0.0, "{}: wrong winding", mesh.name());
            }
        }
    }

    #[test]
    fn room_is_open_and_faces_inward() {
        let mesh = Mesh::room(8.0, 6.0, 3.0);
        assert_eq!((mesh.positions().len(), mesh.indices().len()), (20, 30));
        assert_winding_agrees_with_normals(&mesh);
        let center = Vec3::new(0.0, 1.5, 0.0);
        for (position, normal) in mesh.positions().iter().zip(mesh.normals()) {
            assert!((center - *position).dot(*normal) > 0.0);
            // No ceiling: no surface faces down.
            assert!(normal.y >= 0.0);
            assert!(position.x.abs() <= 4.0 && position.z.abs() <= 3.0);
            assert!((0.0..=3.0).contains(&position.y));
        }
    }

    #[test]
    fn a_doorway_opens_the_wall_and_adds_a_passage() {
        let door = Doorway {
            wall: Wall::NegZ,
            offset: 3.0,
            width: 1.0,
            height: 2.0,
            depth: 0.5,
        };
        let mesh = Mesh::room_with_doorways(8.0, 6.0, 3.0, &[door]);
        // The plain room, with 3 quads in place of one wall, and the 5 quads of the passage.
        assert_eq!(mesh.positions().len(), 20 + 4 * 7);
        assert_winding_agrees_with_normals(&mesh);
        // No wall surface in the opening.
        let in_opening = |p: &Vec3| p.z == -3.0 && p.x > 2.5 && p.x < 3.5 && p.y < 2.0;
        assert!(!mesh.positions().iter().any(in_opening));
        // The passage ends half a meter behind the wall.
        let back = mesh.positions().iter().map(|p| p.z).fold(0.0, f32::min);
        assert_eq!(back, -3.5);

        // A doorway that is wider than its wall is left out.
        let wide = Doorway { width: 9.0, ..door };
        assert_eq!(
            Mesh::room_with_doorways(8.0, 6.0, 3.0, &[wide]),
            Mesh::room(8.0, 6.0, 3.0)
        );
    }

    #[test]
    fn a_capsule_stands_on_the_origin() {
        let capsule = Mesh::capsule(0.3, 1.7, 8, 2);
        assert_winding_agrees_with_normals(&capsule);
        let (low, high) = capsule
            .positions()
            .iter()
            .fold((f32::MAX, f32::MIN), |(low, high), p| {
                (low.min(p.y), high.max(p.y))
            });
        assert!(low.abs() < 1e-6 && (high - 1.7).abs() < 1e-6);
        assert!(
            capsule
                .positions()
                .iter()
                .all(|p| p.with_y(0.0).length() < 0.3 + 1e-6)
        );
        // No cylinder: a sphere.
        assert_winding_agrees_with_normals(&Mesh::capsule(0.5, 1.0, 8, 2));
    }

    #[test]
    fn a_cuboid_goes_from_corner_to_corner() {
        let door = Mesh::cuboid(Vec3::new(0.0, 0.0, -0.03), Vec3::new(1.0, 2.0, 0.03));
        assert_winding_agrees_with_normals(&door);
        let fold =
            |f: fn(Vec3, Vec3) -> Vec3, start| door.positions().iter().copied().fold(start, f);
        assert_eq!(fold(Vec3::min, Vec3::MAX), Vec3::new(0.0, 0.0, -0.03));
        assert_eq!(fold(Vec3::max, Vec3::MIN), Vec3::new(1.0, 2.0, 0.03));
    }

    #[test]
    fn cube_and_sphere_face_outward() {
        let cube = Mesh::cube(2.0);
        assert_eq!((cube.positions().len(), cube.indices().len()), (24, 36));
        let sphere = Mesh::sphere(0.5, 8, 4);
        // 5 rings of 9 vertices. 8 quads in each of 4 bands, less one triangle at each pole.
        assert_eq!(
            (sphere.positions().len(), sphere.indices().len()),
            (45, 144)
        );
        for mesh in [&cube, &sphere] {
            assert_winding_agrees_with_normals(mesh);
            for (position, normal) in mesh.positions().iter().zip(mesh.normals()) {
                assert!(position.dot(*normal) > 0.0);
            }
        }
        assert!(cube.positions().iter().all(|p| p.abs() == Vec3::ONE));
        assert!(
            sphere
                .positions()
                .iter()
                .all(|p| (p.length() - 0.5).abs() < 1e-6)
        );
    }

    #[test]
    fn missing_normals_are_generated() {
        let cube = Mesh::cube(1.0);
        let mesh = Mesh::new(
            "no normals",
            cube.positions().to_vec(),
            None,
            cube.indices().to_vec(),
            [1.0; 4],
        )
        .unwrap();
        // The cube has four vertices for each side, so smooth normals are the side normals.
        for (generated, expected) in mesh.normals().iter().zip(cube.normals()) {
            assert!(generated.distance(*expected) < 1e-6);
        }
    }

    #[test]
    fn bad_mesh_data_is_rejected() {
        let triangle = [Vec3::ZERO, Vec3::X, Vec3::Y];
        let new = |positions: &[Vec3], normals: Option<Vec<Vec3>>, indices: &[u32]| {
            Mesh::new(
                "bad",
                positions.to_vec(),
                normals,
                indices.to_vec(),
                [1.0; 4],
            )
        };
        assert!(new(&triangle, None, &[0, 1, 2]).is_ok());
        assert!(new(&[], None, &[]).is_err());
        assert!(new(&triangle, None, &[0, 1]).is_err());
        assert!(new(&triangle, None, &[0, 1, 3]).is_err());
        assert!(new(&triangle, Some(vec![Vec3::Z]), &[0, 1, 2]).is_err());
        assert!(new(&[Vec3::NAN, Vec3::X, Vec3::Y], None, &[0, 1, 2]).is_err());
    }
}
