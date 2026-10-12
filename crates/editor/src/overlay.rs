//! The marks of the editor on the picture: the grid of the floor, the things that
//! have no mesh (spawn points, event areas, the swing of a door, the second pose of a
//! mover), the selection, and the gizmo.
//!
//! All are lines and text of the egui painter, from points of the room that `View`
//! turns into pixels. Not a line pass of the frame graph: the marks then are drawn
//! by the UI pass, after the tone curve, with no new GPU state. They are not in the
//! path tracer, the denoiser, or the anti-aliasing.
//!
//! What is in front of a mark hides it ([`MarkDepth`]): the grid does not show on a
//! prop. The lines are cut on the CPU, with the distance of each pixel that the trace
//! pass has. The selection, the item under the pointer, and the gizmo are not cut:
//! they show through an object, which they must do.

use std::time::Instant;

use bevy_ecs::resource::Resource;
use bevy_ecs::world::World;
use glam::{Mat4, Vec3};
use maki_render::{Accumulation, Distances, FrameGraph, RenderGpu, RenderWorld, read_distances};
use maki_scene::RoomItem;
use maki_ui::egui::{self, Color32, Stroke, Ui};

use crate::gizmo::{Handle, SQUARE, Tool};
use crate::view::{MARK_RADIUS, Mirrors, View, mesh_boxes};
use crate::{Editor, Item};

/// What hides a point of the room from the camera of the editor, as of the last
/// frame that changed the picture.
#[derive(Resource, Default)]
pub struct MarkDepth {
    distances: Option<Distances>,
    mirrors: Mirrors,
    /// `Accumulation::samples` at the last look.
    samples: u32,
}

/// A point is hidden if it is farther away than the surface of its pixel by more
/// than `SLACK` meters and `SLACK_PART` of its distance. A mark on the floor is at
/// the distance of the floor; the image has 16-bit floats (0.1 % of a distance), and
/// one pixel of a floor that the camera sees at a flat angle has a range of distances.
const SLACK: f32 = 0.02;
const SLACK_PART: f32 = 0.005;
/// A line is tested at steps of this many pixels.
const CUT_STEP: f32 = 3.0;

impl MarkDepth {
    /// True if a surface of the picture is in front of the point.
    pub fn hides(&self, view: &View, point: Vec3) -> bool {
        let Some(distances) = &self.distances else {
            return false;
        };
        let Some([x, y]) = view.project(point) else {
            return true;
        };
        let distance = point.distance(view.camera.position);
        let limit = distance * (1.0 - SLACK_PART) - SLACK;
        let surface = distances.at(x / view.size[0], y / view.size[1]);
        if surface > 0.0 && surface < limit {
            return true;
        }
        // The pixel of a mirror has the distance of what the mirror shows.
        let ray = view.ray([x, y]);
        self.mirrors.hit(&ray).is_some_and(|mirror| mirror < limit)
    }

    /// The parts of a line of the room that no surface hides.
    fn parts(&self, view: &View, a: Vec3, b: Vec3) -> Vec<[Vec3; 2]> {
        let Some([from, to]) = view.segment(a, b) else {
            return Vec::new();
        };
        let pixels = glam::Vec2::from(from).distance(to.into());
        let steps = (pixels / CUT_STEP).ceil().clamp(1.0, 1024.0) as usize;
        let point = |step: usize| a.lerp(b, step as f32 / steps as f32);
        let mut parts = Vec::new();
        // The first point of the part that is open.
        let mut start = None;
        for step in 0..=steps {
            let hidden = self.hides(view, point(step));
            match (hidden, start) {
                (false, None) => start = Some(step),
                (true, Some(first)) => {
                    parts.push([point(first), point(step - 1)]);
                    start = None;
                }
                _ => {}
            }
        }
        if let Some(first) = start {
            parts.push([point(first), b]);
        }
        parts
    }
}

