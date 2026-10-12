//! The debug control of the light: the light actions move the first light of the room.

use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Query, Res, ResMut};
use glam::Vec3;
use maki_core::{App, FixedUpdate, Interpolated, Module, Time, Update};
use maki_events::run_events;
use maki_platform::{Action, ActionState, FixedActionState, Key, add_bindings};
use maki_scene::{Camera, CurrentRoom, Light, RoomDef};

use crate::controls::{MoveCapture, move_direction};

/// The move actions go to the light, or back to the player.
pub const TOGGLE_LIGHT_CONTROL: Action = Action("toggle_light_control");
pub const LIGHT_UP: Action = Action("light_up");
pub const LIGHT_DOWN: Action = Action("light_down");
pub const LIGHT_INTENSITY_UP: Action = Action("light_intensity_up");
pub const LIGHT_INTENSITY_DOWN: Action = Action("light_intensity_down");

/// Light speed for the move actions, in meters per second.
const MOVE_SPEED: f32 = 2.0;
/// One step of the intensity actions multiplies or divides the intensity by this.
const INTENSITY_STEP: f32 = 1.1;
/// The intensity actions keep the intensity in this range.
const MIN_INTENSITY: f32 = 0.1;
const MAX_INTENSITY: f32 = 1000.0;

/// Debug: moves the first light of the room file with the light actions, and with the
/// move actions while the light has them (`MoveCapture`). The lights of a model file
/// stay where they are. Needs `ControlsModule`.
pub struct LightControlModule;

impl Module for LightControlModule {
    fn build(&self, app: &mut App) {
        add_bindings(
            app,
            &[
                (Key::L, TOGGLE_LIGHT_CONTROL, "Move the light / the player"),
                (Key::Q, LIGHT_DOWN, "Move light down"),
                (Key::E, LIGHT_UP, "Move light up"),
                (Key::WheelUp, LIGHT_INTENSITY_UP, "Increase light intensity"),
                (
                    Key::WheelDown,
                    LIGHT_INTENSITY_DOWN,
                    "Decrease light intensity",
                ),
            ],
        );
        app.init_resource::<MoveCapture>()
            .add_systems(Update, toggle_move_target)
            // After the events: the two write the same lights.
            .add_systems(FixedUpdate, control_lights.after(run_events));
    }
}

fn toggle_move_target(actions: Res<ActionState>, mut capture: ResMut<MoveCapture>) {
    if actions.just_pressed(TOGGLE_LIGHT_CONTROL) {
        capture.0 = !capture.0;
        tracing::info!(light = capture.0, "move actions");
    }
}

fn control_lights(
    actions: Res<FixedActionState>,
    capture: Res<MoveCapture>,
    time: Res<Time>,
    room: Res<CurrentRoom>,
    cameras: Query<&Camera>,
    // Only a light of the room file has a pose for fixed steps.
    mut lights: Query<(&mut Interpolated, &mut Light)>,
) {
    let on_floor = if capture.0 {
        move_direction(&actions, cameras.iter().next())
    } else {
        Vec3::ZERO
    };
    let up = f32::from(actions.pressed(LIGHT_UP)) - f32::from(actions.pressed(LIGHT_DOWN));
    let direction = (on_floor + Vec3::Y * up).normalize_or_zero();
    let step = direction * MOVE_SPEED * time.fixed_delta.as_secs_f32();

    // A wheel step is a press of one frame: `just_pressed` shows it to one fixed step.
    let intensity_steps = f32::from(actions.just_pressed(LIGHT_INTENSITY_UP))
        - f32::from(actions.just_pressed(LIGHT_INTENSITY_DOWN));

    if let Some((mut pose, mut light)) = lights.iter_mut().next() {
        // Writes only a different value. Each write marks the component as changed,
        // and a changed light is a change of the lighting for the renderer.
        let position = clamp_to_room(pose.current.translation + step, light.radius, &room.def);
        if pose.current.translation != position {
            pose.current.translation = position;
        }
        if intensity_steps != 0.0 {
            light.intensity = (light.intensity * INTENSITY_STEP.powf(intensity_steps))
                .clamp(MIN_INTENSITY, MAX_INTENSITY);
        }
    }
}

/// The nearest position where the full sphere of a light is in the room.
pub fn clamp_to_room(position: Vec3, radius: f32, room: &RoomDef) -> Vec3 {
    // The floor has its center at the origin and is at y = 0.
    let half = Vec3::new(room.width / 2.0, 0.0, room.depth / 2.0);
    let low = -half + radius;
    let high = half.with_y(room.height) - radius;
    // `max` then `min`, not `clamp`: `clamp` panics if the room is smaller than the light.
    position.max(low).min(high)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_light_stays_in_the_room() {
        let config = RoomDef {
            width: 8.0,
            depth: 8.0,
            height: 3.0,
            ..RoomDef::default()
        };
        let inside = Vec3::new(1.0, 2.0, -3.0);
        assert_eq!(clamp_to_room(inside, 0.5, &config), inside);
        assert_eq!(
            clamp_to_room(Vec3::new(9.0, -1.0, -9.0), 0.5, &config),
            Vec3::new(3.5, 0.5, -3.5)
        );
        assert_eq!(clamp_to_room(Vec3::Y * 9.0, 0.5, &config).y, 2.5);
        // A light that is larger than the room: no panic.
        assert!(clamp_to_room(inside, 10.0, &config).is_finite());
    }
}
