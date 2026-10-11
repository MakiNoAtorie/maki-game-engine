//! The camera: one `Camera` entity that follows the player.

use bevy_ecs::change_detection::{DetectChanges, DetectChangesMut};
use bevy_ecs::query::With;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Commands, Query, Res};
use glam::Vec3;
use maki_core::{App, Config, Module, Startup, Time, Transform, Update, interpolate_transforms};
use maki_scene::{Camera, CurrentRoom, Player, RoomDef};

/// The target is moved onto the goal when it is nearer than this, in meters. Without
/// it the camera would move by smaller and smaller steps forever, and each step
/// starts the picture again.
const ARRIVE_DISTANCE: f32 = 0.002;

/// Spawns the camera of the config and moves it with the player.
pub struct CameraModule;

impl Module for CameraModule {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_camera)
            // After the interpolation: the camera follows the pose that the frame shows.
            .add_systems(Update, follow_player.after(interpolate_transforms));
    }
}

/// The camera for a target point: the view direction and the distance of the config.
fn camera_at(target: Vec3, config: &Config) -> Camera {
    let offset = Vec3::from(config.camera_position) - Vec3::from(config.camera_target);
    Camera {
        position: target + offset,
        target,
        fov_y_degrees: config.camera_fov_degrees,
    }
}

/// The point that the camera looks at when the player is at `player`: the player, at
/// the height of the config target, and not nearer to a wall than `camera_margin`.
pub(crate) fn follow_goal(player: Vec3, config: &Config, room: &RoomDef) -> Vec3 {
    // A margin that is larger than half of the room keeps the target in the center.
    let limit = |size: f32| (size / 2.0 - config.camera_margin).max(0.0);
    let (x, z) = (limit(room.width), limit(room.depth));
    Vec3::new(
        player.x.clamp(-x, x),
        config.camera_target[1],
        player.z.clamp(-z, z),
    )
}

fn spawn_camera(mut commands: Commands, config: Res<Config>) {
    // `follow_player` puts it at the player when the first room is loaded.
    commands.spawn(camera_at(config.camera_target.into(), &config));
}

fn follow_player(
    time: Res<Time>,
    config: Res<Config>,
    room: Res<CurrentRoom>,
    players: Query<&Transform, With<Player>>,
    mut cameras: Query<&mut Camera>,
) {
    let Some(player) = players.iter().next() else {
        return;
    };
    let goal = follow_goal(player.translation, &config, &room.def);
    // The part of the way to the goal for this frame. With this form the motion is the
    // same at each frame rate. In another room the camera is there at once.
    let part = if config.camera_follow_time > 0.0 && !room.is_changed() {
        1.0 - (-time.delta.as_secs_f32() / config.camera_follow_time).exp()
    } else {
        1.0
    };
    for mut camera in &mut cameras {
        let mut target = camera.target.lerp(goal, part);
        if target.distance(goal) < ARRIVE_DISTANCE {
            target = goal;
        }
        // Only a different value is a write: a camera at rest keeps the picture.
        camera.set_if_neq(camera_at(target, &config));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_camera_target_follows_the_player_and_stays_away_from_the_walls() {
        // A room of 8 x 8 m and a margin of 2.5 m: the target stays within 1.5 m.
        let config = Config {
            camera_margin: 2.5,
            ..Config::default()
        };
        let room = RoomDef {
            width: 8.0,
            depth: 8.0,
            height: 3.0,
            ..RoomDef::default()
        };
        let height = config.camera_target[1];
        assert_eq!(
            follow_goal(Vec3::new(1.0, 0.0, -0.5), &config, &room),
            Vec3::new(1.0, height, -0.5)
        );
        assert_eq!(
            follow_goal(Vec3::new(3.9, 0.0, -9.0), &config, &room),
            Vec3::new(1.5, height, -1.5)
        );
        // A margin that is larger than the room: no panic, the center.
        let wide = Config {
            camera_margin: 50.0,
            ..Config::default()
        };
        assert_eq!(
            follow_goal(Vec3::new(3.0, 0.0, 3.0), &wide, &room),
            Vec3::new(0.0, height, 0.0)
        );
        // The view direction is that of the config.
        let camera = camera_at(Vec3::X, &config);
        assert_eq!(
            camera.position - camera.target,
            Vec3::from(config.camera_position) - Vec3::from(config.camera_target)
        );
    }
}