/// Runs in `Update`, between two frames: reads the distances of the picture after
/// the picture changed. While the camera moves, that is each frame, and the cut of
/// the marks is one frame late.
pub(crate) fn read_depth(world: &mut World) {
    if world.resource::<Editor>().playing() || !world.contains_resource::<RenderGpu>() {
        return;
    }
    let accumulation = world.resource::<RenderWorld>().0.resource::<Accumulation>();
    let (samples, sampling) = (accumulation.samples(), accumulation.sampling());
    let before = std::mem::replace(&mut world.resource_mut::<MarkDepth>().samples, samples);
    // The count of the samples started again: the picture changed. The same count
    // as before while new samples come: it starts again in each frame.
    let changed = samples < before || (samples == before && sampling);
    if !changed && world.resource::<MarkDepth>().distances.is_some() {
        return;
    }
    let start = Instant::now();
    let mirrors = Mirrors::of(world);
    let distances = world.resource_scope(|world, mut gpu: bevy_ecs::world::Mut<RenderGpu>| {
        let graph = world.get_resource::<FrameGraph>()?;
        read_distances(graph, &mut gpu)
    });
    tracing::trace!(time = ?start.elapsed(), "the distances of the picture are read");
    let mut depth = world.resource_mut::<MarkDepth>();
    (depth.distances, depth.mirrors) = (distances, mirrors);
}

const GRID: Color32 = Color32::from_rgba_premultiplied(70, 70, 70, 70);
const MARK: Color32 = Color32::from_rgb(120, 200, 255);
const AREA: Color32 = Color32::from_rgb(255, 170, 60);
const SELECTED: Color32 = Color32::from_rgb(255, 235, 60);
use crate::widgets::AXES;
/// Points of a circle.
const CIRCLE_POINTS: usize = 32;

/// Draws lines of the room on the picture.
struct Pen<'a> {
    painter: egui::Painter,
    view: &'a View,
    /// The top left corner of the picture, in egui points.
    corner: egui::Pos2,
    /// egui points for one pixel.
    points: f32,
    /// `Some`: what is in front of a line hides it. `None`: the line is on top.
    depth: Option<&'a MarkDepth>,
}

impl Pen<'_> {
    fn pos(&self, pixel: [f32; 2]) -> egui::Pos2 {
        self.corner + egui::vec2(pixel[0], pixel[1]) * self.points
    }

    fn line(&self, a: Vec3, b: Vec3, stroke: Stroke) {
        let parts = match self.depth {
            Some(depth) => depth.parts(self.view, a, b),
            None => vec![[a, b]],
        };
        for [a, b] in parts {
            if let Some([a, b]) = self.view.segment(a, b) {
                self.painter
                    .line_segment([self.pos(a), self.pos(b)], stroke);
            }
        }
    }

    fn path(&self, points: &[Vec3], stroke: Stroke) {
        for part in points.windows(2) {
            self.line(part[0], part[1], stroke);
        }
    }

    /// A circle around `center` in the plane of `u` and `v` (each of length 1).
    fn circle(&self, center: Vec3, u: Vec3, v: Vec3, radius: f32, stroke: Stroke) {
        let point = |index: usize| {
            let (sin, cos) =
                (index as f32 / CIRCLE_POINTS as f32 * std::f32::consts::TAU).sin_cos();
            center + (u * cos + v * sin) * radius
        };
        let points: Vec<Vec3> = (0..=CIRCLE_POINTS).map(point).collect();
        self.path(&points, stroke);
    }

    /// The 12 edges of a box between two corners, in the space of `matrix`.
    fn cuboid(&self, matrix: Mat4, low: Vec3, high: Vec3, stroke: Stroke) {
        let corner = |index: usize| {
            let pick =
                |bit: usize, low: f32, high: f32| if index >> bit & 1 == 0 { low } else { high };
            let local = Vec3::new(
                pick(0, low.x, high.x),
                pick(1, low.y, high.y),
                pick(2, low.z, high.z),
            );
            matrix.transform_point3(local)
        };
        for index in 0..8 {
            for bit in 0..3 {
                // Each edge one time: from the corner where its bit is 0.
                if index >> bit & 1 == 0 {
                    self.line(corner(index), corner(index | 1 << bit), stroke);
                }
            }
        }
    }

    fn text(&self, at: Vec3, text: &str, color: Color32) {
        if self.depth.is_some_and(|depth| depth.hides(self.view, at)) {
            return;
        }
        if let Some(pixel) = self.view.project(at) {
            let font = egui::FontId::proportional(13.0);
            let at = self.pos(pixel) + egui::vec2(6.0, -6.0);
            self.painter
                .text(at, egui::Align2::LEFT_BOTTOM, text, font, color);
        }
    }
}

