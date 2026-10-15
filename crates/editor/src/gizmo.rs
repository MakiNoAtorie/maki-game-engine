//! The gizmo: handles at the selected item that the mouse drags to move, turn, and
//! scale it.
//!
//! Our own, not a crate: the handles are lines that the overlay draws with plain egui
//! shapes, and the math is a ray against a line (`Ray::along`), which the pick has.
//!
//! - Move: one handle for each axis, and a square at the origin that moves the item
//!   on the floor plane (the most used one: a room is laid out from above).
//! - Rotate: one ring for each axis. The turn is the angle of the pointer around the
//!   origin, on the screen.
//! - Scale: one handle for each axis of the item itself.
//! - Handles of the item itself, with each tool ([`Extra`]): where a cone light
//!   points, the corners of an event area, the top corners of a doorway.
//!
//! A drag keeps the room of its start, and each frame of the drag makes the room
//! again from that start and the full motion: no error adds up, and a snap is a snap
//! of the result. The document changes in each frame; the history takes the drag as
//! one step.

use glam::{EulerRot, Quat, Vec3};
use maki_events::EventDef;
use maki_scene::{PoseDef, RoomDef};

use crate::document::Place;
use crate::view::{Ray, View};
use crate::{Document, Item};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Move,
    Rotate,
    Scale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    /// 0, 1, 2: X, Y, Z.
    Axis(usize),
    /// The square at the origin: a move on the plane of the floor.
    Floor,
    /// Where the middle of the cone of a light meets the floor.
    Aim,
    /// A corner of an event area (0 to 3, around it), or a top corner of a doorway
    /// (0: toward the start of the wall, 1: the other one).
    Corner(usize),
}

/// The handles that an item has beside those of the tool.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Extra {
    None,
    /// A cone light: the point that its handle is at.
    Aim(Vec3),
    /// This many corners.
    Corners([Vec3; 4], usize),
}

/// The corners of an event area, around it: the signs of the half size in X and Z.
/// The corner across from corner `n` is corner `n + 2`.
const AREA_CORNERS: [[f32; 2]; 4] = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];
/// A drag of a corner makes no area and no doorway smaller than this, in meters.
const SMALLEST: f32 = 0.1;
/// The aim handle of a light that does not point at the floor is this far from the
/// light, in meters: the length of the cone that the overlay draws.
const AIM_LENGTH: f32 = 2.0;

impl Extra {
    fn of(document: &Document, item: &Item) -> Self {
        let room = &document.room;
        if let Some(light) = document.light(item) {
            if light.cone.is_none_or(|degrees| degrees >= 180.0) {
                return Self::None;
            }
            let (at, to) = (Vec3::from(light.position), light.light().direction);
            // Where the middle of the cone meets the floor, if it does near by.
            let on_floor = (to.y < -1e-3).then(|| at + to * (at.y / -to.y));
            let near = on_floor.filter(|point| point.distance(at) < 50.0);
            return Self::Aim(near.unwrap_or(at + to * AIM_LENGTH));
        }
        if let Some(doorway) = document.doorway(item) {
            let (start, along, length, _) = doorway.wall.frame(room.width, room.depth);
            let (from, to) = doorway.span(length);
            let top = |along_wall: f32| start + along * along_wall + Vec3::Y * doorway.height;
            return Self::Corners([top(from), top(to), Vec3::ZERO, Vec3::ZERO], 2);
        }
        match item {
            Item::Event(index) => {
                let area = document.events.get(*index).and_then(|event| event.area);
                let Some(area) = area else {
                    return Self::None;
                };
                let corner = |[x, z]: [f32; 2]| {
                    let [center_x, center_z] = area.center;
                    let (half_x, half_z) = (area.size[0] / 2.0, area.size[1] / 2.0);
                    Vec3::new(center_x + x * half_x, 0.0, center_z + z * half_z)
                };
                Self::Corners(AREA_CORNERS.map(corner), 4)
            }
            _ => Self::None,
        }
    }
}

