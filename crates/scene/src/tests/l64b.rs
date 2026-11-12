//! The timeline (`maki_scene::timeline`): the file form, tracks on objects by id, the
//! start pose, the counted jump of a step, a field track, the end, `ResetPlay`, and the
//! room check.

use std::path::PathBuf;
use std::time::Duration;

use bevy_ecs::entity::Entity;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Query, ResMut};
use glam::Vec3;
use maki_assets::clip::{Channel, Key, KeyMode, KeyValue, Track};
use maki_core::{App, ConfigFile, FixedUpdate, Interpolated, PoseJumps};

use crate::{
    AnimationModule, ComponentText, Light, ObjectId, RoomCheck, RoomRequest, RoomTarget,
    RoomValidators, SceneModule, Timeline, TimelinePlayback, TimelineTrack, animate, move_movers,
    parse_room, room_text,
};

fn key(time: f32, value: KeyValue) -> Key {
    Key { time, value }
}

/// A step position track on `object`: at rest, then 1 m up from 0.1 s.
fn rise(object: u32) -> TimelineTrack {
    let up = |time, y| key(time, KeyValue::Vector([0.0, y, 0.0]));
    TimelineTrack {
        object: ObjectId(object),
        track: Track {
            channel: Channel::Position,
            target: None,
            mode: Some(KeyMode::Step),
            keys: vec![up(0.0, 0.0), up(0.1, 1.0)],
        },
    }
}

/// A linear track of the light intensity of the object of the timeline: 1 to 3 in 0.2 s.
fn brighten() -> TimelineTrack {
    TimelineTrack {
        object: ObjectId(0),
        track: Track {
            channel: Channel::Field {
                component: "light".to_owned(),
                field: "intensity".to_owned(),
            },
            target: None,
            mode: None,
            keys: vec![
                key(0.0, KeyValue::Number(1.0)),
                key(0.2, KeyValue::Number(3.0)),
            ],
        },
    }
}

fn timeline(tracks: Vec<TimelineTrack>) -> Timeline {
    Timeline {
        playing: true,
        tracks,
        ..Timeline::default()
    }
}

