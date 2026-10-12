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
//!
//! A drag keeps the room of its start, and each frame of the drag makes the room
//! again from that start and the full motion: no error adds up, and a snap is a snap
//! of the result. The document changes in each frame; the history takes the drag as
//! one step.

use glam::{EulerRot, Quat, Vec3};
use maki_events::EventDef;
use maki_scene::RoomDef;

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
}

impl Gizmo {
    /// The gizmo of the last selected item. `None`: nothing is selected that has a
    /// place (the room, a doorway, a door). `local`: the axes of the item, not those
    /// of the room; a scale is always along the axes of the item.
    pub fn of(
        document: &Document,
        item: &Item,
        second: bool,
        local: bool,
        tool: Tool,
        view: &View,
    ) -> Option<Self> {
        let (origin, rotation) = document.pose_of(item, second)?;
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
        })
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

        match (tool, self.handle) {
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
                };
                // Tenths of a millimeter: no 2.4999998 in a room file.
                let moved = |from: f32, by: f32| ((from + by) * 1e4).round() / 1e4;
                let moved_point = |point: &mut [f32; 3]| {
                    *point = [0, 1, 2].map(|axis| moved(point[axis], offset[axis]));
                };
                for item in selection {
                    match document.place(item, second) {
                        Some(Place::Pose(pose)) => moved_point(&mut pose.position),
                        Some(Place::Point(point)) => moved_point(point),
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
                for item in selection {
                    if let Some(Place::Pose(pose)) = document.place(item, second) {
                        let (y, x, z) = (turn * pose.transform().rotation).to_euler(EulerRot::YXZ);
                        // Hundredths of a degree: no 29.999998 in a room file.
                        pose.rotation = [x, y, z]
                            .map(|angle| (angle.to_degrees() * 100.0).round() / 100.0 + 0.0);
                    }
                }
            }
            (Tool::Scale, Handle::Axis(axis)) => {
                let factor = ray.along(origin, axes[axis]) / self.grab.max(1e-3);
                for item in selection {
                    if let Some(Place::Pose(pose)) = document.place(item, second) {
                        let step = if steps.is_some() { SCALE_STEP } else { 0.0 };
                        pose.scale[axis] = snap(pose.scale[axis] * factor, step).max(0.01);
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

    /// A view from the +Z side and above, and a room with one cube at (1.1, 0.5, 0).
    fn scene() -> (View, Document, [Item; 1]) {
        let camera = Camera {
            position: Vec3::new(0.0, 9.0, 7.0),
            target: Vec3::new(0.0, 0.5, 0.0),
            fov_y_degrees: 45.0,
        };
        let mut document = Document::new_room(PathBuf::from("room.ron"));
        let mut cube = PropDef::default();
        cube.pose.position = [1.1, 0.5, 0.0];
        document.room.props.push(cube);
        (
            View::new(camera, [1280.0, 720.0]),
            document,
            [Item::Prop(0)],
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
            Handle::Floor => gizmo.origin,
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
        document.room.props[0].pose
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

    #[test]
    fn a_drag_of_a_scale_handle_scales_one_axis() {
        // The end of the handle goes to two times its distance.
        let to = |gizmo: &Gizmo, _: &View| gizmo.origin + (gizmo.handle(0)[1] - gizmo.origin) * 2.0;
        let scaled = drag(Tool::Scale, Handle::Axis(0), to, Some((0.25, 15.0)));
        assert_eq!(scaled.scale, [2.0, 1.0, 1.0]);
    }
}
