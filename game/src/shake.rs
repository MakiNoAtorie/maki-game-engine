//! An event command of this game: `ShakeCamera(seconds, strength)`. It shows how a
//! game adds a command: no engine crate knows it, and a room file uses it as it uses
//! the built-in commands.

use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Query, Res, ResMut};
use glam::Vec3;
use maki_core::{App, Module, Time, Update};
use maki_events::CommandRegistry;
use maki_gameplay::follow_player;
use maki_scene::Camera;
use serde::{Deserialize, Serialize};

/// The event commands of this game.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GameCommand {
    /// Shakes the camera for this many seconds. The second number is the largest
    /// offset, in meters. The event does not wait for the end of the shake.
    ShakeCamera(f32, f32),
}

/// The shake that runs. `left` at zero: none.
#[derive(Resource, Debug, Default)]
struct CameraShake {
    seconds: f32,
    left: f32,
    strength: f32,
}

/// Adds the commands of [`GameCommand`]. Needs `maki_events::EventsModule` and
/// `maki_gameplay::CameraModule`.
pub struct ShakeModule;

impl Module for ShakeModule {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraShake>()
            // After the follow: it puts the camera at its place of rest in each frame,
            // and this adds the offset of the frame.
            .add_systems(Update, shake_camera.after(follow_player));
        let registry = app.world().resource::<CommandRegistry>();
        registry.add(|command: &GameCommand, _step, world| {
            let GameCommand::ShakeCamera(seconds, strength) = *command;
            *world.resource_mut::<CameraShake>() = CameraShake {
                seconds,
                left: seconds,
                strength,
            };
            true
        });
    }
}

fn shake_camera(time: Res<Time>, mut shake: ResMut<CameraShake>, mut cameras: Query<&mut Camera>) {
    if shake.left <= 0.0 {
        return;
    }
    shake.left = (shake.left - time.delta.as_secs_f32()).max(0.0);
    // Two sine waves of different rates, weaker toward the end. No random number:
    // each run shows the same shake.
    let passed = shake.seconds - shake.left;
    let wave = Vec3::new((passed * 53.0).sin(), (passed * 71.0).sin(), 0.0);
    let offset = wave * shake.strength * (shake.left / shake.seconds);
    for mut camera in &mut cameras {
        // Only a different value is a write: a camera at rest keeps the picture.
        if offset != Vec3::ZERO {
            camera.position += offset;
            camera.target += offset;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use maki_core::{ConfigFile, Interpolated, Startup, Transform};
    use maki_events::EventsModule;
    use maki_platform::PlatformModule;
    use maki_scene::{Player, RoomRequest, RoomTarget, SceneModule};

    use super::*;

    #[test]
    fn an_event_of_a_room_file_shakes_the_camera_and_the_camera_comes_back() {
        let path = std::env::temp_dir().join(format!("maki-shake-{}.ron", std::process::id()));
        let room = r#"(width: 8, depth: 8, height: 3, spawns: {"start": (0, 0, 0)},
            events: [(name: "quake", pages: [(trigger: Autorun, commands: [
                ShakeCamera(0.1, 0.5), SetFlag("after", true)])])])"#;
        std::fs::write(&path, room).unwrap();

        let mut app = App::new(ConfigFile::from_text("[camera]\nfollow_time = 0.0")).unwrap();
        app.add_module(PlatformModule)
            .add_module(SceneModule)
            .add_module(EventsModule)
            .add_module(maki_gameplay::CameraModule)
            .add_module(ShakeModule);
        let target = RoomTarget {
            path: path.clone(),
            spawn: Some("start".to_owned()),
        };
        app.add_systems(
            Startup,
            move |mut commands: bevy_ecs::system::Commands, mut request: ResMut<RoomRequest>| {
                commands.spawn((Player, Interpolated::at(Transform::IDENTITY)));
                request.0 = Some(target.clone());
            },
        );
        app.check_config().unwrap();

        app.run(|mut app| {
            let mut frame = || {
                app.advance(Duration::from_secs_f32(1.0 / 60.0));
                let mut cameras = app.world().try_query::<&Camera>().unwrap();
                cameras.single(app.world()).unwrap().target
            };
            // The room loads, the event starts, and its command runs.
            let rest = frame();
            let shaken: Vec<Vec3> = (0..6).map(|_| frame()).collect();
            assert!(shaken.iter().any(|target| *target != rest), "{shaken:?}");
            // The offset is never larger than the strength, in X and in Y.
            assert!(
                shaken
                    .iter()
                    .all(|target| (*target - rest).length() <= 0.71)
            );
            // 0.1 s is 6 frames: the camera is back.
            for _ in 0..6 {
                frame();
            }
            assert_eq!(frame(), rest);
        });
        std::fs::remove_file(path).unwrap();
    }
}
