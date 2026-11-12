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
//! `animate` runs before the events (`run_events.after(animate)`): an exclusive system
//! after them would need an order to each system that runs after them. So a `SetField`
//! of an animator plays from the next fixed step.

use std::path::Path;
use std::sync::Arc;

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::query::QueryState;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::Query;
use bevy_ecs::world::World;
use maki_assets::clip::{Channel, Clip, KeyMode, Sample, clip_time};
use maki_assets::{DataAssets, DataRef, FileRef, add_data_type};
use maki_core::{
    App, Fields, FixedUpdate, Interpolated, Module, PoseJumps, ResetPlay, Time, Transform,
};
use serde::{Deserialize, Serialize};

use crate::{CurrentRoom, add_object_component, move_movers, write_field};

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
}

/// One fixed step of one animator.
fn play(world: &mut World, entity: Entity, animator: &Animator, playback: &mut Playback, dt: f64) {
    let cut = playback.file.as_ref() != Some(&animator.clip.file);
    if cut {
        playback.steps = 0.0;
        playback.warned.clear();
        playback.file = Some(animator.clip.file.clone());
        playback.clip = load(world, &animator.clip);
        if playback.rest.is_none() {
            let pose = world.get::<Interpolated>(entity).map(|pose| pose.current);
            playback.rest = pose.or_else(|| world.get::<Transform>(entity).copied());
        }
    }
    let Some(clip) = playback.clip.clone() else {
        return;
    };
    let time = (playback.steps * dt) as f32;
    let t = clip_time(time, clip.duration(), animator.looping, clip.fps);
    playback.steps += f64::from(animator.speed);

    // The key of each transform channel; a channel with no track keeps the rest.
    let mut key = Transform::IDENTITY;
    let (mut posed, mut stepped) = (false, cut || clip.fps.is_some());
    for (index, track) in clip.tracks.iter().enumerate() {
        let Some(sample) = track.sample(t, clip.mode) else {
            continue;
        };
        let step = track.mode_in(clip.mode) == KeyMode::Step;
        match (&track.channel, sample) {
            (Channel::Position, Sample::Vector(v)) => key.translation = v,
            (Channel::Scale, Sample::Vector(v)) => key.scale = v,
            (Channel::Rotation, Sample::Rotation(r)) => key.rotation = r,
            (Channel::Field { component, field }, Sample::Value(value)) => {
                let written = write_field(world, entity, component, field, &value);
                if let Err(error) = written
                    && !playback.warned.contains(&index)
                {
                    playback.warned.push(index);
                    tracing::warn!(
                        ?entity,
                        component,
                        field,
                        ?error,
                        "a field track does not write"
                    );
                }
                continue;
            }
            _ => continue,
        }
        posed = true;
        stepped |= step;
    }
    if !posed {
        return;
    }
    let rest = playback.rest.unwrap_or(Transform::IDENTITY);
    write_pose(world, entity, product(&rest, &key), stepped);
}

/// `rest x key`: the key in the space of the rest pose.
fn product(rest: &Transform, key: &Transform) -> Transform {
    Transform {
        translation: rest.translation + rest.rotation * (rest.scale * key.translation),
        rotation: (rest.rotation * key.rotation).normalize(),
        scale: rest.scale * key.scale,
    }
}

/// Writes the pose only if it differs (a write is a change for the renderer). `snap`:
/// with no blend, and one more `PoseJumps`.
fn write_pose(world: &mut World, entity: Entity, pose: Transform, snap: bool) {
    let Some(mut current) = world.get_mut::<Interpolated>(entity) else {
        // Only an animated object gets one; its first pose has no blend.
        world.entity_mut(entity).insert(Interpolated::at(pose));
        return;
    };
    if snap {
        if *current != Interpolated::at(pose) {
            let mut jumps = PoseJumps::default();
            current.snap(pose, &mut jumps);
            world.resource_mut::<PoseJumps>().0 += jumps.0;
        }
    } else if current.current != pose {
        current.current = pose;
    }
}

/// The clip of a reference, from the directory of the room. `None`, with a warning:
/// the file does not read.
fn load(world: &mut World, clip: &DataRef<Clip>) -> Option<Arc<Clip>> {
    let room = world.get_resource::<CurrentRoom>();
    let directory = room
        .and_then(|room| room.path.parent())
        .unwrap_or(Path::new(""));
    let directory = directory.to_owned();
    let loaded = world
        .resource_mut::<DataAssets>()
        .get::<Clip>(&clip.file, &directory);
    loaded
        .inspect_err(|error| tracing::warn!(%error, "an animator has no clip"))
        .ok()
}

/// Runs in `ResetPlay`: each animated object goes back to its rest pose, and the time
/// of play to 0.
fn reset_animators(mut animators: Query<(&mut Playback, Option<&mut Interpolated>)>) {
    for (mut playback, pose) in &mut animators {
        // No jump count: a room load follows each `ResetPlay`.
        if let (Some(rest), Some(mut pose)) = (playback.rest, pose)
            && *pose != Interpolated::at(rest)
        {
            *pose = Interpolated::at(rest);
        }
        *playback = Playback::default();
    }
}