/// While the view is that of the game camera: the frame of the picture of the game.
/// The view of the editor has the shape of its place in the window, so it shows
/// more or less of the room at the sides than the game does.
fn game_frame(world: &mut World, pen: &Pen, picture: egui::Rect, stroke: Stroke) {
    let game = world.resource::<maki_platform::WindowSettings>();
    let aspect = game.width.max(1) as f32 / game.height.max(1) as f32;
    let label = format!("picture of the game, {} x {}", game.width, game.height);
    let settings = world.get_resource::<maki_gameplay::CameraSettings>();
    let game_view = crate::camera::EditorCamera::game_view(&settings.cloned().unwrap_or_default());
    let mut cameras = world.query::<&crate::camera::EditorCamera>();
    if cameras.iter(world).next() != Some(&game_view) {
        return;
    }
    // The same angle from top to bottom: the same height, another width.
    let size = egui::vec2(picture.height() * aspect, picture.height());
    let frame = egui::Rect::from_center_size(picture.center(), size);
    let kind = egui::StrokeKind::Inside;
    pen.painter.rect_stroke(frame, 0.0, stroke, kind);
    let (text, at) = if frame.width() > picture.width() {
        (
            format!("{label}: wider than this view"),
            picture.center_top(),
        )
    } else {
        (label, frame.center_top())
    };
    let font = egui::FontId::proportional(12.0);
    let anchor = egui::Align2::CENTER_TOP;
    pen.painter
        .text(at + egui::vec2(0.0, 4.0), anchor, text, font, stroke.color);
}

/// The panel function of the overlay.
pub(crate) fn draw(ui: &mut Ui, world: &mut World, editor: &Editor) {
    if editor.playing() {
        return;
    }
    let Some(view) = crate::editor_view(world) else {
        return;
    };
    let picture = ui.clip_rect();
    // Out of the world while the pens have it: the marks need the world too.
    let depth = world.remove_resource::<MarkDepth>().unwrap_or_default();
    let pen = |depth| Pen {
        painter: ui.painter().with_clip_rect(picture),
        view: &view,
        corner: picture.min,
        points: 1.0 / ui.ctx().pixels_per_point(),
        depth,
    };
    // `pen` draws on top. `under` draws what no surface hides.
    let (pen, under) = (pen(None), pen(Some(&depth)));
    marks(world, editor, &view, picture, &pen, &under);
    world.insert_resource(depth);
}

