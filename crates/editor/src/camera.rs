//! The camera of the editor: it turns around a point, and the mouse moves it.
//!
//! The game camera stays as it is. The marker `ActiveCamera` tells which of the two
//! the picture shows: `Editor::apply_mode` gives it to this camera while the room is
//! edited, and to the camera of the game while the game is played.

use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::component::Component;
use bevy_ecs::query::Without;
use bevy_ecs::system::{Commands, Query, Res};
use glam::Vec3;
use maki_gameplay::{CameraSettings, FollowCamera};
use maki_platform::{Action, ActionState, Key, Pointer, WindowSettings};
use maki_scene::Camera;

use crate::Editor;

pub const ORBIT: Action = Action("editor_orbit");
pub const PAN: Action = Action("editor_pan");
pub const ZOOM_IN: Action = Action("editor_zoom_in");
pub const ZOOM_OUT: Action = Action("editor_zoom_out");
pub const VIEW_TOP: Action = Action("editor_view_top");
pub const VIEW_GAME: Action = Action("editor_view_game");

pub(crate) const BINDINGS: [(Key, Action, &str); 6] = [
    (Key::MouseRight, ORBIT, "Turn the view (drag)"),
    (Key::MouseMiddle, PAN, "Move the view (drag)"),
    (Key::WheelUp, ZOOM_IN, "Nearer"),
    (Key::WheelDown, ZOOM_OUT, "Farther"),
    (Key::T, VIEW_TOP, "View from the top"),
    (Key::G, VIEW_GAME, "View of the game camera"),
];

/// Radians of turn for one pixel of a drag.
const TURN_SPEED: f32 = 0.005;
/// One wheel step multiplies or divides the distance by this.
const ZOOM_STEP: f32 = 1.15;
/// Not straight down: the view then keeps its "up" (see `maki_render::ray_basis`).
const MAX_PITCH: f32 = 1.55;

/// The view of the editor: the camera looks at `target` from `distance` meters.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct EditorCamera {
    pub target: Vec3,
    /// The turn around the Y axis, in radians. 0: the camera is on the +Z side.
    pub yaw: f32,
    /// The angle above the floor, in radians.
    pub pitch: f32,
    pub distance: f32,
    pub fov_y_degrees: f32,
}

impl EditorCamera {
    /// The view that the game camera has at its start.
    fn game_view(settings: &CameraSettings) -> Self {
        let target = Vec3::from(settings.target);
        let offset = Vec3::from(settings.position) - target;
        let distance = offset.length().max(0.5);
        Self {
            target,
            yaw: offset.x.atan2(offset.z),
            pitch: (offset.y / distance).asin().clamp(0.0, MAX_PITCH),
            distance,
            fov_y_degrees: settings.fov_degrees,
        }
    }

    fn camera(&self) -> Camera {
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();
        let direction = Vec3::new(cos_pitch * sin_yaw, sin_pitch, cos_pitch * cos_yaw);
        Camera {
            position: self.target + direction * self.distance,
            target: self.target,
            fov_y_degrees: self.fov_y_degrees,
        }
    }
}

pub(crate) fn spawn(mut commands: Commands, settings: Option<Res<CameraSettings>>) {
    // A game without the camera module: the same view, from the default settings.
    let settings = settings.map_or_else(CameraSettings::default, |settings| settings.clone());
    let view = EditorCamera::game_view(&settings);
    commands.spawn((view.camera(), view));
}

/// Runs in `Update`. The actions are of the editor mode, so they are not pressed
/// while the game is played, and the pointer does not move while it is on a panel.
pub(crate) fn move_camera(
    actions: Res<ActionState>,
    pointer: Res<Pointer>,
    window: Res<WindowSettings>,
    settings: Option<Res<CameraSettings>>,
    editor: Res<Editor>,
    // `Without`: the two cameras are different entities, so this system and the
    // camera systems of the game need no order.
    mut cameras: Query<(&mut EditorCamera, &mut Camera), Without<FollowCamera>>,
) {
    let [dx, dy] = pointer.delta;
    for (mut view, mut camera) in &mut cameras {
        let mut new = *view;
        if actions.just_pressed(VIEW_GAME) {
            let settings = settings.as_deref().cloned().unwrap_or_default();
            new = EditorCamera::game_view(&settings);
        }
        if actions.just_pressed(VIEW_TOP) {
            let room = &editor.document.room;
            new.target = Vec3::ZERO;
            (new.yaw, new.pitch) = (0.0, MAX_PITCH);
            // The full room is in the picture, with the top of its walls.
            let half_fov = (new.fov_y_degrees.to_radians() / 2.0).tan();
            new.distance = room.width.max(room.depth) * 0.55 / half_fov + room.height;
        }
        if actions.pressed(ORBIT) {
            new.yaw -= dx * TURN_SPEED;
            new.pitch = (new.pitch + dy * TURN_SPEED).clamp(0.05, MAX_PITCH);
        }
        if actions.pressed(PAN) {
            // The point under the pointer stays under it: meters for one pixel at
            // the distance of the target.
            let scale = 2.0 * (new.fov_y_degrees.to_radians() / 2.0).tan() * new.distance
                / window.height.max(1) as f32;
            let now = new.camera();
            let forward = (now.target - now.position).normalize();
            let right = forward.cross(Vec3::Y).normalize();
            let up = right.cross(forward);
            new.target += (up * dy - right * dx) * scale;
        }
        let steps =
            f32::from(actions.just_pressed(ZOOM_OUT)) - f32::from(actions.just_pressed(ZOOM_IN));
        new.distance = (new.distance * ZOOM_STEP.powf(steps)).clamp(0.5, 200.0);
        // Only a different value is a write: a camera at rest keeps the picture.
        view.set_if_neq(new);
        camera.set_if_neq(new.camera());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_game_view_is_the_view_of_the_camera_settings() {
        let settings = CameraSettings::default();
        let camera = EditorCamera::game_view(&settings).camera();
        assert!(camera.position.distance(settings.position.into()) < 1e-4);
        assert_eq!(camera.target, Vec3::from(settings.target));
    }
}
