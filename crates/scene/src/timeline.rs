//! The timeline: an object component with tracks of keys, each on an object of the
//! room by its id.
//!
//! ```ron
//! "timeline": (length: 2.0, playing: true, tracks: [
//!     (object: 3, track: (channel: Position, mode: Step, keys: [
//!         (time: 0.0, value: Vector((0.0, 0.0, 0.0))),
//!         (time: 1.0, value: Vector((0.0, 1.0, 0.0))),
//!     ])),
//!     (track: (channel: Field(component: "light", field: "intensity"), keys: [
//!         (time: 0.0, value: Number(1.0)),
//!         (time: 2.0, value: Number(4.0)),
//!     ])),
//! ])
//! ```
//!
//! Object 0 (no `object`) is the object of the timeline. The tracks and their sampling
//! are those of a clip; the write is the one of the animator (`write_tracks`), at the
//! end of `animate`, so a timeline track wins over an animator track on the same
//! channel. A transform track is relative to the pose of its object when the timeline
//! first started. A timeline that does not loop sets `playing` to false at its end.

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::query::With;
use bevy_ecs::system::Query;
use bevy_ecs::world::World;
use maki_assets::clip::{KeyMode, Track, clip_time};
use maki_core::{App, Fields, Interpolated, ResetPlay, Transform};
use serde::{Deserialize, Serialize};

use crate::animation::{Target, write_tracks};
use crate::{ObjectId, RoomProblem, RoomValidators, add_object_component};

/// Tracks of keys on objects of the room.
#[derive(Component, Debug, Clone, PartialEq, Default, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
#[fields(default)]
// The run state is apart: the live form inserts the whole `timeline` again.
#[require(TimelinePlayback)]
pub struct Timeline {
    /// How a track with no mode of its own goes from one key to the next.
    #[serde(skip_serializing_if = "is_default")]
    pub mode: KeyMode,
    /// Frames in a second: the time is cut to its frames. None: no cut.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[field(some = 12.0)]
    pub fps: Option<f32>,
    /// Seconds. 0: the time of the last key.
    #[serde(skip_serializing_if = "is_default")]
    #[field(range = 0.0..=3600.0)]
    pub length: f32,
    #[serde(skip_serializing_if = "is_default")]
    pub looping: bool,
    /// True in the file: plays from the room load. True from false: plays from time 0.
    /// False: stopped; the end of a timeline that does not loop sets it.
    #[serde(skip_serializing_if = "is_default")]
    pub playing: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tracks: Vec<TimelineTrack>,
}

/// A track of a clip on an object of the room.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
#[fields(default)]
pub struct TimelineTrack {
    /// 0: the object of the timeline.
    #[serde(skip_serializing_if = "is_default")]
    pub object: ObjectId,
    pub track: Track,
}

impl Timeline {
    /// The length in seconds: `length`, or with 0 the time of the last key.
    pub fn duration(&self) -> f32 {
        if self.length > 0.0 {
            return self.length;
        }
        let keys = self.tracks.iter().flat_map(|track| &track.track.keys);
        keys.map(|key| key.time).fold(0.0, f32::max)
    }
}

/// The run state of a [`Timeline`]. `ResetPlay` gives it back.
#[derive(Component, Debug, Default, Clone)]
pub struct TimelinePlayback {
    /// The time in fixed steps: a count, as a sum of float seconds drifts.
    steps: u64,
    /// False: the next play starts at time 0.
    running: bool,
    /// The tracks that played last: other tracks are a cut.
    tracks: Vec<TimelineTrack>,
    /// The entity and the pose of each target when the timeline first started.
    rests: Vec<(ObjectId, Entity, Transform)>,
    /// For each target: the tracks that warned of a write that failed.
    warned: Vec<(ObjectId, Vec<usize>)>,
    /// The objects of tracks that are not in the room: one warning each.
    missing: Vec<ObjectId>,
}

/// Registers the component `timeline`, its reset, and its room check.
pub(crate) fn add(app: &mut App) {
    add_object_component::<Timeline>(app, "timeline");
    app.add_systems(ResetPlay, reset_timelines)
        .init_resource::<RoomValidators>();
    app.resource_mut::<RoomValidators>()
        .0
        .push(Box::new(|check| {
            let mut problems = Vec::new();
            for (index, object) in check.room.objects.iter().enumerate() {
                let Some(Ok(timeline)) = object.component::<Timeline>("timeline") else {
                    continue;
                };
                let name = object.name.as_deref().unwrap_or("");
                for track in &timeline.tracks {
                    if track.object.0 != 0 && check.room.find(&track.object).is_none() {
                        problems.push(RoomProblem {
                            warning: true,
                            text: format!(
                                " object {index} {name}: a timeline track on {}, which is not in the room",
                                track.object
                            ),
                        });
                    }
                }
            }
            problems
        }));
}

/// Starts a timeline from time 0. False: the entity has no timeline.
pub fn play_timeline(world: &mut World, entity: Entity) -> bool {
    let Some(mut timeline) = world.get_mut::<Timeline>(entity) else {
        return false;
    };
    timeline.playing = true;
    world
        .entity_mut(entity)
        .get_mut::<TimelinePlayback>()
        .expect("required")
        .running = false;
    true
}