fn marks(
    world: &mut World,
    editor: &Editor,
    view: &View,
    picture: egui::Rect,
    pen: &Pen,
    under: &Pen,
) {
    let thin = |color| Stroke::new(1.0, color);
    game_frame(world, pen, picture, thin(GRID));
    let selected = |item: &Item| editor.selection.contains(item);
    // The stroke of a mark: that of the selection if its item is selected.
    let hovered = |item: &Item| editor.hover == Some(crate::Hover::Item(item.clone()));
    let stroke = |item: &Item, color| {
        if selected(item) {
            Stroke::new(2.0, SELECTED)
        } else if hovered(item) {
            // A click takes this item.
            Stroke::new(1.5, Color32::WHITE)
        } else {
            thin(color)
        }
    };
    // The pen of the mark of an item: on top if a click took it, or would take it.
    let pen_of = |item: &Item| {
        if selected(item) || hovered(item) {
            pen
        } else {
            under
        }
    };
    let (room, events) = (&editor.document.room, &editor.document.events);
    let (half_width, half_depth) = (room.width / 2.0, room.depth / 2.0);

    // The floor grid: one line for each meter, from the center of the room.
    let lines = |half: f32| (-(half.floor() as i32)..=half.floor() as i32).map(|line| line as f32);
    for x in lines(half_width) {
        under.line(
            Vec3::new(x, 0.0, -half_depth),
            Vec3::new(x, 0.0, half_depth),
            thin(GRID),
        );
    }
    for z in lines(half_depth) {
        under.line(
            Vec3::new(-half_width, 0.0, z),
            Vec3::new(half_width, 0.0, z),
            thin(GRID),
        );
    }

    for (index, event) in events.iter().enumerate() {
        let (Some(area), item) = (event.area, Item::Event(index)) else {
            continue;
        };
        let corner = |x: f32, z: f32| {
            let [center_x, center_z] = area.center;
            Vec3::new(
                center_x + x * area.size[0] / 2.0,
                0.01,
                center_z + z * area.size[1] / 2.0,
            )
        };
        let corners = [
            (-1.0, -1.0),
            (1.0, -1.0),
            (1.0, 1.0),
            (-1.0, 1.0),
            (-1.0, -1.0),
        ];
        let pen = pen_of(&item);
        pen.path(&corners.map(|(x, z)| corner(x, z)), stroke(&item, AREA));
        pen.text(corner(-1.0, -1.0), &event.name, AREA);
    }

    for (name, position) in &room.spawns {
        let (at, item) = (Vec3::from(*position), Item::Spawn(name.clone()));
        let (stroke, pen) = (stroke(&item, MARK), pen_of(&item));
        pen.circle(at, Vec3::X, Vec3::Z, MARK_RADIUS, stroke);
        pen.line(at, at + Vec3::Y * 2.0 * MARK_RADIUS, stroke);
        pen.text(at + Vec3::Y * 2.0 * MARK_RADIUS, name, MARK);
    }

    for (index, door) in room.doors.iter().enumerate() {
        let Some(doorway) = room.doorways.get(door.doorway) else {
            continue;
        };
        // The hinge and the way of the free edge of the leaf, as the room loader
        // makes the door: it turns into the room.
        let (start, along, length, inward) = doorway.wall.frame(room.width, room.depth);
        let hinge = start + along * doorway.span(length).0;
        let (stroke, pen) = (stroke(&Item::Door(index), MARK), pen_of(&Item::Door(index)));
        let swing = |part: usize| {
            let (sin, cos) = (part as f32 / 8.0 * 85f32.to_radians()).sin_cos();
            hinge + (along * cos + inward * sin) * doorway.width
        };
        pen.line(hinge, hinge + Vec3::Y * doorway.height, stroke);
        pen.path(&(0..=8).map(swing).collect::<Vec<_>>(), stroke);
        pen.line(hinge, swing(8), stroke);
    }

    for (index, mover) in room.movers.iter().enumerate() {
        // The mesh shows at pose `a`: the box of the mesh at pose `b`, and the way.
        let (stroke, pen) = (
            stroke(&Item::Mover(index), MARK),
            pen_of(&Item::Mover(index)),
        );
        pen.line(mover.a.position.into(), mover.b.position.into(), stroke);
        for (_, low, high) in mesh_boxes(world, RoomItem::Mover(index)) {
            pen.cuboid(mover.b.transform().matrix(), low, high, stroke);
        }
    }

    // The item under the pointer: a thin box around its mesh.
    if let Some(crate::Hover::Item(item)) = &editor.hover
        && !selected(item)
    {
        let of_mesh = match item {
            Item::Prop(index) => Some(RoomItem::Prop(*index)),
            Item::Door(index) => Some(RoomItem::Door(*index)),
            Item::Mover(index) => Some(RoomItem::Mover(*index)),
            Item::Prefab(index) => Some(RoomItem::Prefab(*index)),
            _ => None,
        };
        for (matrix, low, high) in of_mesh.map_or(Vec::new(), |item| mesh_boxes(world, item)) {
            pen.cuboid(matrix, low, high, Stroke::new(1.0, Color32::WHITE));
        }
        if let Item::Light(index) = item
            && let Some(light) = room.lights.get(*index)
        {
            let (at, white) = (Vec3::from(light.position), Stroke::new(1.0, Color32::WHITE));
            pen.circle(at, Vec3::X, Vec3::Y, light.radius.max(0.1), white);
        }
    }

    let mark = Stroke::new(2.0, SELECTED);
    for item in &editor.selection {
        let of_mesh = match item {
            Item::Prop(index) => Some(RoomItem::Prop(*index)),
            Item::Door(index) => Some(RoomItem::Door(*index)),
            Item::Mover(index) => Some(RoomItem::Mover(*index)),
            Item::Prefab(index) => Some(RoomItem::Prefab(*index)),
            _ => None,
        };
        for (matrix, low, high) in of_mesh.map_or(Vec::new(), |item| mesh_boxes(world, item)) {
            pen.cuboid(matrix, low, high, mark);
        }
        match item {
            Item::Light(index) => {
                let Some(light) = room.lights.get(*index) else {
                    continue;
                };
                let at = Vec3::from(light.position);
                for (u, v) in [(Vec3::X, Vec3::Y), (Vec3::Y, Vec3::Z), (Vec3::Z, Vec3::X)] {
                    pen.circle(at, u, v, light.radius, mark);
                }
                // The edge of the cone, 2 m long.
                if let Some(degrees) = light.cone.filter(|degrees| *degrees < 180.0) {
                    let direction = light.direction.map_or(Vec3::NEG_Y, Vec3::from);
                    let direction = direction.try_normalize().unwrap_or(Vec3::NEG_Y);
                    let u = direction.any_orthonormal_vector();
                    let v = direction.cross(u);
                    let (sin, cos) = degrees.to_radians().sin_cos();
                    let center = at + direction * 2.0 * cos;
                    pen.circle(center, u, v, 2.0 * sin, mark);
                    for side in [u, -u, v, -v] {
                        pen.line(at, center + side * 2.0 * sin, mark);
                    }
                }
            }
            Item::Doorway(index) => {
                let Some(doorway) = room.doorways.get(*index) else {
                    continue;
                };
                let (start, along, length, _) = doorway.wall.frame(room.width, room.depth);
                let (from, to) = doorway.span(length);
                let (left, right) = (start + along * from, start + along * to);
                let up = Vec3::Y * doorway.height;
                pen.path(&[left, left + up, right + up, right], mark);
            }
            _ => {}
        }
    }

    let Some(gizmo) = editor.gizmo(view) else {
        return;
    };
    let active = editor.dragged();
    // The handle under the pointer is brighter and thicker: a click takes it.
    let over = |handle: Handle| editor.hover == Some(crate::Hover::Handle(handle));
    // The handles of the item itself: a square at each point.
    for (handle, point) in gizmo.points() {
        if handle == Handle::Aim {
            pen.line(gizmo.origin, point, Stroke::new(1.0, SELECTED));
        }
        let Some(pixel) = view.project(point) else {
            continue;
        };
        let side = if over(handle) { 13.0 } else { 9.0 } * pen.points;
        let square = egui::Rect::from_center_size(pen.pos(pixel), egui::Vec2::splat(side));
        let color = if active == Some(handle) {
            SELECTED
        } else {
            Color32::WHITE
        };
        pen.painter.rect_filled(square, 0.0, color);
        let outline = Stroke::new(1.0, Color32::BLACK);
        pen.painter
            .rect_stroke(square, 0.0, outline, egui::StrokeKind::Outside);
    }
    if !gizmo.posed {
        return;
    }
    let color = |axis: usize| {
        let handle = Handle::Axis(axis);
        let color = if active == Some(handle) {
            SELECTED
        } else if over(handle) {
            AXES[axis].lerp_to_gamma(Color32::WHITE, 0.6)
        } else {
            AXES[axis]
        };
        Stroke::new(if over(handle) { 5.0 } else { 3.0 }, color)
    };
    for axis in 0..3 {
        match gizmo.tool {
            Tool::Rotate => pen.path(&gizmo.ring(axis), color(axis)),
            tool => {
                let [from, to] = gizmo.handle(axis);
                pen.line(from, to, color(axis));
                // The end of a handle: a dot to move, a square to scale.
                if let Some(end) = view.project(to) {
                    let (end, size) = (pen.pos(end), 4.0 * pen.points);
                    match tool {
                        Tool::Scale => {
                            let square =
                                egui::Rect::from_center_size(end, egui::Vec2::splat(2.0 * size));
                            pen.painter.rect_filled(square, 0.0, color(axis).color);
                        }
                        _ => drop(pen.painter.circle_filled(end, size, color(axis).color)),
                    }
                }
            }
        }
    }
    if let (Tool::Move, Some(origin)) = (gizmo.tool, view.project(gizmo.origin)) {
        let side = 2.0 * SQUARE * pen.points;
        let square = egui::Rect::from_center_size(pen.pos(origin), egui::Vec2::splat(side));
        let color = if active == Some(Handle::Floor) {
            SELECTED
        } else {
            Color32::WHITE
        };
        let width = if over(Handle::Floor) { 4.0 } else { 2.0 };
        pen.painter.rect_stroke(
            square,
            0.0,
            Stroke::new(width, color),
            egui::StrokeKind::Middle,
        );
    }
}

