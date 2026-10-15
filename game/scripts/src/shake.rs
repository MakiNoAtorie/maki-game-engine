//! An event command of this game: `ShakeCamera(seconds, strength)`. It shows how a
//! game adds a command: no engine crate knows it, and a room file uses it as it uses
//! the built-in commands.

use bevy_ecs::query::With;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Query, Res, ResMut};
use glam::Vec3;
use maki_engine::core::{App, Module, ResetPlay, Time, Update};
use maki_engine::events::{CommandRegistry, Finding};
use maki_engine::gameplay::{FollowCamera, follow_player};
use maki_engine::scene::Camera;
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

/// Adds the commands of [`GameCommand`]. Needs `maki_engine::events::EventsModule` and
/// `maki_engine::gameplay::CameraModule`.
pub struct ShakeModule;

impl Module for ShakeModule {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraShake>()
            // After the follow: it puts the camera at its place of rest in each frame,
            // and this adds the offset of the frame.
            .add_systems(Update, shake_camera.after(follow_player))
            // The play state of this module: a shake does not go on in the next game.
            .add_systems(ResetPlay, |mut shake: ResMut<CameraShake>| {
                if shake.left > 0.0 {
                    *shake = CameraShake::default();
                }
            });
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
        // The row and the help of the command in the event panel of the editor.
        registry.add_texts(
            |command: &GameCommand| {
                let GameCommand::ShakeCamera(seconds, strength) = *command;
                Some(format!("Shake the camera: {seconds} s, {strength} m"))
            },
            |_| Some("Shakes the camera for some seconds. The event does not wait for the end."),
        );
        // What the "Add" list of the editor makes.
        registry.add_defaults(|_, _| Some(GameCommand::ShakeCamera(0.5, 0.05)));
        // The proof that a module checks its own commands: the room loader runs
        // this for each room file, and the editor shows it on the row.
        registry.add_check(|command: &GameCommand, _room| {
            let GameCommand::ShakeCamera(seconds, strength) = *command;
            let nothing = seconds <= 0.0 || strength <= 0.0;
            nothing.then(|| Finding::warning("the shake does nothing: a number is 0".to_owned()))
        });
    }
}

fn shake_camera(
    time: Res<Time>,
    mut shake: ResMut<CameraShake>,
    mut cameras: Query<&mut Camera, With<FollowCamera>>,
) {
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

    use maki_engine::core::{ConfigFile, Interpolated, Startup, Transform};
    use maki_engine::events::EventsModule;
    use maki_engine::platform::PlatformModule;
    use maki_engine::scene::{Player, RoomRequest, RoomTarget, SceneModule};

    use super::*;

    #[test]
    fn an_event_of_a_room_file_shakes_the_camera_and_the_camera_comes_back() {
        let path = std::env::temp_dir().join(format!("maki-shake-{}.ron", std::process::id()));
        let room = r#"(width: 8, depth: 8, height: 3, objects: [(name: "start", components: {"spawn": ()})],
            events: [(name: "quake", pages: [(trigger: Autorun, commands: [
                ShakeCamera(0.1, 0.5), SetFlag("after", true)])])])"#;
        std::fs::write(&path, room).unwrap();

        let mut app = App::new(ConfigFile::from_text("[camera]\nfollow_time = 0.0")).unwrap();
        app.add_module(PlatformModule)
            .add_module(SceneModule)
            .add_module(EventsModule)
            .add_module(maki_engine::gameplay::CameraModule)
            .add_module(ShakeModule);
        let target = RoomTarget {
            path: path.clone(),
            spawn: Some("start".to_owned()),
            text: None,
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
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_shake_that_runs_does_not_go_on_after_the_play_signal() {
        let mut app = App::new(ConfigFile::default()).unwrap();
        app.add_module(PlatformModule)
            .add_module(SceneModule)
            .add_module(EventsModule)
            .add_module(maki_engine::gameplay::CameraModule)
            .add_module(ShakeModule);
        app.run(|mut app| {
            let left = |app: &App| app.world().resource::<CameraShake>().left;
            *app.resource_mut::<CameraShake>() = CameraShake {
                seconds: 5.0,
                left: 5.0,
                strength: 0.5,
            };
            app.advance(Duration::from_secs_f32(1.0 / 60.0));
            assert!(left(&app) > 4.0);
            // What a tool does on stop and on play, and the game at its start.
            app.reset_play();
            assert_eq!(left(&app), 0.0);
        });
    }
}
