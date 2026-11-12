//! The pose of the joints (`maki_scene::skin`): target tracks of the animator, the
//! counted jump of a step, the palette, `ResetPlay`, and the object entity of a
//! skinned model.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use bevy_ecs::change_detection::DetectChanges;
use bevy_ecs::entity::Entity;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Query, ResMut};
use bevy_ecs::world::Ref;
use glam::{Mat4, Vec3};
use maki_assets::clip::{Channel, Clip, Key, KeyMode, KeyValue, Track};
use maki_core::{App, ConfigFile, Interpolated, PoseJumps, Transform, Update};

use crate::{
    AnimationModule, Animator, Collider, MeshInstance, ObjectId, RoomItem, RoomRequest, RoomTarget,
    SceneModule, SkinPalette, SkinPose, make_palettes, room_text,
};

/// A skinned triangle: joints `root` (rest at (0, 1, 0), from the node `rig` above it)
/// and its child `arm` (rest (1, 0, 0) from `root`). All vertices on `arm`.
fn write_figure(directory: &std::path::Path) {
    let positions = [Vec3::ZERO, Vec3::X, Vec3::Y];
    let joints = [[0u16, 0, 0, 0]; 3];
    let weights = [[1.0f32, 0.0, 0.0, 0.0]; 3];
    // In the order of the skin: node 3 (`arm`, at (1, 1, 0)), then node 2 (`root`).
    let inverse_binds = [
        Mat4::from_translation(Vec3::new(-1.0, -1.0, 0.0)),
        Mat4::from_translation(Vec3::new(0.0, -1.0, 0.0)),
    ];
    let mut bin: Vec<u8> = positions
        .iter()
        .flat_map(|p| p.to_array())
        .flat_map(f32::to_le_bytes)
        .collect();
    bin.extend(joints.as_flattened().iter().flat_map(|j| j.to_le_bytes()));
    bin.extend(weights.as_flattened().iter().flat_map(|w| w.to_le_bytes()));
    bin.extend(
        inverse_binds
            .iter()
            .flat_map(|m| m.to_cols_array())
            .flat_map(f32::to_le_bytes),
    );
    let json = format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0,1]}}],
"nodes":[{{"name":"rig","translation":[0,1,0],"children":[2]}},
{{"mesh":0,"skin":0}},
{{"name":"root","children":[3]}},
{{"name":"arm","translation":[1,0,0]}}],
"skins":[{{"joints":[3,2],"inverseBindMatrices":3}}],
"meshes":[{{"name":"skinned","primitives":[{{"attributes":{{"POSITION":0,"JOINTS_0":1,"WEIGHTS_0":2}}}}]}}],
"buffers":[{{"uri":"figure.bin","byteLength":{}}}],
"bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":24}},
{{"buffer":0,"byteOffset":60,"byteLength":48}},{{"buffer":0,"byteOffset":108,"byteLength":128}}],
"accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}},
{{"bufferView":1,"componentType":5123,"count":3,"type":"VEC4"}},
{{"bufferView":2,"componentType":5126,"count":3,"type":"VEC4"}},
{{"bufferView":3,"componentType":5126,"count":2,"type":"MAT4"}}]}}"#,
        bin.len()
    );
    let models = directory.join("models");
    std::fs::create_dir_all(&models).unwrap();
    std::fs::write(models.join("figure.bin"), bin).unwrap();
    std::fs::write(models.join("figure.gltf"), json).unwrap();
}

/// A clip of 0.3 s, looping: the joint `target` at rest, turned 90 degrees around Z
/// from 0.1 s.
fn turn(target: &str, mode: KeyMode) -> Clip {
    let key = |time, z| Key {
        time,
        value: KeyValue::Vector([0.0, 0.0, z]),
    };
    Clip {
        mode,
        fps: None,
        length: 0.3,
        tracks: vec![Track {
            channel: Channel::Rotation,
            target: Some(target.to_owned()),
            mode: None,
            keys: vec![key(0.0, 0.0), key(0.1, 90.0)],
        }],
    }
}

/// For each frame: true if the `SkinPalette` changed in it.
#[derive(Resource, Default)]
struct Changes(Vec<bool>);

/// A clip file that the animators take in the next frame.
#[derive(Resource, Default)]
struct NextClip(Option<maki_assets::FileRef>);

