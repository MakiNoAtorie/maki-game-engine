//! The animator: an object component that plays a clip (`maki_assets::clip`) on its
//! own object.
//!
//! ```ron
//! "animator": (clip: "data/jump.clip.ron", speed: 1.0, looping: true, playing: true)
//! ```
//!
//! Each fixed step [`animate`] samples the clip at the time of play and writes the
//! tracks: a transform track writes `Interpolated::current`, a field track writes
//! through `write_field`. A transform track is relative to the rest pose (the pose of
//! the object when its first clip starts): pose = rest x key. A write from a step (a
//! step key, a clip with `fps`, a cut) is a snap, so the picture jumps.
//!
//! A track with a `target` writes the joint of that name in the `SkinPose` of the
//! object: joint pose = joint rest x key.
//!
//! `animate` runs before the events (`run_events.after(animate)`): an exclusive system
//! after them would need an order to each system that runs after them. So a `SetField`
//! of an animator plays from the next fixed step.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::query::QueryState;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::Query;
use bevy_ecs::world::World;
use maki_assets::clip::{Channel, Clip, KeyMode, Sample, Track, clip_time};
use maki_assets::{DataAssets, DataRef, FileRef, add_data_type};
use maki_core::{
    App, Fields, FixedUpdate, Interpolated, Module, PoseJumps, ResetPlay, Time, Transform,
};
use serde::{Deserialize, Serialize};

use crate::{CurrentRoom, SkinPose, add_object_component, move_movers, write_field};

/// Adds the component `animator`, the data asset `clip`, and [`animate`].
pub struct AnimationModule;

impl Module for AnimationModule {
    fn build(&self, app: &mut App) {
        add_data_type::<Clip>(app);
        add_object_component::<Animator>(app, "animator");
        app.init_resource::<DataAssets>()
            // Before the movers: the physics sees the new poses.
            .add_systems(FixedUpdate, animate.before(move_movers))
            .add_systems(ResetPlay, reset_animators);
        crate::timeline::add(app);
        crate::light_animation::add(app);
    }
}

/// Plays a clip on its object.
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
#[fields(default)]
// The run state is apart: the live form inserts the whole `animator` again.
#[require(Playback)]
pub struct Animator {
    /// A clip other than the one that plays starts at its first key, with no blend.
    pub clip: DataRef<Clip>,
    /// 1: as in the clip. 0: the pose holds.
    #[field(range = 0.0..=10.0, step = 0.05)]
    pub speed: f32,
    /// False: the last key holds at the end.
    pub looping: bool,
    /// False: the pose holds.
    pub playing: bool,
}

impl Default for Animator {
    fn default() -> Self {
        Self {
            clip: DataRef::default(),
            speed: 1.0,
            looping: true,
            playing: true,
        }
    }
}

/// The run state of an [`Animator`]. `ResetPlay` gives it back.
#[derive(Component, Debug, Default, Clone)]
pub struct Playback {
    /// The time of play in fixed steps (times the speed): a count, as a sum of
    /// float seconds drifts.
    steps: f64,
    /// The file of the clip that plays. `None`: no clip started.
    file: Option<FileRef>,
    /// `None`: the file does not read (a warning was given).
    clip: Option<Arc<Clip>>,
    /// The pose of the object when its first clip started.
    rest: Option<Transform>,
    /// The tracks of the clip that warned of a write that failed: one warning each.
    warned: Vec<usize>,
}

/// Runs in `FixedUpdate`, exclusive: a field track writes through `Live`, which needs
/// the world. Fixed-step time only, so each run gives the same poses.
pub fn animate(world: &mut World, animators: &mut QueryState<(Entity, &Animator)>) {
    let dt = world.resource::<Time>().fixed_delta.as_secs_f64();
    let list: Vec<(Entity, Animator)> = animators
        .iter(world)
        .map(|(entity, animator)| (entity, animator.clone()))
        .collect();
    for (entity, animator) in list {
        if !animator.playing {
            continue;
        }
        let Some(mut playback) = world.get_mut::<Playback>(entity) else {
            continue;
        };
        // Out of the entity while the tracks write the world, then back.
        let mut playback = std::mem::take(&mut *playback);
        play(world, entity, &animator, &mut playback, dt);
        if let Some(mut slot) = world.get_mut::<Playback>(entity) {
            *slot = playback;
        }
    }
    // After the animators: a timeline track on the same channel wins.
    crate::timeline::play(world, dt);
}

