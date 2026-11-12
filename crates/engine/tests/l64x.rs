//! Stage 64, the fixes of the review: the skip (BACK) of the template "Cutscene trigger".

use std::time::Duration;

use bevy_ecs::query::With;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Commands, Query, ResMut};
use glam::Vec3;
use maki_engine::core::{
    App, ConfigFile, FixedFirst, Interpolated, PlayState, PoseJumps, Startup, Transform,
};
use maki_engine::platform::{Key, RawInput};
use maki_engine::scene::{Light, Player, RoomRequest, RoomTarget, Timeline, room_text};
use maki_engine::{events, gameplay, physics, platform, scene};

#[path = "../templates/cutscene_trigger.rs"]
mod cutscene_trigger;

/// The trigger at (0, 0, 0), with a timeline of 10 s on its own light: 1 to 11.
const TRIGGER: &str = r#"(id: 1, name: "trigger", components: {
    "script_name": (), "area": (width: 1.0, depth: 1.0),
    "light": (radius: 0.1, color: (1, 1, 1), intensity: 1.0),
    "timeline": (tracks: [(track: (channel: Field(component: "light", field: "intensity"),
        keys: [(time: 0.0, value: Number(1.0)), (time: 10.0, value: Number(11.0))]))])})"#;

/// Where the test puts the player in the next fixed step.
#[derive(Resource, Default)]
struct PlaceAt(Option<Vec3>);

fn steps(app: &mut App, count: usize) {
    for _ in 0..count {
        app.advance(Duration::from_millis(20));
    }
}

fn timeline(app: &App) -> (bool, f32) {
    let world = app.world();
    let mut query = world.try_query::<(&Timeline, &Light)>().unwrap();
    let (timeline, light) = query.single(world).unwrap();
    (timeline.playing, light.intensity)
}

fn play_state(app: &App) -> PlayState {
    *app.world().resource::<PlayState>()
}

#[test]
fn back_skips_the_cutscene_to_its_end() {
    let config = "[time]\nfixed_hz = 50.0\n[assets]\nbackground = false";
    let mut app = App::new(ConfigFile::from_text(config)).unwrap();
    app.add_module(platform::PlatformModule)
        .add_module(maki_engine::assets::AssetsModule)
        .add_module(scene::SceneModule)
        .add_module(physics::PhysicsModule)
        .add_module(scene::AnimationModule)
        .add_module(events::EventsModule)
        .add_module(gameplay::ControlsModule)
        .add_module(gameplay::GameModule)
        .init_resource::<PlaceAt>()
        .add_systems(
            FixedFirst,
            |mut place: ResMut<PlaceAt>,
             mut jumps: ResMut<PoseJumps>,
             mut players: Query<&mut Interpolated, With<Player>>| {
                if let Some(at) = place.0.take() {
                    for mut pose in &mut players {
                        pose.jump(Transform::from_translation(at), &mut jumps);
                    }
                }
            },
        );
    cutscene_trigger::add(&mut app);
    let room = room_text(8.0, 8.0, 3.0, &[TRIGGER]);
    app.add_systems(
        Startup,
        move |mut commands: Commands, mut request: ResMut<RoomRequest>| {
            // The capsule is what the sensor of the area sees. Out of the area at the start.
            let body = physics::Kinematic {
                shape: physics::Shape::Capsule {
                    radius: 0.3,
                    half_height: 0.5,
                },
                center: Vec3::Y * 0.8,
                sensor: false,
            };
            let start = Transform::from_translation(Vec3::new(3.0, 0.0, 3.0));
            commands.spawn((Player, Interpolated::at(start), body));
            request.0 = Some(RoomTarget {
                path: "l64x.ron".into(),
                spawn: None,
                text: Some(room.clone()),
            });
        },
    );
    app.run(|mut app| {
        steps(&mut app, 5);
        app.resource_mut::<PlaceAt>().0 = Some(Vec3::ZERO);
        steps(&mut app, 10);
        assert!(timeline(&app).0, "the enter plays the timeline");
        assert_eq!(play_state(&app), PlayState::Cutscene);
        app.write_message(RawInput::KeyDown(Key::X))
            .write_message(RawInput::KeyUp(Key::X));
        steps(&mut app, 5);
        assert_eq!(timeline(&app), (false, 11.0), "skipped to the end");
        assert_eq!(play_state(&app), PlayState::Play);
    });
}