/// Length of a handle on the screen, in pixels.
const LENGTH: f32 = 90.0;
/// The pointer takes a handle that is not farther away than this, in pixels.
const REACH: f32 = 9.0;
/// Half of the side of the floor square, in pixels.
pub(crate) const SQUARE: f32 = 9.0;
/// A scale snaps to steps of this.
const SCALE_STEP: f32 = 0.05;
/// Points of a ring.
const RING_POINTS: usize = 48;

/// Where the handles are in this frame.
#[derive(Debug, Clone, Copy)]
pub struct Gizmo {
    pub origin: Vec3,
    /// The directions of the three handles, each of length 1.
    pub axes: [Vec3; 3],
    /// Length of a handle in meters: the same size on the screen at each distance.
    pub length: f32,
    pub tool: Tool,
    /// False: the item has no pose (a doorway), and the gizmo has no handles of the
    /// tool.
    pub posed: bool,
    pub extra: Extra,
}

impl Gizmo {
    /// The gizmo of the last selected item. `None`: nothing is selected that has a
    /// place or a handle of its own (the room, a door). `local`: the axes of the
    /// item, not those of the room; a scale is always along the axes of the item.
    pub fn of(
        document: &Document,
        item: &Item,
        second: bool,
        local: bool,
        tool: Tool,
        view: &View,
    ) -> Option<Self> {
        let extra = Extra::of(document, item);
        let pose = document.pose_of(item, second);
        let (origin, rotation) = match (pose, extra) {
            (Some(pose), _) => pose,
            // No pose: the gizmo is the corners.
            (None, Extra::Corners(corners, count)) => {
                let sum: Vec3 = corners[..count].iter().sum();
                (sum / count as f32, Quat::IDENTITY)
            }
            (None, _) => return None,
        };
        let rotation = if local || tool == Tool::Scale {
            rotation
        } else {
            Quat::IDENTITY
        };
        Some(Self {
            origin,
            axes: [Vec3::X, Vec3::Y, Vec3::Z].map(|axis| rotation * axis),
            length: LENGTH * view.meters_per_pixel(origin),
            tool,
            posed: pose.is_some(),
            extra,
        })
    }

    /// The handles of the item itself, each with its place.
    pub fn points(&self) -> Vec<(Handle, Vec3)> {
        match self.extra {
            Extra::None => Vec::new(),
            Extra::Aim(point) => vec![(Handle::Aim, point)],
            Extra::Corners(corners, count) => {
                let corner = |index: usize| (Handle::Corner(index), corners[index]);
                (0..count).map(corner).collect()
            }
        }
    }

    /// The two ends of the handle of an axis.
    pub fn handle(&self, axis: usize) -> [Vec3; 2] {
        let along = self.axes[axis] * self.length;
        [self.origin + along * 0.2, self.origin + along]
    }

    /// The points of the ring of an axis, closed.
    pub fn ring(&self, axis: usize) -> Vec<Vec3> {
        let (u, v) = (self.axes[(axis + 1) % 3], self.axes[(axis + 2) % 3]);
        let point = |index: usize| {
            let (sin, cos) = (index as f32 / RING_POINTS as f32 * std::f32::consts::TAU).sin_cos();
            self.origin + (u * cos + v * sin) * self.length
        };
        (0..=RING_POINTS).map(point).collect()
    }