/// One fixed step of one animator.
fn play(world: &mut World, entity: Entity, animator: &Animator, playback: &mut Playback, dt: f64) {
    let cut = playback.file.as_ref() != Some(&animator.clip.file);
    if cut {
        playback.steps = 0.0;
        playback.warned.clear();
        playback.file = Some(animator.clip.file.clone());
        let loaded = load(world, &animator.clip);
        playback.clip = loaded
            .inspect_err(|error| tracing::warn!(%error, "an animator has no clip"))
            .ok();
        if playback.rest.is_none() {
            let pose = world.get::<Interpolated>(entity).map(|pose| pose.current);
            playback.rest = pose.or_else(|| world.get::<Transform>(entity).copied());
        }
    } else if let Ok(clip) = load(world, &animator.clip)
        && playback
            .clip
            .as_ref()
            .is_none_or(|old| !Arc::ptr_eq(old, &clip))
    {
        // A changed file plays at the same time; a file that does not read keeps the old clip.
        playback.warned.clear();
        playback.clip = Some(clip);
    }
    let Some(clip) = playback.clip.clone() else {
        return;
    };
    let time = (playback.steps * dt) as f32;
    let t = clip_time(time, clip.duration(), animator.looping, clip.fps);
    playback.steps += f64::from(animator.speed);

    let target = Target {
        entity,
        rest: playback.rest.unwrap_or(Transform::IDENTITY),
        cut,
        fps: clip.fps.is_some(),
    };
    write_tracks(
        world,
        &target,
        &clip.tracks,
        t,
        clip.mode,
        &mut playback.warned,
    );
}

/// What a track list writes to: one entity, its start pose, and how the time moved.
pub(crate) struct Target {
    pub entity: Entity,
    /// The pose of the entity at the start: a transform track is relative to it.
    pub rest: Transform,
    /// The tracks changed (a new clip, a first step): every write is a jump, and a
    /// joint with no track goes to its rest.
    pub cut: bool,
    /// The tracks step (a clip with `fps`): every write is a jump.
    pub fps: bool,
}

/// Writes the tracks sampled at `t` to `target.entity`: transform tracks as rest x key,
/// joint tracks into its `SkinPose`, field tracks through `write_field`. A step write
/// counts one `PoseJumps`. `warned`: the indices of the tracks that already warned
/// (one warning each); the caller keeps it per owner and clears it when the tracks change.
pub(crate) fn write_tracks(
    world: &mut World,
    target: &Target,
    tracks: &[Track],
    t: f32,
    mode: KeyMode,
    warned: &mut Vec<usize>,
) {
    let Target {
        entity,
        rest,
        cut,
        fps,
    } = *target;
    // The key of each transform channel; a channel with no track keeps the rest.
    let mut key = Transform::IDENTITY;
    let (mut posed, mut stepped) = (false, cut || fps);
    // The key of each joint with a track, and if it is a step.
    let mut joint_keys: BTreeMap<usize, (Transform, bool)> = BTreeMap::new();
    let skeleton = world
        .get::<SkinPose>(entity)
        .map(|pose| pose.skeleton.clone());
    for (index, track) in tracks.iter().enumerate() {
        let Some(sample) = track.sample(t, mode) else {
            continue;
        };
        let step = track.mode_in(mode) == KeyMode::Step;
        if let Channel::Field { component, field } = &track.channel {
            if let Sample::Value(value) = sample {
                let written = write_field(world, entity, component, field, &value);
                if let Err(error) = written
                    && !warned.contains(&index)
                {
                    warned.push(index);
                    tracing::warn!(
                        ?entity,
                        component,
                        field,
                        ?error,
                        "a field track does not write"
                    );
                }
            }
            continue;
        }
        let Some(target) = &track.target else {
            if set_channel(&mut key, &track.channel, sample) {
                posed = true;
                stepped |= step;
            }
            continue;
        };
        // No skeleton yet: the model has not loaded, which is not a missing joint.
        let Some(skeleton) = &skeleton else {
            continue;
        };
        let joints = &skeleton.joints;
        let Some(joint) = joints.iter().position(|joint| joint.name == *target) else {
            if !warned.contains(&index) {
                warned.push(index);
                tracing::warn!(?entity, target, "a track names no joint of the model");
            }
            continue;
        };
        let (key, snap) = joint_keys
            .entry(joint)
            .or_insert((Transform::IDENTITY, cut || fps));
        if set_channel(key, &track.channel, sample) {
            *snap |= step;
        }
    }
    // A cut sends a joint with no track in the new clip to its rest, as an object channel.
    if cut && let Some(skeleton) = &skeleton {
        for joint in 0..skeleton.joints.len() {
            joint_keys
                .entry(joint)
                .or_insert((Transform::IDENTITY, true));
        }
    }
    if !joint_keys.is_empty() {
        write_joints(world, entity, &joint_keys);
    }
    if !posed {
        return;
    }
    write_pose(world, entity, pose_on_rest(&rest, &key), stepped);
}