/// A game at 50 Hz in a room with the figure as object 10 at (1, 0, 0), with an
/// animator that plays `clip`.
fn figure(test: &str, clip: Clip) -> App {
    let directory: PathBuf =
        std::env::temp_dir().join(format!("maki-scene-l63d-{test}-{}", std::process::id()));
    write_figure(&directory);
    std::fs::create_dir_all(directory.join("data")).unwrap();
    let text = maki_assets::data_text(&clip);
    std::fs::write(directory.join("data/move.clip.ron"), text).unwrap();
    let object = r#"(id: 10, name: "figure", pose: (position: (1, 0, 0)), components: {"prop": (mesh: File("models/figure.gltf")), "animator": (clip: "data/move.clip.ron")})"#;
    std::fs::write(directory.join("a.ron"), room_text(4.0, 4.0, 3.0, &[object])).unwrap();

    // On the thread of the test: the model is there in the frame of the load.
    let config = "[time]\nfixed_hz = 50.0\n[assets]\nbackground = false";
    let mut app = App::new(ConfigFile::from_text(config)).unwrap();
    app.add_module(maki_assets::AssetsModule)
        .add_module(SceneModule)
        .add_module(AnimationModule)
        .init_resource::<Changes>()
        .init_resource::<NextClip>()
        .add_systems(
            Update,
            |mut next: ResMut<NextClip>, mut animators: Query<&mut Animator>| {
                if let Some(file) = next.0.take() {
                    animators
                        .iter_mut()
                        .for_each(|mut a| a.clip.file = file.clone());
                }
            },
        )
        .add_systems(
            Update,
            (|palettes: Query<Ref<SkinPalette>>, mut changes: ResMut<Changes>| {
                changes
                    .0
                    .push(palettes.iter().any(|palette| palette.is_changed()));
            })
            .after(make_palettes),
        );
    app.resource_mut::<RoomRequest>().0 = Some(RoomTarget {
        path: directory.join("a.ron"),
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

fn entity(app: &App) -> Entity {
    let mut objects = app.world().try_query::<(Entity, &ObjectId)>().unwrap();
    let found = objects.iter(app.world()).find(|(_, id)| id.0 == 10);
    found.expect("the object entity").0
}

fn skin(app: &App) -> SkinPose {
    app.world().get::<SkinPose>(entity(app)).unwrap().clone()
}

fn joint(pose: &SkinPose, name: &str) -> usize {
    let joints = &pose.skeleton.joints;
    joints.iter().position(|joint| joint.name == name).unwrap()
}

fn jumps(app: &App) -> u64 {
    app.world().resource::<PoseJumps>().0
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
fn a_skinned_model_keeps_its_object_entity_with_the_animator() {
    figure("entity", turn("arm", KeyMode::Linear)).run(|mut app| {
        steps(&mut app, 2);
        let world = app.world();
        let object = world.entity(entity(&app));
        assert!(object.contains::<Animator>());
        assert!(object.contains::<SkinPose>() && object.contains::<SkinPalette>());
        assert!(object.contains::<Collider>());
        // One instance draws the whole model.
        let mut meshes = app
            .world()
            .try_query::<(&MeshInstance, &RoomItem)>()
            .unwrap();
        assert_eq!(meshes.iter(app.world()).count(), 1);
    });
}

#[test]
fn a_target_track_moves_its_joint_and_not_the_object() {
    figure("target", turn("arm", KeyMode::Linear)).run(|mut app| {
        steps(&mut app, 8);
        let pose = skin(&app);
        let (root, arm) = (joint(&pose, "root"), joint(&pose, "arm"));
        let rest = |index: usize| pose.skeleton.joints[index].rest;
        assert_eq!(pose.current[root], rest(root));
        assert_eq!(pose.current[arm].translation, rest(arm).translation);
        assert_ne!(pose.current[arm].rotation, rest(arm).rotation);
        let object = app.world().get::<Interpolated>(entity(&app)).unwrap();
        assert_eq!(
            *object,
            Interpolated::at(Transform::from_translation(Vec3::X))
        );
    });
}

#[test]
fn an_unknown_target_is_a_warning_once() {
    let app = figure("unknown", turn("tail", KeyMode::Linear));
    let warnings = Arc::new(AtomicUsize::new(0));
    tracing::subscriber::with_default(Warnings(warnings.clone()), || {
        app.run(|mut app| {
            steps(&mut app, 20);
            let pose = skin(&app);
            assert_eq!(pose, SkinPose::rest(pose.skeleton.clone()));
            // At rest each joint is at its bind pose: the palette moves no vertex.
            let palette = app.world().get::<SkinPalette>(entity(&app)).unwrap();
            assert_eq!(palette.0.len(), 2);
            let identity = |m: &Mat4| m.abs_diff_eq(Mat4::IDENTITY, 1e-5);
            assert!(palette.0.iter().all(identity), "{palette:?}");
        });
    });
    assert_eq!(warnings.load(Ordering::Relaxed), 1);
}

#[test]
fn a_track_of_a_model_that_is_not_there_is_no_warning() {
    let app = figure("no_model", turn("arm", KeyMode::Linear));
    let directory =
        std::env::temp_dir().join(format!("maki-scene-l63d-no_model-{}", std::process::id()));
    std::fs::remove_file(directory.join("models/figure.gltf")).unwrap();
    let warnings = Arc::new(AtomicUsize::new(0));
    tracing::subscriber::with_default(Warnings(warnings.clone()), || {
        app.run(|mut app| steps(&mut app, 20));
    });
    assert_eq!(warnings.load(Ordering::Relaxed), 0);
}

#[test]
fn a_cut_gives_a_joint_with_no_track_its_rest() {
    figure("cut", turn("arm", KeyMode::Linear)).run(|mut app| {
        steps(&mut app, 8);
        let pose = skin(&app);
        let arm = joint(&pose, "arm");
        assert_ne!(pose.current[arm], pose.skeleton.joints[arm].rest);
        let directory =
            std::env::temp_dir().join(format!("maki-scene-l63d-cut-{}", std::process::id()));
        let text = maki_assets::data_text(&turn("root", KeyMode::Linear));
        std::fs::write(directory.join("data/root.clip.ron"), text).unwrap();
        app.resource_mut::<NextClip>().0 = Some("data/root.clip.ron".into());
        let count = jumps(&app);
        steps(&mut app, 2);
        let pose = skin(&app);
        assert_eq!(pose.current[arm], pose.skeleton.joints[arm].rest);
        // A cut has no blend.
        assert_eq!(pose.previous[arm], pose.current[arm]);
        assert_eq!(jumps(&app) - count, 1);
    });
}

#[test]
fn a_step_write_of_a_joint_counts_one_jump_and_leaves_no_blend() {
    figure("step", turn("arm", KeyMode::Step)).run(|mut app| {
        steps(&mut app, 2);
        let arm = joint(&skin(&app), "arm");
        let mut seen = 0;
        for step in 0..20 {
            let (before, count) = (skin(&app).current[arm], jumps(&app));
            steps(&mut app, 1);
            let pose = skin(&app);
            assert_eq!(pose.previous, pose.current, "step {step}: no blend");
            let moved = pose.current[arm] != before;
            assert_eq!(jumps(&app) - count, u64::from(moved), "step {step}");
            seen += usize::from(moved);
        }
        // Turned at 0.1 s, back at the loop at 0.3 s: two jumps in each loop.
        assert!(seen >= 2, "{seen}");
    });
}

#[test]
fn the_palette_does_not_change_while_the_pose_holds() {
    figure("palette", turn("arm", KeyMode::Step)).run(|mut app| {
        steps(&mut app, 2);
        let start = jumps(&app);
        app.resource_mut::<Changes>().0.clear();
        // Frames of many lengths: the blend factor differs from frame to frame.
        for ms in [7, 13, 20, 3, 21, 16, 9, 5, 40, 11, 30, 17, 25, 8, 19] {
            app.advance(Duration::from_millis(ms));
        }
        let changes = &app.world().resource::<Changes>().0;
        let changed = changes.iter().filter(|changed| **changed).count() as u64;
        let jumped = jumps(&app) - start;
        assert!(jumped >= 1);
        assert_eq!(changed, jumped, "{changes:?}");
    });
}

#[test]
fn reset_play_gives_back_the_rest_of_the_joints() {
    figure("reset", turn("arm", KeyMode::Step)).run(|mut app| {
        steps(&mut app, 8);
        let pose = skin(&app);
        assert_ne!(pose, SkinPose::rest(pose.skeleton.clone()));
        app.reset_play();
        let pose = skin(&app);
        assert_eq!(pose, SkinPose::rest(pose.skeleton.clone()));
    });
}