    /// The handle under a pixel.
    pub fn handle_at(&self, view: &View, pixel: [f32; 2]) -> Option<Handle> {
        // First: a handle of the item is a small square, also at the place of a
        // handle of the tool.
        let on_point = |(_, point): &(Handle, Vec3)| {
            let at = view.project(*point);
            at.is_some_and(|at| (0..2).all(|side| (pixel[side] - at[side]).abs() <= REACH))
        };
        if let Some((handle, _)) = self.points().into_iter().find(on_point) {
            return Some(handle);
        }
        if !self.posed {
            return None;
        }
        let origin = view.project(self.origin)?;
        if self.tool == Tool::Move
            && (0..2).all(|side| (pixel[side] - origin[side]).abs() <= SQUARE)
        {
            return Some(Handle::Floor);
        }
        let reach = |axis: usize| {
            let line = match self.tool {
                Tool::Rotate => self.ring(axis),
                _ => self.handle(axis).to_vec(),
            };
            let parts = line
                .windows(2)
                .filter_map(|part| view.segment(part[0], part[1]));
            parts
                .map(|[a, b]| distance_to_segment(pixel, a, b))
                .fold(f32::MAX, f32::min)
        };
        let nearest = (0..3)
            .map(|axis| (reach(axis), axis))
            .min_by(|a, b| a.0.total_cmp(&b.0))?;
        (nearest.0 <= REACH).then_some(Handle::Axis(nearest.1))
    }
}

fn distance_to_segment(point: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (point, a, b) = (
        glam::Vec2::from(point),
        glam::Vec2::from(a),
        glam::Vec2::from(b),
    );
    let along = ((point - a).dot(b - a) / (b - a).length_squared().max(1e-6)).clamp(0.0, 1.0);
    point.distance(a + (b - a) * along)
}

/// The angle of a pixel around a center, on the screen.
fn screen_angle(center: [f32; 2], pixel: [f32; 2]) -> f32 {
    (pixel[1] - center[1]).atan2(pixel[0] - center[0])
}

fn snap(value: f32, step: f32) -> f32 {
    if step > 0.0 {
        (value / step).round() * step
    } else {
        value
    }
}

/// A drag of a handle.
pub struct Drag {
    pub handle: Handle,
    gizmo: Gizmo,
    /// True: the axes are those of the item, so a snap is a snap of the way, not of
    /// the place.
    local: bool,
    /// Where the pointer took the handle: the distance along the axis, the angle on
    /// the screen, or the point of the floor plane.
    grab: f32,
    grab_point: Vec3,
    /// The room and the events when the drag started.
    start: (RoomDef, Vec<EventDef>),
}

impl Drag {
    pub fn start(
        gizmo: Gizmo,
        handle: Handle,
        local: bool,
        view: &View,
        pixel: [f32; 2],
        document: &Document,
    ) -> Self {
        let ray = view.ray(pixel);
        let grab = match (gizmo.tool, handle) {
            (_, Handle::Aim | Handle::Corner(_)) => 0.0,
            (Tool::Rotate, _) => view
                .project(gizmo.origin)
                .map_or(0.0, |center| screen_angle(center, pixel)),
            (_, Handle::Axis(axis)) => ray.along(gizmo.origin, gizmo.axes[axis]),
            (_, Handle::Floor) => 0.0,
        };
        Self {
            handle,
            gizmo,
            local,
            grab,
            grab_point: floor_point(&ray, gizmo.origin),
            start: (document.room.clone(), document.events.clone()),
        }
    }

    /// Ends the drag with no change: the document gets the values of its start.
    pub fn cancel(self, document: &mut Document) {
        (document.room, document.events) = self.start;
    }

