//! The camera: one `Camera` entity that follows the player from a fixed direction.

use bevy_ecs::change_detection::{DetectChanges, DetectChangesMut};
use bevy_ecs::component::Component;
use bevy_ecs::query::With;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Commands, Local, Query, Res};
use glam::Vec3;
use maki_core::{
    App, Fields, Interpolated, Module, Settings, Startup, Time, Transform, Update,
    interpolate_transforms,
};
use maki_scene::{
    ActiveCamera, Camera, CurrentRoom, ObjectId, Player, RoomDef, add_object_component,
};
use serde::{Deserialize, Serialize};

/// The target is moved onto the goal when it is nearer than this, in meters. Without
/// it the camera would move by smaller and smaller steps forever, and each step
/// starts the picture again.
const ARRIVE_DISTANCE: f32 = 0.002;

/// The `[camera]` table of the config file.
#[derive(Resource, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CameraSettings {
    /// The view of the camera: where it is when it looks at `target`, in meters: X, Y
    /// (up), Z. The camera keeps this direction and distance when it follows the
    /// player.
    pub position: [f32; 3],
    /// The point that the camera looks at before it follows the player. Its height
    /// stays the height of the point that the camera looks at.
    pub target: [f32; 3],
    /// Vertical field of view, in degrees.
    pub fov_degrees: f32,
    /// The target moves most of the way to the player in this time, in seconds.
    /// 0: no delay.
    pub follow_time: f32,
    /// The target stays this far from the walls of the current room, in meters, so
    /// the view shows less of the outside of the room.
    pub margin: f32,
}

impl Default for CameraSettings {
    fn default() -> Self {
        Self {
            // Above and in front of the target, looking down at an angle.
            position: [0.0, 9.0, 7.0],
            target: [0.0, 0.5, 0.0],
            fov_degrees: 45.0,
            follow_time: 0.25,
            margin: 3.0,
        }
    }
}

impl Settings for CameraSettings {
    const TABLE: &'static str = "camera";

    fn check(&self) -> Result<(), String> {
        if !(self.follow_time.is_finite() && self.follow_time >= 0.0) {
            return Err("follow_time must be zero or a positive number".to_owned());
        }
        Ok(())
    }
}

/// Marks the camera that follows the player. A system that moves that camera for a
/// frame (a shake) finds it with this, also while another camera is the active one.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct FollowCamera;

/// Spawns the camera of the settings and moves it with the entity that has the
/// `Player` component.
pub struct CameraModule;

impl Module for CameraModule {
    fn build(&self, app: &mut App) {
        add_object_component::<CameraDef>(app, "camera");
        app.add_settings::<CameraSettings>()
            .add_systems(Startup, spawn_camera)
            // After the interpolation: the camera follows the pose that the frame shows.
            .add_systems(Update, follow_player.after(interpolate_transforms));
    }
}

/// The component `camera` of an object: while it is `active`, the view of the game is
/// the pose of the object (a timeline moves it by a pose track and switches it by a
/// switch track on `active`). It is the `FollowCamera` that takes the view; the camera
/// of the editor is another entity, so editing never shows it.
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
#[fields(default)]
pub struct CameraDef {
    /// Vertical field of view, in degrees.
    #[field(range = 5.0..=120.0, step = 1.0)]
    pub fov: f32,
    /// The object is the view of the game. Of several, the lowest object id.
    pub active: bool,
}

impl Default for CameraDef {
    fn default() -> Self {
        Self {
            fov: CameraSettings::default().fov_degrees,
            active: true,
        }
    }
}

/// The camera for a target point: the view direction and the distance of the settings.
fn camera_at(target: Vec3, settings: &CameraSettings) -> Camera {
    let offset = Vec3::from(settings.position) - Vec3::from(settings.target);
    Camera {
        position: target + offset,
        target,
        fov_y_degrees: settings.fov_degrees,
    }
}

/// The point that the camera looks at when the player is at `player`: the player, at
/// the height of the settings target, and not nearer to a wall than `margin`.
fn follow_goal(player: Vec3, settings: &CameraSettings, room: &RoomDef) -> Vec3 {
    // A margin that is larger than half of the room keeps the target in the center.
    let limit = |size: f32| (size / 2.0 - settings.margin).max(0.0);
    let (x, z) = (limit(room.width), limit(room.depth));
    Vec3::new(
        player.x.clamp(-x, x),
        settings.target[1],
        player.z.clamp(-z, z),
    )
}