/// Sets the part of `key` of a transform channel. False: not a transform sample.
pub fn set_channel(key: &mut Transform, channel: &Channel, sample: Sample) -> bool {
    match (channel, sample) {
        (Channel::Position, Sample::Vector(v)) => key.translation = v,
        (Channel::Scale, Sample::Vector(v)) => key.scale = v,
        (Channel::Rotation, Sample::Rotation(r)) => key.rotation = r,
        _ => return false,
    }
    true
}

/// Writes the joints of `keys` into `SkinPose::current` (joint rest x key), only if a
/// pose differs. A snap sets `previous` too and counts one `PoseJumps` for the pose.
fn write_joints(world: &mut World, entity: Entity, keys: &BTreeMap<usize, (Transform, bool)>) {
    let Some(skin) = world.get::<SkinPose>(entity) else {
        return;
    };
    let (mut previous, mut current) = (skin.previous.clone(), skin.current.clone());
    let mut jumped = false;
    for (&joint, (key, snap)) in keys {
        let pose = pose_on_rest(&skin.skeleton.joints[joint].rest, key);
        if *snap {
            jumped |= previous[joint] != pose || current[joint] != pose;
            previous[joint] = pose;
        }
        current[joint] = pose;
    }
    if previous != skin.previous || current != skin.current {
        let mut skin = world.get_mut::<SkinPose>(entity).expect("read above");
        skin.previous = previous;
        skin.current = current;
    }
    // Only a jump touches the count: a write marks `PoseJumps` as changed.
    if jumped {
        world.resource_mut::<PoseJumps>().add(1);
    }
}

/// `rest x key`: the key in the space of the rest pose.
pub fn pose_on_rest(rest: &Transform, key: &Transform) -> Transform {
    Transform {
        translation: rest.translation + rest.rotation * (rest.scale * key.translation),
        rotation: (rest.rotation * key.rotation).normalize(),
        scale: rest.scale * key.scale,
    }
}

/// Writes the pose only if it differs (a write is a change for the renderer). `snap`:
/// with no blend, and one more `PoseJumps`.
fn write_pose(world: &mut World, entity: Entity, pose: Transform, snap: bool) {
    let jumped = match world.get_mut::<Interpolated>(entity) {
        Some(mut current) if snap => {
            let jumped = *current != Interpolated::at(pose);
            if jumped {
                current.snap(pose);
            }
            jumped
        }
        Some(mut current) => {
            if current.current != pose {
                current.current = pose;
            }
            false
        }
        None => {
            // Only an animated object gets one; its first pose has no blend.
            let jumped = snap && world.get::<Transform>(entity) != Some(&pose);
            world.entity_mut(entity).insert(Interpolated::at(pose));
            jumped
        }
    };
    // Only a jump touches the count: a write marks `PoseJumps` as changed.
    if jumped {
        world.resource_mut::<PoseJumps>().add(1);
    }
}

/// The clip of a reference, from the directory of the room. `Err`: the file does not
/// read.
fn load(world: &mut World, clip: &DataRef<Clip>) -> Result<Arc<Clip>, String> {
    let room = world.get_resource::<CurrentRoom>();
    let directory = room
        .and_then(|room| room.path.parent())
        .unwrap_or(Path::new(""));
    let directory = directory.to_owned();
    // Asked in each step of each animator: a cache hit must not mark the resource changed.
    world
        .resource_mut::<DataAssets>()
        .bypass_change_detection()
        .get::<Clip>(&clip.file, &directory)
}

/// Runs in `ResetPlay`: each animated object goes back to its rest pose, and the time
/// of play to 0.
fn reset_animators(mut animators: Query<(&mut Playback, Option<&mut Interpolated>)>) {
    for (mut playback, pose) in &mut animators {
        if let (Some(rest), Some(mut pose)) = (playback.rest, pose)
            && *pose != Interpolated::at(rest)
        {
            pose.snap(rest);
        }
        *playback = Playback::default();
    }
}
