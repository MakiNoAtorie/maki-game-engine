//! The animator (`maki_scene::animation`): step and linear clips, the frame rate, field
//! tracks, the cut by `SetField`, the rest pose, and `ResetPlay`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bevy_ecs::change_detection::DetectChanges;
use bevy_ecs::entity::Entity;
use bevy_ecs::world::Ref;
use glam::Vec3;
use maki_assets::clip::{Channel, Clip, KeyMode, KeyValue, Track};
use maki_core::{PoseJumps, Update};
use maki_scene::AnimationModule;

use super::*;

fn track(channel: Channel, keys: &[(f32, KeyValue)]) -> Track {
    let keys = keys.iter().cloned();
    Track {
        channel,
        mode: None,
        target: None,
        keys: keys
            .map(|(time, value)| maki_assets::clip::Key { time, value })
            .collect(),
    }
}

fn vector(x: f32, y: f32, z: f32) -> KeyValue {
    KeyValue::Vector([x, y, z])
}

fn field(component: &str, name: &str) -> Channel {
    Channel::Field {
        component: component.to_owned(),
        field: name.to_owned(),
    }
}

/// A game in a room with the objects `objects` (room text, each with a comma) and the
/// events `events`, and the clips as files `data/<name>.clip.ron`.
fn animated(test: &str, clips: &[(&str, Clip)], objects: &str, events: &str) -> App {
    let directory = directory(&format!("l62d-{test}"));
    std::fs::create_dir_all(directory.join("data")).unwrap();
    for (name, clip) in clips {
        let path = directory.join(format!("data/{name}.clip.ron"));
        std::fs::write(path, maki_assets::data_text(clip)).unwrap();
    }
    let text = room(events).replacen("objects: [", &format!("objects: [\n{objects}"), 1);
    std::fs::write(directory.join("a.ron"), text).unwrap();
    let mut app = game(&directory);
    app.add_module(AnimationModule)
        .init_resource::<Changes>()
        .add_systems(Update, record_changes);
    app
}

/// For each frame: the objects whose `Interpolated` or `Light` changed in it.
#[derive(bevy_ecs::resource::Resource, Default)]
struct Changes(Vec<Vec<u32>>);

fn record_changes(
    poses: Query<(&ObjectId, Ref<Interpolated>)>,
    lights: Query<(&ObjectId, Ref<Light>)>,
    mut changes: ResMut<Changes>,
) {
    let poses = poses.iter().filter(|(_, pose)| pose.is_changed());
    let lights = lights.iter().filter(|(_, light)| light.is_changed());
    let mut ids: Vec<u32> = poses.map(|(id, _)| id.0).collect();
    ids.extend(lights.map(|(id, _)| id.0));
    changes.0.push(ids);
}

/// True if the object changed in the last frame.
fn changed(app: &App, id: u32) -> bool {
    let changes = &app.world().resource::<Changes>().0;
    changes.last().unwrap().contains(&id)
}

fn entity(app: &App, id: u32) -> Entity {
    let mut objects = app.world().try_query::<(Entity, &ObjectId)>().unwrap();
    let found = objects.iter(app.world()).find(|(_, object)| object.0 == id);
    found.unwrap().0
}

fn pose(app: &App, id: u32) -> Interpolated {
    *app.world().get::<Interpolated>(entity(app, id)).unwrap()
}

fn jumps(app: &App) -> u64 {
    app.world().resource::<PoseJumps>().0
}

/// A step clip of 0.3 s, looping: up 1 m at 0.1 s, 2 m at 0.2 s.
fn stairs() -> Clip {
    Clip {
        mode: KeyMode::Step,
        fps: None,
        length: 0.3,
        tracks: vec![track(
            Channel::Position,
            &[
                (0.0, vector(0.0, 0.0, 0.0)),
                (0.1, vector(0.0, 1.0, 0.0)),
                (0.2, vector(0.0, 2.0, 0.0)),
            ],
        )],
    }
}