fn spawn_camera(mut commands: Commands, settings: Res<CameraSettings>) {
    // `follow_player` puts it at the player when the first room is loaded.
    let camera = camera_at(settings.target.into(), &settings);
    commands.spawn((camera, FollowCamera, ActiveCamera));
}

/// Runs in `Update`. A system that moves the camera away from this place for a frame
/// (a shake) runs `.after(follow_player)`.
pub fn follow_player(
    time: Res<Time>,
    settings: Res<CameraSettings>,
    room: Res<CurrentRoom>,
    players: Query<&Transform, With<Player>>,
    mut cameras: Query<&mut Camera, With<FollowCamera>>,
    // .0: the look-at point, not read back from the `Camera`; .1: a clash was logged.
    mut target: Local<(Option<Vec3>, bool)>,
    objects: Query<(&ObjectId, &CameraDef, &Interpolated)>,
) {
    // In this system, so a shake that runs `.after(follow_player)` adds to either view.
    if let Some(view) = object_view(&time, &objects, &mut target.1) {
        for mut camera in &mut cameras {
            camera.set_if_neq(view);
        }
        return;
    }
    let Some(player) = players.iter().next() else {
        return;
    };
    let goal = follow_goal(player.translation, &settings, &room.def);
    // The part of the way to the goal for this frame. With this form the motion is the
    // same at each frame rate. In another room the camera is there at once.
    let part = if settings.follow_time > 0.0 && !room.is_changed() {
        1.0 - (-time.delta.as_secs_f32() / settings.follow_time).exp()
    } else {
        1.0
    };
    let target = target.0.get_or_insert(settings.target.into());
    *target = target.lerp(goal, part);
    if target.distance(goal) < ARRIVE_DISTANCE {
        *target = goal;
    }
    for mut camera in &mut cameras {
        // Only a different value is a write: a camera at rest keeps the picture.
        camera.set_if_neq(camera_at(*target, &settings));
    }
}

/// The view of the active `camera` object, or none. Its front is +Z of the object, as
/// the front of a walker, so a turn of the object turns the view.
fn object_view(
    time: &Time,
    objects: &Query<(&ObjectId, &CameraDef, &Interpolated)>,
    warned: &mut bool,
) -> Option<Camera> {
    let active = || objects.iter().filter(|(_, def, _)| def.active);
    // The lowest id wins; the warning is once per time of the clash.
    let clash = active().nth(1).is_some();
    if clash && !*warned {
        tracing::warn!("several active cameras: the object with the lowest id is the view");
    }
    *warned = clash;
    let (_, def, pose) = active().min_by_key(|(id, ..)| **id)?;
    // An object has the `Interpolated` pose only: the blend is the pose of this frame.
    let pose = pose.previous.blend(&pose.current, time.alpha);
    Some(Camera {
        position: pose.translation,
        target: pose.translation + pose.rotation * Vec3::Z,
        fov_y_degrees: def.fov,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_camera_target_follows_the_player_and_stays_away_from_the_walls() {
        // A room of 8 x 8 m and a margin of 2.5 m: the target stays within 1.5 m.
        let settings = CameraSettings {
            margin: 2.5,
            ..CameraSettings::default()
        };
        let room = RoomDef {
            width: 8.0,
            depth: 8.0,
            height: 3.0,
            ..RoomDef::default()
        };
        let height = settings.target[1];
        assert_eq!(
            follow_goal(Vec3::new(1.0, 0.0, -0.5), &settings, &room),
            Vec3::new(1.0, height, -0.5)
        );
        assert_eq!(
            follow_goal(Vec3::new(3.9, 0.0, -9.0), &settings, &room),
            Vec3::new(1.5, height, -1.5)
        );
        // A margin that is larger than the room: no panic, the center.
        let wide = CameraSettings {
            margin: 50.0,
            ..CameraSettings::default()
        };
        assert_eq!(
            follow_goal(Vec3::new(3.0, 0.0, 3.0), &wide, &room),
            Vec3::new(0.0, height, 0.0)
        );
        // The view direction is that of the settings.
        let camera = camera_at(Vec3::X, &settings);
        assert_eq!(
            camera.position - camera.target,
            Vec3::from(settings.position) - Vec3::from(settings.target)
        );
    }
}