    /// Makes the document from the start of the drag and the place of the pointer
    /// now. `steps`: the grid step and the angle step (degrees) to snap to; `None`:
    /// no snap. `second`: the gizmo is at the second pose of a mover.
    pub fn apply(
        &self,
        view: &View,
        pixel: [f32; 2],
        steps: Option<(f32, f32)>,
        document: &mut Document,
        selection: &[Item],
        second: bool,
    ) {
        let (grid, angle_step) = steps.unwrap_or((0.0, 0.0));
        let Gizmo {
            origin, axes, tool, ..
        } = self.gizmo;
        let ray = view.ray(pixel);
        document.room.clone_from(&self.start.0);
        document.events.clone_from(&self.start.1);
        document.dirty = true;
        // Tenths of a millimeter: no 2.4999998 in a room file.
        let round = |value: f32| (value * 1e4).round() / 1e4;

        match (tool, self.handle) {
            // The handles of the item itself: of the item that has the gizmo.
            (_, Handle::Aim) => {
                let Some(item) = selection.last() else {
                    return;
                };
                let (light, floor) = (document.light(item), ray.height(0.0));
                let (Some(mut light), Some((floor, _))) = (light, floor) else {
                    return;
                };
                let target = Vec3::new(snap(floor.x, grid), 0.0, snap(floor.z, grid));
                let to = (target - Vec3::from(light.position)).try_normalize();
                if let Some(to) = to {
                    // Thousandths: the direction is made to length 1 when it is read.
                    light.direction = Some(to.to_array().map(|part| (part * 1e3).round() / 1e3));
                    document.set_light(item, light);
                }
            }
            (_, Handle::Corner(corner)) => match selection.last() {
                Some(Item::Event(index)) => {
                    let area = document.events.get_mut(*index);
                    let area = area.and_then(|event| event.area.as_mut());
                    let (Some(area), Some((floor, _))) = (area, ray.height(0.0)) else {
                        return;
                    };
                    let to = [snap(floor.x, grid), snap(floor.z, grid)];
                    let across = AREA_CORNERS[(corner + 2) % 4];
                    for axis in 0..2 {
                        // The corner across stays where it is.
                        let fixed = area.center[axis] + across[axis] * area.size[axis] / 2.0;
                        area.center[axis] = round((fixed + to[axis]) / 2.0);
                        area.size[axis] = round((to[axis] - fixed).abs().max(SMALLEST));
                    }
                }
                Some(item @ Item::Object(_)) => {
                    let room = &document.room;
                    let (width, depth, height) = (room.width, room.depth, room.height);
                    let Some(mut doorway) = document.doorway(item) else {
                        return;
                    };
                    let (start, along, length, inward) = doorway.wall.frame(width, depth);
                    // Where the ray meets the plane of the wall.
                    let toward = ray.direction.dot(inward);
                    let t = (start - ray.origin).dot(inward) / toward;
                    if toward.abs() < 1e-4 || t <= 0.0 {
                        return;
                    }
                    let hit = ray.at(t);
                    // The place along the wall, on the grid of the room.
                    let at = snap(hit.dot(along), grid) - start.dot(along);
                    let (from, to) = doorway.span(length);
                    // The other side of the opening stays where it is.
                    let (from, to) = if corner == 0 {
                        (at.clamp(0.0, to - SMALLEST), to)
                    } else {
                        (from, at.clamp(from + SMALLEST, length))
                    };
                    doorway.width = round(to - from);
                    doorway.offset = round((from + to) / 2.0 - length / 2.0);
                    doorway.height = round(snap(hit.y, grid).clamp(SMALLEST, height));
                    document.set_doorway(item, doorway);
                }
                _ => {}
            },
            (Tool::Move, handle) => {
                let offset = match handle {
                    Handle::Axis(axis) => {
                        let way = ray.along(origin, axes[axis]) - self.grab;
                        // Along an axis of the room, the place snaps to the grid: an
                        // item that was off the grid goes onto it.
                        let at = origin.dot(axes[axis]);
                        let way = if self.local {
                            snap(way, grid)
                        } else {
                            snap(at + way, grid) - at
                        };
                        axes[axis] * way
                    }
                    Handle::Floor => {
                        let to = origin + floor_point(&ray, origin) - self.grab_point;
                        Vec3::new(snap(to.x, grid), origin.y, snap(to.z, grid)) - origin
                    }
                    Handle::Aim | Handle::Corner(_) => return,
                };
                let moved = |from: f32, by: f32| round(from + by);
                let moved_point = |point: &mut [f32; 3]| {
                    *point = [0, 1, 2].map(|axis| moved(point[axis], offset[axis]));
                };
                for item in selection {
                    // A mover moves as a whole: its second pose goes with the first.
                    let b_only = second && document.mover(item).is_some();
                    if document.with_mover_b(item, |b| moved_point(&mut b.position)) && b_only {
                        continue;
                    }
                    match document.place(item) {
                        Some(Place::Pose(pose)) => moved_point(&mut pose.position),
                        Some(Place::Area(area)) => {
                            area.center = [
                                moved(area.center[0], offset.x),
                                moved(area.center[1], offset.z),
                            ];
                        }
                        None => {}
                    }
                }
            }
            (Tool::Rotate, Handle::Axis(axis)) => {
                let Some(center) = view.project(origin) else {
                    return;
                };
                // On the screen a larger angle is a turn to the right (Y goes down).
                // That is a turn in the negative direction around an axis that
                // points to the viewer.
                let turned = screen_angle(center, pixel) - self.grab;
                let to_viewer = axes[axis].dot(view.camera.position - origin) > 0.0;
                let angle = if to_viewer { -turned } else { turned };
                let angle = snap(angle.to_degrees(), angle_step).to_radians();
                let turn = Quat::from_axis_angle(axes[axis], angle);
                // The items turn as a group, around the gizmo: each place goes
                // around it. The item that has the gizmo stays at its place.
                let around = |point: &mut [f32; 3]| {
                    let from_origin = Vec3::from(*point) - origin;
                    if from_origin.length() > 1e-5 {
                        *point = (origin + turn * from_origin).to_array().map(round);
                    }
                };
                let turned = |pose: &mut PoseDef| {
                    let (y, x, z) = (turn * pose.transform().rotation).to_euler(EulerRot::YXZ);
                    // Hundredths of a degree: no 29.999998 in a room file.
                    pose.rotation =
                        [x, y, z].map(|angle| (angle.to_degrees() * 100.0).round() / 100.0 + 0.0);
                    around(&mut pose.position);
                };
                for item in selection {
                    // A mover turns as a whole, as it moves.
                    let b_only = second && document.mover(item).is_some();
                    if document.with_mover_b(item, |pose| turned(pose)) && b_only {
                        continue;
                    }
                    match document.place(item) {
                        Some(Place::Pose(pose)) => turned(pose),
                        // An area has no turn: its sides are along X and Z.
                        Some(Place::Area(_)) | None => {}
                    }
                }
            }
            (Tool::Scale, Handle::Axis(axis)) => {
                let factor = ray.along(origin, axes[axis]) / self.grab.max(1e-3);
                let step = if steps.is_some() { SCALE_STEP } else { 0.0 };
                let scaled = |pose: &mut PoseDef| {
                    pose.scale[axis] = snap(pose.scale[axis] * factor, step).max(0.01);
                };
                for item in selection {
                    if second && document.with_mover_b(item, |pose| scaled(pose)) {
                        continue;
                    }
                    if let Some(Place::Pose(pose)) = document.place(item) {
                        scaled(pose);
                    }
                }
            }
            (_, Handle::Floor) => {}
        }
    }
}