const BOX: &str = r#"(id: 10, name: "box", pose: (position: (1, 0, 0)), components: {"animator": (clip: "data/stairs.clip.ron")}),"#;

#[test]
fn a_step_clip_holds_each_key_and_jumps_with_no_blend() {
    animated("step", &[("stairs", stairs())], BOX, "").run(|mut app| {
        // The frame that loads the room.
        steps(&mut app, 1);
        let start = jumps(&app);
        let (mut heights, mut counts) = (Vec::new(), Vec::new());
        for step in 0..16 {
            steps(&mut app, 1);
            let pose = pose(&app, 10);
            assert_eq!(pose.previous, pose.current, "step {step}: no blend");
            assert_eq!(pose.current.translation.x, 1.0);
            heights.push(pose.current.translation.y);
            counts.push(jumps(&app) - start);
            // A held value writes nothing. Step 0 sees the entity that the room load added.
            if step > 0 {
                let jumped = heights[step] != heights[step - 1];
                assert_eq!(changed(&app, 10), jumped, "step {step}");
            }
        }
        let wanted = [0.0, 1.0, 2.0].map(|y| [y; 5]).concat();
        assert_eq!(heights[..15], wanted[..]);
        // The loop at 0.3 s.
        assert_eq!(heights[15], 0.0);
        let wanted_counts = [[0; 5], [1; 5], [2; 5]].concat();
        assert_eq!(counts[..15], wanted_counts[..]);
        assert_eq!(counts[15], 3);
    });
}

#[test]
fn a_linear_clip_at_8_fps_changes_its_value_8_times_in_a_second() {
    let clip = Clip {
        mode: KeyMode::Linear,
        fps: Some(8.0),
        length: 1.0,
        tracks: vec![track(
            Channel::Position,
            &[(0.0, vector(0.0, 0.0, 0.0)), (1.0, vector(8.0, 0.0, 0.0))],
        )],
    };
    animated("fps", &[("stairs", clip)], BOX, "").run(|mut app| {
        steps(&mut app, 1);
        let mut places = Vec::new();
        // 51 steps of 20 ms: from 0 s to 1 s.
        for _ in 0..=50 {
            steps(&mut app, 1);
            let pose = pose(&app, 10);
            assert_eq!(pose.previous, pose.current);
            places.push(pose.current.translation.x - 1.0);
        }
        let changes = places.windows(2).filter(|pair| pair[0] != pair[1]).count();
        assert_eq!(changes, 8, "{places:?}");
        assert!(places.iter().all(|x| x.fract().abs() < 1e-4), "{places:?}");
    });
}

/// Counts the warnings of the animation module.
struct Warnings(Arc<AtomicUsize>);

