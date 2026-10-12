//! The marks of the editor on the picture: the grid of the floor, the things that
//! have no mesh (spawn points, event areas, the swing of a door, the second pose of a
//! mover), the selection, and the gizmo.
//!
//! All are lines and text of the egui painter, from points of the room that `View`
//! turns into pixels. Not a line pass of the frame graph: the marks then are drawn
//! by the UI pass, after the tone curve, with no new GPU state. They are not in the
//! path tracer, the denoiser, or the anti-aliasing, and they have no depth test: a
//! mark behind an object shows through it, which a mark must do.

use bevy_ecs::world::World;
use glam::{Mat4, Vec3};
use maki_scene::RoomItem;
use maki_ui::egui::{self, Color32, Stroke, Ui};

use crate::gizmo::{Handle, SQUARE, Tool};
use crate::view::{MARK_RADIUS, View, mesh_boxes};
use crate::{Editor, Item};

const GRID: Color32 = Color32::from_rgba_premultiplied(70, 70, 70, 70);
const MARK: Color32 = Color32::from_rgb(120, 200, 255);
const AREA: Color32 = Color32::from_rgb(255, 170, 60);
const SELECTED: Color32 = Color32::from_rgb(255, 235, 60);
/// X, Y, Z.
const AXES: [Color32; 3] = [
    Color32::from_rgb(235, 80, 80),
    Color32::from_rgb(90, 210, 90),
    Color32::from_rgb(90, 140, 255),
];
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
}

impl Pen<'_> {
    fn pos(&self, pixel: [f32; 2]) -> egui::Pos2 {
        self.corner + egui::vec2(pixel[0], pixel[1]) * self.points
    }

    fn line(&self, a: Vec3, b: Vec3, stroke: Stroke) {
        if let Some([a, b]) = self.view.segment(a, b) {
            self.painter
                .line_segment([self.pos(a), self.pos(b)], stroke);
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
        if let Some(pixel) = self.view.project(at) {
            let font = egui::FontId::proportional(13.0);
            let at = self.pos(pixel) + egui::vec2(6.0, -6.0);
            self.painter
                .text(at, egui::Align2::LEFT_BOTTOM, text, font, color);
        }
    }
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
    let pen = Pen {
        painter: ui.painter().with_clip_rect(picture),
        view: &view,
        corner: picture.min,
        points: 1.0 / ui.ctx().pixels_per_point(),
    };
    let thin = |color| Stroke::new(1.0, color);
    let selected = |item: &Item| editor.selection.contains(item);
    // The stroke of a mark: that of the selection if its item is selected.
    let stroke = |item: &Item, color| {
        if selected(item) {
            Stroke::new(2.0, SELECTED)
        } else {
            thin(color)
        }
    };
    let (room, events) = (&editor.document.room, &editor.document.events);
    let (half_width, half_depth) = (room.width / 2.0, room.depth / 2.0);

    // The floor grid: one line for each meter, from the center of the room.
    let lines = |half: f32| (-(half.floor() as i32)..=half.floor() as i32).map(|line| line as f32);
    for x in lines(half_width) {
        pen.line(
            Vec3::new(x, 0.0, -half_depth),
            Vec3::new(x, 0.0, half_depth),
            thin(GRID),
        );
    }
    for z in lines(half_depth) {
        pen.line(
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
        pen.path(&corners.map(|(x, z)| corner(x, z)), stroke(&item, AREA));
        pen.text(corner(-1.0, -1.0), &event.name, AREA);
    }

    for (name, position) in &room.spawns {
        let (at, item) = (Vec3::from(*position), Item::Spawn(name.clone()));
        let stroke = stroke(&item, MARK);
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
        let stroke = stroke(&Item::Door(index), MARK);
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
        let stroke = stroke(&Item::Mover(index), MARK);
        pen.line(mover.a.position.into(), mover.b.position.into(), stroke);
        for (_, low, high) in mesh_boxes(world, RoomItem::Mover(index)) {
            pen.cuboid(mover.b.transform().matrix(), low, high, stroke);
        }
    }

    let mark = Stroke::new(2.0, SELECTED);
    for item in &editor.selection {
        let of_mesh = match item {
            Item::Prop(index) => Some(RoomItem::Prop(*index)),
            Item::Door(index) => Some(RoomItem::Door(*index)),
            Item::Mover(index) => Some(RoomItem::Mover(*index)),
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

    let Some(gizmo) = editor.gizmo(&view) else {
        return;
    };
    let active = editor.dragged();
    let color = |axis: usize| {
        let color = if active == Some(Handle::Axis(axis)) {
            SELECTED
        } else {
            AXES[axis]
        };
        Stroke::new(3.0, color)
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
        pen.painter.rect_stroke(
            square,
            0.0,
            Stroke::new(2.0, color),
            egui::StrokeKind::Middle,
        );
    }
}