#[cfg(test)]
mod tests {
    use maki_scene::Camera;

    use super::*;

    #[test]
    fn a_line_is_cut_where_a_surface_is_in_front_of_it() {
        // The camera looks along -Z, from 10 m in front of the origin.
        let camera = Camera {
            position: Vec3::new(0.0, 0.0, 10.0),
            target: Vec3::ZERO,
            fov_y_degrees: 45.0,
        };
        let view = View::new(camera, [200.0, 100.0]);
        // A picture of 2 x 1 places, as 16-bit floats. Left: a surface at 8 m.
        // Right: one at 16 m.
        let depth = |left: u16, right: u16| MarkDepth {
            distances: Some(Distances::from_halves(2, 1, vec![left, right])),
            ..MarkDepth::default()
        };
        let cut = depth(0x4800, 0x4C00);
        // A point at 10 m: behind the left surface, in front of the right one.
        assert!(cut.hides(&view, Vec3::new(-1.0, 0.0, 0.0)));
        assert!(!cut.hides(&view, Vec3::new(1.0, 0.0, 0.0)));
        // A point on a surface is not hidden by it: a mark on the floor.
        assert!(!cut.hides(&view, Vec3::new(-0.5, 0.0, 2.0)));
        // A line from the left to the right, at 10 m: its right half is left.
        let parts = cut.parts(&view, Vec3::new(-2.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0));
        assert_eq!(parts.len(), 1);
        let [from, to] = parts[0];
        // The cut is at a step of the test, some pixels from the edge.
        assert!(from.x.abs() < 0.2 && to.x == 2.0, "{parts:?}");
        // Nothing in the way (0.0: the ray hit nothing): the whole line.
        let open = depth(0, 0);
        let parts = open.parts(&view, Vec3::new(-2.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0));
        assert_eq!(
            parts,
            [[Vec3::new(-2.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0)]]
        );
        // No picture yet: nothing is hidden.
        assert!(!MarkDepth::default().hides(&view, Vec3::new(-1.0, 0.0, 0.0)));
    }