impl tracing::Subscriber for Warnings {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let metadata = event.metadata();
        if *metadata.level() == tracing::Level::WARN
            && metadata.target().starts_with("maki_scene::animation")
        {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

#[test]
fn a_field_track_changes_a_light_and_a_bad_track_warns_once() {
    let clip = Clip {
        mode: KeyMode::Step,
        fps: None,
        length: 0.2,
        tracks: vec![
            track(field("nothing", "x"), &[(0.0, KeyValue::Number(1.0))]),
            track(field("light", "nothing"), &[(0.0, KeyValue::Number(1.0))]),
            track(
                field("light", "intensity"),
                &[(0.0, KeyValue::Number(1.0)), (0.1, KeyValue::Number(3.0))],
            ),
        ],
    };
    let lamp = r#"(id: 20, name: "pulse", pose: (position: (0, 2, 1)), components: {"light": (radius: 0.1, color: (1, 1, 1), intensity: 5.0), "animator": (clip: "data/pulse.clip.ron")}),"#;
    let app = animated("light", &[("pulse", clip)], lamp, "");
    let warnings = Arc::new(AtomicUsize::new(0));
    let subscriber = Warnings(warnings.clone());
    tracing::subscriber::with_default(subscriber, || {
        app.run(|mut app| {
            let intensity =
                |app: &App| app.world().get::<Light>(entity(app, 20)).unwrap().intensity;
            steps(&mut app, 2);
            assert_eq!(intensity(&app), 1.0);
            assert!(changed(&app, 20));
            // A held value writes nothing.
            steps(&mut app, 1);
            assert!(!changed(&app, 20));
            steps(&mut app, 4);
            assert_eq!(intensity(&app), 3.0);
            steps(&mut app, 20);
        });
    });
    assert_eq!(warnings.load(Ordering::Relaxed), 2);
}

#[test]
fn a_set_field_of_the_clip_cuts_to_the_first_key_of_the_new_clip() {
    let slide = Clip {
        mode: KeyMode::Linear,
        fps: None,
        length: 1.0,
        tracks: vec![track(
            Channel::Position,
            &[(0.0, vector(0.0, 0.0, 0.0)), (1.0, vector(0.0, 0.0, 4.0))],
        )],
    };
    let hop = Clip {
        mode: KeyMode::Linear,
        fps: None,
        length: 1.0,
        tracks: vec![track(
            Channel::Position,
            &[(0.0, vector(5.0, 0.0, 0.0)), (1.0, vector(6.0, 0.0, 0.0))],
        )],
    };
    let object = BOX.replace("stairs", "slide");
    let events = r#"(name: "cut", pages: [(trigger: Autorun, commands: [Wait(0.2), SetField(10, "animator", "clip", "data/hop.clip.ron")])])"#;
    animated("cut", &[("slide", slide), ("hop", hop)], &object, events).run(|mut app| {
        steps(&mut app, 3);
        // The slide blends between steps.
        let sliding = pose(&app, 10);
        assert!(sliding.previous != sliding.current && sliding.current.translation.z > 0.0);
        for _ in 0..40 {
            let before = jumps(&app);
            steps(&mut app, 1);
            let pose = pose(&app, 10);
            if pose.current.translation.x == 1.0 {
                continue;
            }
            // The first key of the new clip, from the rest pose, with no blend.
            assert_eq!(pose.current.translation, Vec3::new(6.0, 0.0, 0.0));
            assert_eq!(pose.previous, pose.current);
            assert_eq!(jumps(&app), before + 1);
            return;
        }
        panic!("no cut");
    });
}

#[test]
fn one_clip_moves_two_objects_from_their_own_rest_poses() {
    let clip = Clip {
        mode: KeyMode::Step,
        fps: None,
        length: 0.0,
        tracks: vec![track(Channel::Position, &[(0.0, vector(0.0, 0.0, 1.0))])],
    };
    let objects = r#"
        (id: 11, name: "a", pose: (position: (1, 0, 0), rotation: (0, 90, 0)), components: {"animator": (clip: "data/push.clip.ron")}),
        (id: 12, name: "b", pose: (position: (-2, 0, 3)), components: {"animator": (clip: "data/push.clip.ron")}),"#;
    animated("two", &[("push", clip)], objects, "").run(|mut app| {
        steps(&mut app, 3);
        let a = pose(&app, 11).current.translation;
        let b = pose(&app, 12).current.translation;
        assert!(a.abs_diff_eq(Vec3::new(2.0, 0.0, 0.0), 1e-5), "{a}");
        assert!(b.abs_diff_eq(Vec3::new(-2.0, 0.0, 4.0), 1e-5), "{b}");
    });
}

#[test]
fn speed_0_and_no_play_hold_the_pose() {
    let objects = r#"
        (id: 13, name: "still", pose: (position: (1, 0, 0)), components: {"animator": (clip: "data/stairs.clip.ron", speed: 0.0)}),
        (id: 14, name: "off", pose: (position: (2, 0, 0)), components: {"animator": (clip: "data/stairs.clip.ron", playing: false)}),"#;
    animated("hold", &[("stairs", stairs())], objects, "").run(|mut app| {
        steps(&mut app, 2);
        for _ in 0..20 {
            steps(&mut app, 1);
            assert!(!changed(&app, 13) && !changed(&app, 14));
        }
        assert_eq!(pose(&app, 13).current.translation, Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(pose(&app, 14).current.translation, Vec3::new(2.0, 0.0, 0.0));
    });
}

#[test]
fn reset_play_gives_back_the_rest_pose() {
    animated("reset", &[("stairs", stairs())], BOX, "").run(|mut app| {
        steps(&mut app, 8);
        assert_eq!(pose(&app, 10).current.translation.y, 1.0);
        app.reset_play();
        let rest = Interpolated::at(Transform::from_translation(Vec3::X));
        assert_eq!(pose(&app, 10), rest);
        // The time of play is 0 again: the first key.
        steps(&mut app, 1);
        assert_eq!(pose(&app, 10), rest);
    });
}

#[test]
fn a_changed_clip_file_plays_at_the_same_time() {
    animated("reload", &[("stairs", stairs())], BOX, "").run(|mut app| {
        steps(&mut app, 7);
        assert_eq!(pose(&app, 10).current.translation.y, 1.0);
        let mut tall = stairs();
        tall.tracks[0].keys[1].value = vector(0.0, 10.0, 0.0);
        let path = directory("l62d-reload").join("data/stairs.clip.ron");
        std::fs::write(&path, maki_assets::data_text(&tall)).unwrap();
        // What the watch does at a change (`AssetsModule` is not in this app).
        app.resource_mut::<maki_assets::DataAssets>().forget(&path);
        // Not the first key again: the time of play is kept.
        steps(&mut app, 2);
        assert_eq!(pose(&app, 10).current.translation.y, 10.0);
    });
}

#[test]
fn the_first_step_pose_away_from_the_rest_pose_counts_one_jump() {
    let clip = Clip {
        mode: KeyMode::Step,
        fps: None,
        length: 0.0,
        tracks: vec![track(Channel::Position, &[(0.0, vector(0.0, 1.0, 0.0))])],
    };
    let object = BOX.replace("stairs", "up");
    animated("first", &[("up", clip)], &object, "").run(|mut app| {
        steps(&mut app, 3);
        assert_eq!(pose(&app, 10).current.translation.y, 1.0);
        assert_eq!(jumps(&app), 1);
    });
}

#[test]
fn the_room_check_warns_of_an_animator_with_a_fixed_collider() {
    let objects = r#"
        (id: 20, name: "crate", components: {"prop": (mesh: Cube(1.0)), "animator": (clip: "data/stairs.clip.ron")}),
        (id: 21, name: "wall", components: {"collider": (), "animator": (clip: "data/stairs.clip.ron")}),
        (id: 22, name: "ghost", components: {"prop": (mesh: Cube(1.0), collider: false), "animator": (clip: "data/stairs.clip.ron")}),
        (id: 23, name: "ball", components: {"collider": (), "body": Fixed, "animator": (clip: "data/stairs.clip.ron")}),"#;
    let mut app = animated("check", &[("stairs", stairs())], objects, "");
    // The check knows the types of components after the modules are built.
    app.update();
    let path = directory("l62d-check").join("a.ron");
    let text = std::fs::read_to_string(&path).unwrap();
    let room = maki_scene::parse_room(&text).unwrap();
    let check = maki_scene::RoomCheck {
        path: &path,
        room: &room,
        text: &text,
    };
    let validators = app.world().resource::<maki_scene::RoomValidators>();
    let problems: Vec<_> = validators
        .0
        .iter()
        .flat_map(|validate| validate(&check))
        .collect();
    let fixed: Vec<_> = problems
        .iter()
        .filter(|problem| problem.warning && problem.text.contains("fixed collider"))
        .map(|problem| problem.text.as_str())
        .collect();
    assert_eq!(fixed.len(), 2, "{fixed:?}");
    assert!(
        fixed[0].contains("`crate`") && fixed[1].contains("`wall`"),
        "{fixed:?}"
    );
}
