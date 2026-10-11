//! The light: an entity with a `Light` component, and the system that moves it.
//! Two modules, so a scene can have a light that the player does not control.

use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::system::{Commands, Query, Res};
use glam::Vec3;
use maki_core::{App, Config, FixedUpdate, Module, Startup, Time};
use maki_platform::{Action, FixedActionState};
use maki_render::{Camera, Light};

/// Light speed for the move actions, in meters per second.
const MOVE_SPEED: f32 = 2.0;
/// One step of the intensity actions multiplies or divides the intensity by this.
const INTENSITY_STEP: f32 = 1.1;
/// The intensity actions keep the intensity in this range.
const MIN_INTENSITY: f32 = 0.1;
const MAX_INTENSITY: f32 = 1000.0;

/// Spawns one light with the values of the config.
pub struct LightModule;

impl Module for LightModule {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_light);
    }
}

fn spawn_light(mut commands: Commands, config: Res<Config>) {
    commands.spawn(Light {
        position: config.light_position.into(),
        radius: config.light_radius,
        color: config.light_color.into(),
        intensity: config.light_intensity,
    });
}

/// Moves each light with the light actions.
pub struct LightControllerModule;

impl Module for LightControllerModule {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, control_lights);
    }
}

fn control_lights(
    actions: Res<FixedActionState>,
    time: Res<Time>,
    config: Res<Config>,
    cameras: Query<&Camera>,
    mut lights: Query<&mut Light>,
) {
    // 1.0, -1.0, or 0.0 from a pair of opposite actions.
    let axis = |positive, negative| {
        f32::from(actions.pressed(positive)) - f32::from(actions.pressed(negative))
    };
    // "Forward" is the view direction of the camera along the floor, so the light
    // moves up on the screen. A camera that looks straight down has no such
    // direction: -Z is up on the screen then (see `Camera::ray_basis`).
    let forward = cameras
        .iter()
        .next()
        .and_then(|camera| {
            (camera.target - camera.position)
                .with_y(0.0)
                .try_normalize()
        })
        .unwrap_or(Vec3::NEG_Z);
    let right = forward.cross(Vec3::Y);
    let direction = forward * axis(Action::MoveLightForward, Action::MoveLightBack)
        + right * axis(Action::MoveLightRight, Action::MoveLightLeft)
        + Vec3::Y * axis(Action::LightUp, Action::LightDown);
    let step = direction.normalize_or_zero() * MOVE_SPEED * time.fixed_delta.as_secs_f32();

    // A wheel step is a press of one frame: `just_pressed` shows it to one fixed step.
    let intensity_steps = f32::from(actions.just_pressed(Action::LightIntensityUp))
        - f32::from(actions.just_pressed(Action::LightIntensityDown));

    for mut light in &mut lights {
        let mut next = *light;
        next.position = clamp_to_room(light.position + step, light.radius, &config);
        if intensity_steps != 0.0 {
            next.intensity = (light.intensity * INTENSITY_STEP.powf(intensity_steps))
                .clamp(MIN_INTENSITY, MAX_INTENSITY);
        }
        // Writes only a different value. Each write marks the light as changed, and
        // a changed light starts the accumulation of the renderer again.
        light.set_if_neq(next);
    }
}

/// The nearest position where the full sphere of the light is in the room.
pub(crate) fn clamp_to_room(position: Vec3, radius: f32, config: &Config) -> Vec3 {
    // The floor has its center at the origin and is at y = 0.
    let half = Vec3::new(config.room_width / 2.0, 0.0, config.room_depth / 2.0);
    let low = -half + radius;
    let high = half.with_y(config.room_height) - radius;
    // `max` then `min`, not `clamp`: `clamp` panics if the room is smaller than the light.
    position.max(low).min(high)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_light_stays_in_the_room() {
        // 8 x 8 x 3 meters.
        let config = Config::default();
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