    #[test]
    fn a_mirror_hides_what_is_behind_it() {
        use maki_assets::{Assets, Material, Mesh};
        use maki_core::Transform;
        use maki_scene::MeshInstance;

        let camera = Camera {
            position: Vec3::new(0.0, 0.0, 10.0),
            target: Vec3::ZERO,
            fov_y_degrees: 45.0,
        };
        let view = View::new(camera, [200.0, 100.0]);
        // Two cubes, 5 m from the camera: a mirror at the left, a wall at the right.
        let mut world = World::new();
        let mut meshes = Assets::<Mesh>::default();
        let mirror = meshes.add(Mesh::cube(1.0).with_material(Material::Mirror));
        let wall = meshes.add(Mesh::cube(1.0));
        world.insert_resource(meshes);
        let at = |x: f32| Transform::from_translation(Vec3::new(x, 0.0, 5.0));
        world.spawn((at(-1.0), MeshInstance(mirror)));
        world.spawn((at(1.0), MeshInstance(wall)));
        // The picture has, for the mirror, what the mirror shows: 16 m away. Here
        // it has the same for the wall, to show that only a mirror is tested.
        let depth = MarkDepth {
            distances: Some(Distances::from_halves(2, 1, vec![0x4C00, 0x4C00])),
            mirrors: Mirrors::of(&mut world),
            samples: 0,
        };
        assert!(depth.hides(&view, Vec3::new(-1.0, 0.0, 0.0)));
        assert!(!depth.hides(&view, Vec3::new(1.0, 0.0, 0.0)));
        // In front of the mirror.
        assert!(!depth.hides(&view, Vec3::new(-1.0, 0.0, 7.0)));
    }
}
