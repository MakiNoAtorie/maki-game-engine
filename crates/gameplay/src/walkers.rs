//! Characters of a room that walk by a path search: the component `walker`
//! ([`WalkerDef`]), the path that a walker follows ([`PathFollow`]), and the command
//! `WalkTo`.
//!
//! A script or a command asks a walk with a [`WalkGoal`] on the object (`maki-events`
//! is below this crate, so it names the goal, not the walker). [`steer_walkers`] serves
//! it: a path on the [`WalkGrid`], then the `Walker` steered from point to point. The
//! walk itself is the `walk` of the player: the same character controller.

use bevy_ecs::change_detection::DetectChanges;
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::query::{Changed, With};
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Commands, Query, Res};
use bevy_ecs::world::{Mut, World};
use glam::Vec3;
use maki_core::{App, Fields, FixedUpdate, Interpolated, Module, Time};
use maki_events::{CommandRegistry, Finding, Step, WalkGoal, object_entity, run_events};
use maki_physics::Physics;
use maki_scene::{Collider, CurrentRoom, ObjectId, RoomDef, add_object_component};
use serde::{Deserialize, Serialize};

use crate::grid::WalkGrid;
use crate::player::{PlayerDef, Walker, fit_walker_bodies, steer_player};

/// The component `walker` of an object: the object walks by a path search. The step
/// and the slope are those of the player prefab.
// The walk state comes with the walker: an insert in play changes the order of the lights.
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
#[require(WalkGoal, PathFollow)]
#[serde(default, deny_unknown_fields)]
#[fields(default)]
pub struct WalkerDef {
    /// The walker is an upright capsule of this radius for collisions, in meters.
    #[field(range = 0.05..=2.0, step = 0.01)]
    pub radius: f32,
    /// The full height of the capsule, in meters.
    #[field(range = 0.1..=5.0, step = 0.05)]
    pub height: f32,
    /// Meters per second.
    #[field(range = 0.0..=10.0, step = 0.05)]
    pub speed: f32,
}

impl Default for WalkerDef {
    fn default() -> Self {
        Self {
            radius: 0.3,
            height: 1.7,
            speed: 1.6,
        }
    }
}

/// The path that a walker follows: not in the room file. Empty: no path.
#[derive(Component, Debug, Clone, PartialEq, Default)]
pub struct PathFollow {
    /// The points of the path: cell centers, the last one the place.
    pub points: Vec<Vec3>,
    /// The index of the point that the walker goes to.
    pub next: usize,
    /// The place of the walker in the step before.
    last: Vec3,
    /// Seconds with no progress.
    stalled: f32,
}

/// The event commands of [`WalkersModule`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
pub enum WalkCommand {
    /// Walks a walker (0: the object of the event) to a target by a path search, and
    /// waits until it is there. No path: a warning, and the next row.
    WalkTo(
        #[field(label = "object")] ObjectId,
        #[field(label = "target")] WalkTarget,
    ),
}

/// Where `WalkTo` goes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
pub enum WalkTarget {
    /// A place in the room.
    #[field(label = "place")]
    Place([f32; 3]),
    /// The place of an object at the start of the walk (0: the object of the event).
    #[field(label = "object")]
    Object(ObjectId),
}

impl Default for WalkTarget {
    fn default() -> Self {
        Self::Place([0.0; 3])
    }
}

/// The component `walker`, the path follow, and the command `WalkTo`. Needs
/// `PlayerModule` (the walk), `GridModule`, `maki_events::EventsModule`, and
/// `maki_physics::PhysicsModule`.
pub struct WalkersModule;

impl Module for WalkersModule {
    fn build(&self, app: &mut App) {
        add_object_component::<WalkerDef>(app, "walker");
        let registry = app.world().resource::<CommandRegistry>();
        add_commands(registry);
        app.add_systems(
            FixedUpdate,
            (add_walkers, steer_walkers)
                .chain()
                .after(run_events)
                .after(steer_player)
                .before(fit_walker_bodies),
        );
    }
}