/// Stops a timeline. `end`: first writes the values of the end, each a jump (a skip).
/// False: the entity has no timeline.
pub fn stop_timeline(world: &mut World, entity: Entity, end: bool) -> bool {
    let Some(timeline) = world.get::<Timeline>(entity).cloned() else {
        return false;
    };
    if end {
        let mut playback = take_playback(world, entity);
        let duration = timeline.duration();
        write_at(world, entity, &timeline, &mut playback, duration, true);
        playback.running = false;
        put_playback(world, entity, playback);
    }
    let mut timeline = world.get_mut::<Timeline>(entity).expect("read above");
    if timeline.playing {
        timeline.playing = false;
    }
    true
}

/// One fixed step of each timeline. Called at the end of `animate`.
pub(crate) fn play(world: &mut World, dt: f64) {
    let mut query = world.query::<(Entity, &Timeline)>();
    let list: Vec<(Entity, Timeline)> = query
        .iter(world)
        .map(|(entity, timeline)| (entity, timeline.clone()))
        .collect();
    for (entity, timeline) in list {
        if !timeline.playing {
            // Only a change of state is a write.
            let mut playback = world.get_mut::<TimelinePlayback>(entity).expect("required");
            if playback.running {
                playback.running = false;
            }
            continue;
        }
        let mut playback = take_playback(world, entity);
        let cut = !playback.running || playback.tracks != timeline.tracks;
        if cut {
            if !playback.running {
                playback.steps = 0;
            }
            playback.running = true;
            playback.tracks = timeline.tracks.clone();
            playback.warned.clear();
        }
        let time = (playback.steps as f64 * dt) as f32;
        let duration = timeline.duration();
        let t = clip_time(time, duration, timeline.looping, timeline.fps);
        playback.steps += 1;
        write_at(world, entity, &timeline, &mut playback, t, cut);
        put_playback(world, entity, playback);
        if !timeline.looping && time >= duration {
            world.get_mut::<Timeline>(entity).expect("required").playing = false;
        }
    }
}

/// Writes the tracks of each target at the time `t`.
fn write_at(
    world: &mut World,
    owner: Entity,
    timeline: &Timeline,
    playback: &mut TimelinePlayback,
    t: f32,
    cut: bool,
) {
    let mut objects: Vec<ObjectId> = Vec::new();
    for track in &timeline.tracks {
        if !objects.contains(&track.object) {
            objects.push(track.object);
        }
    }
    for object in objects {
        let found = playback.rests.iter().find(|(id, _, _)| *id == object);
        let (entity, rest) = match found {
            Some(&(_, entity, rest)) => (entity, rest),
            None => {
                let Some(entity) = target(world, owner, object) else {
                    if !playback.missing.contains(&object) {
                        playback.missing.push(object);
                        tracing::warn!(?owner, %object, "a timeline track names no object of the room");
                    }
                    continue;
                };
                let pose = world.get::<Interpolated>(entity).map(|pose| pose.current);
                let rest = pose
                    .or_else(|| world.get::<Transform>(entity).copied())
                    .unwrap_or(Transform::IDENTITY);
                playback.rests.push((object, entity, rest));
                (entity, rest)
            }
        };
        let tracks: Vec<Track> = timeline
            .tracks
            .iter()
            .filter(|track| track.object == object)
            .map(|track| track.track.clone())
            .collect();
        let index = match playback.warned.iter().position(|(id, _)| *id == object) {
            Some(index) => index,
            None => {
                playback.warned.push((object, Vec::new()));
                playback.warned.len() - 1
            }
        };
        let target = Target {
            entity,
            rest,
            cut,
            fps: timeline.fps.is_some(),
        };
        let warned = &mut playback.warned[index].1;
        write_tracks(world, &target, &tracks, t, timeline.mode, warned);
    }
}

/// The entity of an object of the room: 0 is the object of the timeline. As the room
/// loader makes it: the entity of the object has its `ObjectId` and an `Interpolated`.
fn target(world: &mut World, owner: Entity, object: ObjectId) -> Option<Entity> {
    if object.0 == 0 {
        return Some(owner);
    }
    let mut query = world.query_filtered::<(Entity, &ObjectId), With<Interpolated>>();
    let found = query.iter(world).find(|(_, id)| **id == object);
    found.map(|(entity, _)| entity)
}

/// The playback out of the entity while the tracks write the world.
fn take_playback(world: &mut World, entity: Entity) -> TimelinePlayback {
    let mut playback = world.get_mut::<TimelinePlayback>(entity).expect("required");
    std::mem::take(&mut *playback)
}

fn put_playback(world: &mut World, entity: Entity, playback: TimelinePlayback) {
    if let Some(mut slot) = world.get_mut::<TimelinePlayback>(entity) {
        *slot = playback;
    }
}

/// Runs in `ResetPlay`: each target goes back to its pose at the start, and the time
/// to 0.
fn reset_timelines(
    mut timelines: Query<&mut TimelinePlayback>,
    mut poses: Query<&mut Interpolated>,
) {
    for mut playback in &mut timelines {
        for &(_, entity, rest) in &playback.rests {
            if let Ok(mut pose) = poses.get_mut(entity)
                && *pose != Interpolated::at(rest)
            {
                pose.snap(rest);
            }
        }
        *playback = TimelinePlayback::default();
    }
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}