/// A lamp object at (x, 2, 0), with the timeline if one is given.
fn lamp(id: u32, x: f32, timeline: Option<&Timeline>) -> String {
    let timeline = timeline
        .map(|t| format!(r#", "timeline": {}"#, ComponentText::of(t).as_str()))
        .unwrap_or_default();
    format!(
        r#"(id: {id}, pose: (position: ({x:?}, 2.0, 0.0)), components: {{"light": (radius: 0.1, color: (1.0, 1.0, 1.0), intensity: 1.0){timeline}}})"#
    )
}

/// For each fixed step after `animate`: the intensity of lamp 1, the poses of lamps 3
/// and 4, and if the timeline plays.
#[derive(Resource, Default)]
struct Record(Vec<(f32, Vec3, Vec3, bool)>);

fn path(test: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("maki-scene-l64b-{test}-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    directory.join("a.ron")
}

/// A game at 50 Hz in a room of lamps 1 (with `timeline`), 3, and 4.
fn game(test: &str, timeline: &Timeline) -> App {
    let objects = [
        lamp(1, 0.0, Some(timeline)),
        lamp(3, 1.0, None),
        lamp(4, 2.0, None),
    ];
    let objects: Vec<&str> = objects.iter().map(String::as_str).collect();
    let path = path(test);
    std::fs::write(&path, room_text(6.0, 6.0, 3.0, &objects)).unwrap();
    let config = "[time]\nfixed_hz = 50.0\n[assets]\nbackground = false";
    let mut app = App::new(ConfigFile::from_text(config)).unwrap();
    app.add_module(maki_assets::AssetsModule)
        .add_module(SceneModule)
        .add_module(AnimationModule)
        .init_resource::<Record>()
        .add_systems(
            FixedUpdate,
            (|objects: Query<(&ObjectId, &Light, &Interpolated)>,
              timelines: Query<&Timeline>,
              mut record: ResMut<Record>| {
                let mut row = (0.0, Vec3::ZERO, Vec3::ZERO, false);
                for (id, light, pose) in &objects {
                    match id.0 {
                        1 => row.0 = light.intensity,
                        3 => row.1 = pose.current.translation,
                        4 => row.2 = pose.current.translation,
                        _ => {}
                    }
                }
                row.3 = timelines.iter().any(|timeline| timeline.playing);
                if timelines.single().is_ok() {
                    record.0.push(row);
                }
            })
            .after(animate)
            .before(move_movers),
        );
    app.resource_mut::<RoomRequest>().0 = Some(RoomTarget {
        path,
        spawn: None,
        text: None,
    });
    app
}

fn steps(app: &mut App, count: u32) {
    for _ in 0..count {
        app.advance(Duration::from_millis(20));
    }
}

fn rows(app: &App) -> Vec<(f32, Vec3, Vec3, bool)> {
    app.world().resource::<Record>().0.clone()
}

fn entity_of(app: &App, object: u32) -> Entity {
    let mut objects = app
        .world()
        .try_query::<(Entity, &ObjectId, &Light)>()
        .unwrap();
    let found = objects.iter(app.world()).find(|(_, id, _)| id.0 == object);
    found.expect("the object entity").0
}

#[test]
fn a_timeline_reads_and_writes_with_no_default() {
    assert_eq!(ComponentText::of(&Timeline::default()).as_str(), "()");
    let mut written = timeline(vec![rise(3), brighten()]);
    written.fps = Some(12.0);
    let text = ComponentText::of(&written);
    let read: Timeline = text.read().unwrap();
    assert_eq!(read, written);
    let text = text.as_str();
    assert!(
        !text.contains("looping") && !text.contains("length"),
        "{text}"
    );
    // Object 0, the own object, is not written.
    assert_eq!(text.matches("object:").count(), 1, "{text}");
}

#[test]
fn tracks_on_two_objects_move_each_from_its_start_pose() {
    // Object 9 is not in the room: its track warns, and the others play.
    let tracks = vec![rise(3), rise(9), rise(4)];
    game("two", &timeline(tracks)).run(|mut app| {
        steps(&mut app, 20);
        let rows = rows(&app);
        let (first, last) = (rows[0], rows[rows.len() - 1]);
        assert_eq!(first.1, Vec3::new(1.0, 2.0, 0.0));
        assert_eq!(first.2, Vec3::new(2.0, 2.0, 0.0));
        assert_eq!(last.1, Vec3::new(1.0, 3.0, 0.0));
        assert_eq!(last.2, Vec3::new(2.0, 3.0, 0.0));
    });
}

#[test]
fn a_step_key_is_one_counted_jump_at_its_time() {
    game("step", &timeline(vec![rise(3)])).run(|mut app| {
        steps(&mut app, 20);
        let rows = rows(&app);
        // 0.1 s at 50 Hz: the sixth step.
        let up: Vec<bool> = rows.iter().map(|row| row.1.y > 2.5).collect();
        assert_eq!(up.iter().position(|up| *up), Some(5), "{up:?}");
        assert!(up[5..].iter().all(|up| *up));
        assert_eq!(app.world().resource::<PoseJumps>().0, 1);
    });
}

#[test]
fn a_field_track_keys_the_light_of_its_own_object() {
    game("field", &timeline(vec![brighten()])).run(|mut app| {
        steps(&mut app, 20);
        let intensity: Vec<f32> = rows(&app).iter().map(|row| row.0).collect();
        assert_eq!(intensity[0], 1.0);
        assert!((intensity[5] - 2.0).abs() < 1e-5, "{intensity:?}");
        assert_eq!(*intensity.last().unwrap(), 3.0);
    });
}

#[test]
fn the_end_sets_playing_false() {
    game("end", &timeline(vec![brighten()])).run(|mut app| {
        steps(&mut app, 20);
        let playing: Vec<bool> = rows(&app).iter().map(|row| row.3).collect();
        // 0.2 s: the eleventh step writes the last key, then the timeline stops.
        assert!(playing[..10].iter().all(|playing| *playing), "{playing:?}");
        assert!(playing[10..].iter().all(|playing| !*playing), "{playing:?}");
        assert_eq!(rows(&app).last().unwrap().0, 3.0);
    });
}

#[test]
fn reset_play_gives_back_the_start_poses() {
    game("reset", &timeline(vec![rise(3)])).run(|mut app| {
        steps(&mut app, 20);
        let lamp = entity_of(&app, 3);
        let pose = |app: &App| {
            app.world()
                .get::<Interpolated>(lamp)
                .unwrap()
                .current
                .translation
        };
        assert_eq!(pose(&app), Vec3::new(1.0, 3.0, 0.0));
        app.reset_play();
        assert_eq!(pose(&app), Vec3::new(1.0, 2.0, 0.0));
        let owner = entity_of(&app, 1);
        let playback = app.world().get::<TimelinePlayback>(owner).unwrap();
        assert_eq!(
            format!("{playback:?}"),
            format!("{:?}", TimelinePlayback::default())
        );
    });
}

#[test]
fn the_room_check_warns_of_a_track_on_an_object_not_in_the_room() {
    let objects = [
        lamp(1, 0.0, Some(&timeline(vec![rise(3), rise(9)]))),
        lamp(3, 1.0, None),
    ];
    let objects: Vec<&str> = objects.iter().map(String::as_str).collect();
    let text = room_text(6.0, 6.0, 3.0, &objects);
    let room = parse_room(&text).unwrap();
    let path = path("check");
    game("check", &Timeline::default()).run(|app| {
        let check = RoomCheck {
            path: &path,
            room: &room,
            text: &text,
        };
        let validators = app.world().resource::<RoomValidators>();
        let problems: Vec<_> = validators
            .0
            .iter()
            .flat_map(|validate| validate(&check))
            .collect();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].warning);
        assert!(problems[0].text.contains("id 9"), "{}", problems[0].text);
    });
}