/// A walker with less progress than this part of its speed in a step makes no
/// progress.
const PROGRESS: f32 = 0.25;

/// After this many seconds with no progress (a door closed in its way, another walker),
/// a walker stops and asks a new path.
const STALL_SECONDS: f32 = 0.5;

/// Runs in `FixedUpdate`: gives each object with a new or changed `walker` its
/// [`Walker`], with the step and the slope of the player.
fn add_walkers(
    mut commands: Commands,
    players: Query<&PlayerDef>,
    mut defs: Query<(Entity, &WalkerDef, Option<&mut Walker>), Changed<WalkerDef>>,
) {
    let player = players.iter().next().cloned().unwrap_or_default();
    for (entity, def, walker) in &mut defs {
        let new = Walker {
            radius: def.radius,
            height: def.height,
            step_height: player.step_height,
            max_slope: player.max_slope,
            push_force: player.push_force,
            direction: walker
                .as_ref()
                .map_or(Vec3::ZERO, |walker| walker.direction),
            speed: def.speed,
        };
        match walker {
            Some(mut walker) => *walker = new,
            // The box of a prop stays where it was: the capsule of the walker takes its place.
            None => drop(commands.entity(entity).insert(new).remove::<Collider>()),
        }
    }
}

/// Runs in `FixedUpdate` before the walk: serves the [`WalkGoal`] of each walker. An
/// asked goal gets a path (or `NoPath` and a warning); a walker on a path goes to its
/// next point, and at the last one it has `Arrived`. A walker with no progress for
/// [`STALL_SECONDS`] asks a new path. `Idle` (a script stopped it): it stands.
pub fn steer_walkers(
    time: Res<Time>,
    room: Res<CurrentRoom>,
    grid: Res<WalkGrid>,
    physics: Res<Physics>,
    mut walkers: Query<
        (
            Entity,
            &mut Walker,
            &Interpolated,
            &mut WalkGoal,
            &mut PathFollow,
        ),
        With<WalkerDef>,
    >,
) {
    let seconds = time.fixed_delta.as_secs_f32();
    for (entity, mut walker, pose, mut goal, mut follow) in &mut walkers {
        let at = pose.current.translation;
        let target = match *goal {
            WalkGoal::Walking(target) => target,
            WalkGoal::Asked(target) => {
                stand(&mut walker);
                // A new room: `make_grid` makes its grid after the physics step.
                if room.is_changed() {
                    continue;
                }
                match grid.find_path(&physics, at, target) {
                    Some(points) => {
                        *goal = WalkGoal::Walking(target);
                        *follow = PathFollow {
                            points,
                            next: 0,
                            last: at,
                            stalled: 0.0,
                        };
                    }
                    None => {
                        tracing::warn!(?entity, %target, "WalkTo: no path to the place");
                        *goal = WalkGoal::NoPath;
                        clear(&mut follow);
                    }
                }
                continue;
            }
            WalkGoal::Idle | WalkGoal::Arrived(_) | WalkGoal::NoPath => {
                stand(&mut walker);
                clear(&mut follow);
                continue;
            }
        };

        let moved = flat(at - follow.last).length();
        follow.stalled = if moved < walker.speed * seconds * PROGRESS {
            follow.stalled + seconds
        } else {
            0.0
        };
        follow.last = at;
        if follow.stalled > STALL_SECONDS {
            *goal = WalkGoal::Asked(target);
            stand(&mut walker);
            continue;
        }
        // Reach of one step: a smaller one lets the walker step over a point and back.
        let reach = (walker.speed * seconds).max(0.01);
        while let Some(point) = follow.points.get(follow.next)
            && flat(*point - at).length() <= reach
        {
            follow.next += 1;
        }
        match follow.points.get(follow.next) {
            Some(point) => {
                let direction = flat(*point - at).normalize_or_zero();
                if walker.direction != direction {
                    walker.direction = direction;
                }
            }
            None => {
                *goal = WalkGoal::Arrived(target);
                stand(&mut walker);
                clear(&mut follow);
            }
        }
    }
}