/// Where a ray meets the plane of the height of `origin`. `origin` itself for a ray
/// that does not meet it.
fn floor_point(ray: &Ray, origin: Vec3) -> Vec3 {
    ray.height(origin.y).map_or(origin, |hit| hit.0)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use maki_scene::{Camera, PropDef};

    use super::*;

    /// A view from the +Z side and above, and a room with one cube at (1.1, 0.5, 0):
    /// object 2, behind the lamp and the spawn point of a new room.
    fn scene() -> (View, Document, [Item; 1]) {
        let camera = Camera {
            position: Vec3::new(0.0, 9.0, 7.0),
            target: Vec3::new(0.0, 0.5, 0.0),
            fov_y_degrees: 45.0,
        };
        let mut document = Document::new_room(PathBuf::from("room.ron"));
        let mut cube = PropDef::default();
        cube.pose.position = [1.1, 0.5, 0.0];
        document.room.objects.push(cube.into_object());
        (
            View::new(camera, [1280.0, 720.0]),
            document,
            [Item::Object(2)],
        )
    }

    fn drag(
        tool: Tool,
        handle: Handle,
        to: impl Fn(&Gizmo, &View) -> Vec3,
        steps: Option<(f32, f32)>,
    ) -> maki_scene::PoseDef {
        let (view, mut document, selection) = scene();
        let gizmo = Gizmo::of(&document, &selection[0], false, false, tool, &view).unwrap();
        let from = match handle {
            Handle::Axis(axis) if tool == Tool::Rotate => gizmo.ring(axis)[0],
            Handle::Axis(axis) => gizmo.handle(axis)[1],
            _ => gizmo.origin,
        };
        let from = view.project(from).unwrap();
        assert_eq!(gizmo.handle_at(&view, from), Some(handle));
        let drag = Drag::start(gizmo, handle, false, &view, from, &document);
        // Two frames: the second one starts again from the room of the start.
        let to = view.project(to(&gizmo, &view)).unwrap();
        drag.apply(
            &view,
            [to[0] + 40.0, to[1]],
            steps,
            &mut document,
            &selection,
            false,
        );
        drag.apply(&view, to, steps, &mut document, &selection, false);
        document.room.objects[2].pose
    }

    #[test]
    fn a_drag_of_an_axis_moves_along_it_and_snaps_to_the_grid() {
        // The end of the X handle goes 1.37 m to the right.
        let to = |gizmo: &Gizmo, _: &View| gizmo.handle(0)[1] + Vec3::X * 1.37;
        let free = drag(Tool::Move, Handle::Axis(0), to, None);
        assert!((free.position[0] - 2.47).abs() < 1e-3, "{free:?}");
        assert_eq!(free.position[1..], [0.5, 0.0]);
        // With a grid of 0.25 the place is on the grid, not the way.
        let snapped = drag(Tool::Move, Handle::Axis(0), to, Some((0.25, 15.0)));
        assert_eq!(snapped.position, [2.5, 0.5, 0.0]);
    }

    #[test]
    fn a_drag_of_the_square_moves_on_the_floor_plane() {
        let to = |gizmo: &Gizmo, _: &View| gizmo.origin + Vec3::new(-2.0, 0.0, 1.3);
        let free = drag(Tool::Move, Handle::Floor, to, None);
        assert!(
            (Vec3::from(free.position) - Vec3::new(-0.9, 0.5, 1.3)).length() < 1e-3,
            "{free:?}"
        );
        let snapped = drag(Tool::Move, Handle::Floor, to, Some((0.5, 15.0)));
        assert_eq!(snapped.position, [-1.0, 0.5, 1.5]);
    }

    #[test]
    fn a_drag_of_a_ring_turns_around_its_axis() {
        // A quarter of the Y ring: from its first point (+Z of the item) to +X.
        let to = |gizmo: &Gizmo, _: &View| gizmo.origin + Vec3::X * gizmo.length;
        let turned = drag(Tool::Rotate, Handle::Axis(1), to, Some((0.25, 15.0)));
        // +Z goes to +X: a quarter turn around Y, in the positive direction.
        assert_eq!(turned.rotation, [0.0, 90.0, 0.0]);
        assert_eq!(turned.position, [1.1, 0.5, 0.0]);
    }

    /// A drag of a handle of the item itself, to the point of the room that `to`
    /// gives. Returns the document.
    fn drag_point(mut document: Document, item: Item, handle: Handle, to: Vec3) -> Document {
        let (view, ..) = scene();
        let selection = [item];
        let gizmo = Gizmo::of(&document, &selection[0], false, false, Tool::Move, &view).unwrap();
        let (_, from) = gizmo
            .points()
            .into_iter()
            .find(|point| point.0 == handle)
            .unwrap();
        let from = view.project(from).unwrap();
        assert_eq!(gizmo.handle_at(&view, from), Some(handle));
        let drag = Drag::start(gizmo, handle, false, &view, from, &document);
        let to = view.project(to).unwrap();
        let steps = Some((0.25, 15.0));
        drag.apply(&view, to, steps, &mut document, &selection, false);
        document
    }

    #[test]
    fn a_drag_of_the_aim_handle_points_a_cone_light_at_the_floor() {
        let (_, mut document, _) = scene();
        // The lamp of a new room, at (0, 2.4, 0), as a cone that points down.
        let lamp = Item::Object(0);
        let cone = |document: &mut Document, cone| {
            let mut light = document.light(&lamp).unwrap();
            light.cone = cone;
            document.set_light(&lamp, light);
        };
        cone(&mut document, Some(30.0));
        let (view, ..) = scene();
        let gizmo = Gizmo::of(&document, &lamp, false, false, Tool::Move, &view).unwrap();
        assert_eq!(gizmo.points(), [(Handle::Aim, Vec3::ZERO)]);

        // To a point of the floor, 2.4 m to the right: on the grid, that is 2.5 m.
        let aimed = drag_point(
            document.clone(),
            lamp.clone(),
            Handle::Aim,
            Vec3::new(2.4, 0.0, 0.0),
        );
        assert_eq!(
            aimed.light(&lamp).unwrap().direction,
            Some([0.721, -0.693, 0.0])
        );
        // The handle is where the light points now.
        let gizmo = Gizmo::of(&aimed, &lamp, false, false, Tool::Move, &view).unwrap();
        assert!(gizmo.points()[0].1.distance(Vec3::new(2.5, 0.0, 0.0)) < 0.01);
        // A light with no cone has no such handle.
        cone(&mut document, None);
        let gizmo = Gizmo::of(&document, &lamp, false, false, Tool::Move, &view).unwrap();
        assert!(gizmo.points().is_empty());
    }

    #[test]
    fn a_drag_of_a_corner_sizes_an_event_area_and_a_doorway() {
        use maki_assets::{Doorway, Wall};
        use maki_events::{Area, EventDef};

        let (view, mut document, _) = scene();
        document.events.push(EventDef {
            name: "area".to_owned(),
            area: Some(Area {
                center: [1.0, 1.0],
                size: [2.0, 1.0],
            }),
            pages: Vec::new(),
        });
        // Corner 2 is at (2, 1.5). To (3.1, 2.6): on the grid that is (3, 2.5).
        let item = Item::Event(0);
        let to = Vec3::new(3.1, 0.0, 2.6);
        let sized = drag_point(document.clone(), item.clone(), Handle::Corner(2), to);
        // The corner across, at (0, 0.5), did not move.
        let area = sized.events[0].area.unwrap();
        assert_eq!((area.center, area.size), ([1.5, 1.5], [3.0, 2.0]));
        // Past the corner across: the area does not turn inside out or go away.
        let to = Vec3::new(0.0, 0.0, 0.5);
        let small = drag_point(document.clone(), item, Handle::Corner(2), to);
        assert_eq!(small.events[0].area.unwrap().size, [0.1, 0.1]);

        // A doorway of 1 x 2 m in the middle of the back wall of the room of 8 m:
        // from 3.5 to 4.5 m along the wall, which is from x = -0.5 to 0.5.
        let mut doorway = maki_scene::ObjectDef::default();
        let opening = Doorway {
            wall: Wall::NegZ,
            offset: 0.0,
            width: 1.0,
            height: 2.0,
            depth: 1.0,
        };
        doorway.set_component("doorway", &opening);
        document.room.objects.push(doorway);
        let item = Item::Object(3);
        let gizmo = Gizmo::of(&document, &item, false, false, Tool::Move, &view).unwrap();
        assert!(!gizmo.posed && gizmo.points().len() == 2);
        assert_eq!(gizmo.points()[1].1, Vec3::new(0.5, 2.0, -4.0));
        // The right top corner goes to x = 1.5 and up to 2.5 m.
        let to = Vec3::new(1.5, 2.5, -4.0);
        let wide = drag_point(document.clone(), item.clone(), Handle::Corner(1), to);
        let doorway = wide.doorway(&item).unwrap();
        assert_eq!(
            (doorway.width, doorway.offset, doorway.height),
            (2.0, 0.5, 2.5)
        );
        // Past the end of the wall and above the room: the end, and the height of
        // the room.
        let to = Vec3::new(-6.0, 3.6, -4.0);
        let end = drag_point(document, item, Handle::Corner(0), to);
        let doorway = end.doorway(&Item::Object(3)).unwrap();
        assert_eq!(
            (doorway.width, doorway.offset, doorway.height),
            (4.5, -1.75, 3.0)
        );
    }

    #[test]
    fn a_turn_of_two_items_turns_the_group_and_a_move_takes_the_two_poses_of_a_mover() {
        use maki_scene::{MeshDef, MoverDef};

        let (view, mut document, _) = scene();
        // The cube of the scene is at (1.1, 0.5, 0). A mover, 1 m beside it.
        let at = |x: f32| PoseDef {
            position: [x, 0.5, 0.0],
            ..PoseDef::default()
        };
        let mover = MoverDef {
            name: None,
            mesh: MeshDef::Cube(0.5),
            look: Default::default(),
            a: at(2.1),
            b: PoseDef {
                position: [2.1, 1.5, 0.0],
                ..PoseDef::default()
            },
            seconds: 1.0,
        };
        document.room.objects.push(mover.into_object());
        // The gizmo is at the last item: the cube.
        let selection = [Item::Object(3), Item::Object(2)];
        let drag = |tool: Tool, handle: Handle, to: &dyn Fn(&Gizmo) -> Vec3| {
            let mut document = document.clone();
            let gizmo = Gizmo::of(&document, &selection[1], false, false, tool, &view).unwrap();
            let from = match (tool, handle) {
                (Tool::Rotate, Handle::Axis(axis)) => gizmo.ring(axis)[0],
                _ => gizmo.origin,
            };
            let from = view.project(from).unwrap();
            let drag = Drag::start(gizmo, handle, false, &view, from, &document);
            let to = view.project(to(&gizmo)).unwrap();
            let steps = Some((0.25, 15.0));
            drag.apply(&view, to, steps, &mut document, &selection, false);
            document
        };
        // A quarter turn around Y (+Z goes to +X): the mover goes around the cube,
        // from 1 m at its +X side to 1 m at its -Z side, with its two poses.
        let turned = drag(Tool::Rotate, Handle::Axis(1), &|gizmo| {
            gizmo.origin + Vec3::X * gizmo.length
        });
        assert_eq!(turned.room.objects[2].pose.position, [1.1, 0.5, 0.0]);
        assert_eq!(turned.room.objects[2].pose.rotation, [0.0, 90.0, 0.0]);
        let mover = &turned.mover(&selection[0]).unwrap();
        assert_eq!(
            (mover.a.position, mover.b.position),
            ([1.1, 0.5, -1.0], [1.1, 1.5, -1.0])
        );
        assert_eq!(
            (mover.a.rotation, mover.b.rotation),
            ([0.0, 90.0, 0.0], [0.0, 90.0, 0.0])
        );

        // A move on the floor, 1 m to +Z: the cube, and the two poses of the mover.
        let moved = drag(Tool::Move, Handle::Floor, &|gizmo| gizmo.origin + Vec3::Z);
        assert_eq!(moved.room.objects[2].pose.position, [1.0, 0.5, 1.0]);
        let mover = &moved.mover(&selection[0]).unwrap();
        assert_eq!(
            (mover.a.position, mover.b.position),
            ([2.0, 0.5, 1.0], [2.0, 1.5, 1.0])
        );
    }

    #[test]
    fn a_drag_of_a_scale_handle_scales_one_axis() {
        // The end of the handle goes to two times its distance.
        let to = |gizmo: &Gizmo, _: &View| gizmo.origin + (gizmo.handle(0)[1] - gizmo.origin) * 2.0;
        let scaled = drag(Tool::Scale, Handle::Axis(0), to, Some((0.25, 15.0)));
        assert_eq!(scaled.scale, [2.0, 1.0, 1.0]);
    }
}