/// The walker has no path. Only a change is written.
fn clear(follow: &mut Mut<PathFollow>) {
    if !follow.points.is_empty() {
        **follow = PathFollow::default();
    }
}

/// The walker stands. Only a change is written: `fit_walker_bodies` reads changes.
fn stand(walker: &mut Mut<Walker>) {
    if walker.direction != Vec3::ZERO {
        walker.direction = Vec3::ZERO;
    }
}

/// A vector on the floor plane.
fn flat(vector: Vec3) -> Vec3 {
    Vec3::new(vector.x, 0.0, vector.z)
}

fn add_commands(registry: &CommandRegistry) {
    registry.add(run);
    registry.add_fields::<WalkCommand>();
    registry.add_texts(
        |command: &WalkCommand| {
            let WalkCommand::WalkTo(object, target) = command;
            Some(match target {
                WalkTarget::Place([x, y, z]) => format!("Walk {object} to ({x}, {y}, {z})"),
                WalkTarget::Object(other) => format!("Walk {object} to {other}"),
            })
        },
        |name| {
            (name == "WalkTo").then_some(
                "Walks a walker around the colliders to a place or an object, and waits.",
            )
        },
    );
    registry.add_defaults(new_command);
    registry.add_check(|command: &WalkCommand, context| {
        let WalkCommand::WalkTo(object, target) = command;
        let mut objects = vec![*object];
        if let WalkTarget::Object(other) = target {
            objects.push(*other);
        }
        let missing = objects
            .into_iter()
            .find(|object| *object != ObjectId::default() && context.room.find(object).is_none());
        missing.map(|object| Finding::error(format!("the room has no object `{object}`")))
    });
}

/// The new row walks the first walker of the room to its own place.
fn new_command(name: &str, room: &RoomDef) -> Option<WalkCommand> {
    if name != "WalkTo" {
        return None;
    }
    let walker = |object: &&maki_scene::ObjectDef| object.components.contains_key("walker");
    let object = room.objects.iter().find(walker);
    let id = object.and_then(|object| object.id).unwrap_or_default();
    let place = object.map_or([0.0; 3], |object| object.pose.position);
    Some(WalkCommand::WalkTo(id, WalkTarget::Place(place)))
}

/// One step of `WalkTo`. The first asks the walk; the next ones wait while it goes on.
fn run(command: &WalkCommand, step: &mut Step, world: &mut World) -> bool {
    let WalkCommand::WalkTo(object, target) = command;
    let Some(entity) = entity_of(world, step, object, true) else {
        tracing::warn!(event = step.event, %object, "WalkTo: no walker");
        return true;
    };
    if step.start {
        let place = match target {
            WalkTarget::Place(place) => Some(Vec3::from(*place)),
            WalkTarget::Object(other) => entity_of(world, step, other, false)
                .and_then(|other| world.get::<Interpolated>(other))
                .map(|pose| pose.current.translation),
        };
        let Some(place) = place else {
            tracing::warn!(event = step.event, ?target, "WalkTo: no such object");
            return true;
        };
        if let Some(mut goal) = world.get_mut::<WalkGoal>(entity) {
            *goal = WalkGoal::Asked(place);
        }
        return false;
    }
    !matches!(
        world.get::<WalkGoal>(entity),
        Some(WalkGoal::Asked(_) | WalkGoal::Walking(_))
    )
}

/// The entity of an object (0: of the event) with a pose: with `walker`, only the one
/// that walks.
fn entity_of(world: &World, step: &Step, object: &ObjectId, walker: bool) -> Option<Entity> {
    let entity = if *object == ObjectId::default() {
        step.entity
            .filter(|entity| world.get::<Interpolated>(*entity).is_some())
    } else {
        object_entity(world, *object)
    }?;
    (!walker || world.get::<WalkerDef>(entity).is_some()).then_some(entity)
}
