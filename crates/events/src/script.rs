//! Scripts: components of a project with hooks.
//!
//! A script is a component of `bevy_ecs` with fields (`#[derive(Fields)]`) and the
//! trait [`Script`]. [`add_script`] registers it under the name that the room files
//! give it, and adds one system for its type. The system calls the hooks of each
//! entity with the component:
//!
//! | Hook | When |
//! |---|---|
//! | `start` | The first step of the entity: after the room loads, and at "Play" in a tool |
//! | `step` | Each fixed step |
//! | `interact` | The player faces the area of the object and presses interact, and no event took the press |
//! | `enter_area`, `leave_area` | The player walks into the area of the object, and out |
//!
//! The area of an object is its component `area` ([`TriggerArea`]). The hooks
//! `interact` and `enter_area` are the triggers `Interact` and `Touch` of the events,
//! with the same reach and the same rules: none in the first step, none while an
//! event blocks the player.

use std::marker::PhantomData;

use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::component::{Component, Mutable};
use bevy_ecs::entity::Entity;
use bevy_ecs::query::With;
use bevy_ecs::resource::Resource;
use bevy_ecs::world::{Mut, World};
use glam::Vec3;
use maki_core::{App, Fields, Interpolated, Time, Transform};
use maki_platform::{Action, FixedActionState};
use maki_scene::{Name, ObjectId, add_object_component};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::command::{Command, CommandSet, ScriptFunction};
use crate::interpreter::{RunningEvent, Start, faces};
use crate::model::{Area, Trigger};
use crate::{CommandRegistry, EventRunner, GameState};

/// The hooks of a script. Each has an empty default: a script writes the ones it
/// needs.
///
/// While a hook runs, the value is out of its entity (`Default` takes its place), so
/// the hook has `&mut self` and the whole world beside it.
pub trait Script:
    Component<Mutability = Mutable> + Default + Fields + Serialize + DeserializeOwned
{
    fn start(&mut self, _context: &mut ScriptContext) {}
    fn step(&mut self, _context: &mut ScriptContext) {}
    fn interact(&mut self, _context: &mut ScriptContext) {}
    fn enter_area(&mut self, _context: &mut ScriptContext) {}
    fn leave_area(&mut self, _context: &mut ScriptContext) {}
}

/// What a hook gets: the world, and the entity of its object.
pub struct ScriptContext<'w> {
    pub world: &'w mut World,
    pub entity: Entity,
    /// `Some`: a row `CallScript` called the function. The runner is out of the world
    /// then, so [`Self::run`] pushes here and the row runs the list after the call.
    queue: Option<Vec<Command>>,
}

impl<'w> ScriptContext<'w> {
    /// The context of a hook.
    pub(crate) fn new(world: &'w mut World, entity: Entity) -> Self {
        Self {
            world,
            entity,
            queue: None,
        }
    }

    /// Calls a function for a row `CallScript`: the commands that it ran, in order.
    pub(crate) fn call_function(
        function: ScriptFunction,
        world: &'w mut World,
        entity: Entity,
    ) -> Vec<Command> {
        let mut context = Self {
            world,
            entity,
            queue: Some(Vec::new()),
        };
        function(&mut context);
        context.queue.unwrap_or_default()
    }

    /// The pose of the object at the end of the last step.
    pub fn pose(&self) -> Transform {
        self.world
            .get::<Interpolated>(self.entity)
            .map_or(Transform::IDENTITY, |pose| pose.current)
    }

    /// Moves the object: the picture blends to the new pose.
    pub fn set_pose(&mut self, pose: Transform) {
        if let Some(mut old) = self.world.get_mut::<Interpolated>(self.entity) {
            old.current = pose;
        }
    }

    /// The length of a step, in seconds.
    pub fn seconds(&self) -> f32 {
        self.world.resource::<Time>().fixed_delta.as_secs_f32()
    }

    /// True while the action is held.
    pub fn held(&self, action: Action) -> bool {
        self.world.resource::<FixedActionState>().pressed(action)
    }

    /// True in the step after the action started.
    pub fn pressed(&self, action: Action) -> bool {
        self.world
            .resource::<FixedActionState>()
            .just_pressed(action)
    }

    /// The flags and variables of the game: the store of the events too.
    pub fn state(&mut self) -> Mut<'_, GameState> {
        self.world.resource_mut::<GameState>()
    }

    /// The name of the object. Empty: it has none.
    pub fn name(&self) -> &str {
        self.world
            .get::<Name>(self.entity)
            .map_or("", |name| name.0.as_str())
    }

    /// A component of the object: of another script, a light.
    pub fn get_mut<C: Component<Mutability = Mutable>>(&mut self) -> Option<Mut<'_, C>> {
        self.world.get_mut::<C>(self.entity)
    }

    /// A component of another object, by the id of the object. The id is in a field
    /// of the script (`ObjectId`): the editor picks the object by its
    /// name, and a new name of the object does not change the id.
    pub fn object_mut<C: Component<Mutability = Mutable>>(
        &mut self,
        id: ObjectId,
    ) -> Option<Mut<'_, C>> {
        let mut objects = self.world.query_filtered::<(Entity, &ObjectId), With<C>>();
        let entity = objects
            .iter(self.world)
            .find_map(|(entity, other)| (*other == id).then_some(entity))?;
        self.world.get_mut::<C>(entity)
    }

    /// A component of another object, by the name of the object: the first entity
    /// with the name and the component.
    pub fn named_mut<C: Component<Mutability = Mutable>>(
        &mut self,
        name: &str,
    ) -> Option<Mut<'_, C>> {
        let mut named = self.world.query_filtered::<(Entity, &Name), With<C>>();
        let entity = named
            .iter(self.world)
            .find_map(|(entity, other)| (other.0 == name).then_some(entity))?;
        self.world.get_mut::<C>(entity)
    }

    /// Sends an output of the object (named with `add_outputs`): its wires start in
    /// the step of the interpreter after this one.
    pub fn signal(&mut self, output: &str) {
        crate::interpreter::Signals::send(self.world, self.entity, output);
    }

    /// A command of the registry (`maki_events::Builtin`, or the commands of any
    /// module) from a value of its enum: for a list, or a branch of a `Choice`.
    /// `None`: no module added the enum; the log has an error.
    pub fn command<T: CommandSet>(&self, value: T) -> Option<Command> {
        let text = format!("{value:?}");
        let command = self.world.resource::<CommandRegistry>().command(value);
        if command.is_none() {
            tracing::error!("{}: no module added the command {text}", self.name());
        }
        command
    }

    /// Runs a command of the registry, with the same handler as a row of an event.
    /// See [`Self::run`].
    pub fn call<T: CommandSet>(&mut self, value: T) {
        if let Some(command) = self.command(value) {
            self.run(vec![command]);
        }
    }

    /// Runs a list of commands as an event runs a page. The first step runs now. A
    /// command that waits (a message, a door that turns) goes on as a running event
    /// with the name of the object: it blocks the player as an event does. In a
    /// function of a row `CallScript`, the commands run in the event of the row,
    /// before its next row.
    pub fn run(&mut self, commands: Vec<Command>) {
        if let Some(queue) = &mut self.queue {
            queue.extend(commands);
            return;
        }
        let name = self.name().to_owned();
        // `Interact`: the player started it, so it blocks the player.
        let running = RunningEvent::new(&name, Start::Page(Trigger::Interact), &commands);
        let mut running = running.of(self.world, self.entity);
        // ponytail: the step mode of a tool is not for script calls.
        if !running.step(self.world, &mut None) {
            self.world
                .resource_mut::<EventRunner>()
                .running
                .push(running);
        }
    }
}

/// The area of an object, for the hooks `interact`, `enter_area`, and `leave_area`:
/// a rectangle on the floor around the position of the object.
#[derive(Component, Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
#[fields(default)]
pub struct TriggerArea {
    /// Length along X, in meters.
    #[field(range = 0.05..=50.0, step = 0.05)]
    pub width: f32,
    /// Length along Z, in meters.
    #[field(range = 0.05..=50.0, step = 0.05)]
    pub depth: f32,
}

/// The outputs of the area: the registration, the interpreter, the check, and the
/// editor name them here.
impl TriggerArea {
    /// The player faces the area and presses interact.
    pub const USED: &'static str = "used";
    /// The player walks into the area.
    pub const ENTERED: &'static str = "entered";
    /// The player walks out of the area.
    pub const LEFT: &'static str = "left";
    pub const OUTPUTS: &'static [&'static str] = &[Self::USED, Self::ENTERED, Self::LEFT];
}

impl Default for TriggerArea {
    fn default() -> Self {
        Self {
            width: 1.0,
            depth: 1.0,
        }
    }
}

/// What the system of a script type keeps of each entity.
#[derive(Component)]
struct Hooked<T: Send + Sync + 'static> {
    /// The player was in the area in the step before.
    inside: bool,
    script: PhantomData<fn() -> T>,
}

/// The function of each script type that runs its hooks. `run_events` calls them
/// after its own step: a system for each type would need an order against each
/// system of `FixedUpdate`, as `run_events` has.
#[derive(Resource, Default)]
pub(crate) struct ScriptRunners(pub(crate) Vec<RunScripts>);

/// Runs the hooks of the entities of one type. The second argument: the position and
/// the view direction of the player.
type RunScripts = fn(&mut World, Option<(Vec3, Vec3)>);

/// Names the outputs of a component type: what a wire of an object with the
/// component can join, and what a row `Signal` can send. Needs `EventsModule`.
pub fn add_outputs(app: &mut App, component: &'static str, outputs: &'static [&'static str]) {
    app.world()
        .resource::<CommandRegistry>()
        .add_outputs(component, outputs);
}

/// Registers a function of the script crate that the row `CallScript` calls by its
/// name, with the object of the event. Needs `EventsModule`.
pub fn add_script_function(app: &mut App, name: &'static str, function: ScriptFunction) {
    app.world()
        .resource::<CommandRegistry>()
        .add_function(name, function);
}

/// Registers a script type under the name that the room files give it. Needs
/// `EventsModule`.
pub fn add_script<T: Script>(app: &mut App, name: &'static str) {
    add_object_component::<T>(app, name);
    app.resource_mut::<ScriptRunners>().0.push(run_scripts::<T>);
}

/// The hooks of each entity of a type. `player`: position and view direction.
fn run_scripts<T: Script>(world: &mut World, player: Option<(Vec3, Vec3)>) {
    let mut scripts = world.query_filtered::<(Entity, Option<&Hooked<T>>), With<T>>();
    let entities: Vec<(Entity, Option<bool>)> = scripts
        .iter(world)
        .map(|(entity, hooked)| (entity, hooked.map(|hooked| hooked.inside)))
        .collect();
    for (entity, before) in entities {
        let area = world.get::<TriggerArea>(entity).map(|area| {
            let center = world
                .get::<Interpolated>(entity)
                .map_or(Vec3::ZERO, |pose| pose.current.translation);
            Area {
                center: [center.x, center.z],
                size: [area.width, area.depth],
            }
        });
        let inside =
            area.is_some_and(|area| player.is_some_and(|(at, _)| area.distance(at.x, at.z) == 0.0));
        // A hook before could have despawned the entity.
        let Some(mut value) = world.get_mut::<T>(entity) else {
            continue;
        };
        let mut script = std::mem::take(&mut *value);
        let mut context = ScriptContext::new(world, entity);
        match before {
            None => script.start(&mut context),
            Some(before) => {
                let blocked = context.world.resource::<EventRunner>().blocks_player();
                if inside && !before && !blocked {
                    script.enter_area(&mut context);
                }
                if before && !inside {
                    script.leave_area(&mut context);
                }
            }
        }
        let runner = context.world.resource::<EventRunner>();
        let faced = area
            .zip(player)
            .is_some_and(|(area, player)| faces(player, area));
        // Not in the first step, as the events.
        if before.is_some() && faced && runner.interact_free && !runner.blocks_player() {
            // One press starts one thing.
            let mut runner = context.world.resource_mut::<EventRunner>();
            runner.bypass_change_detection().interact_free = false;
            script.interact(&mut context);
        }
        script.step(&mut context);

        let Ok(mut entity) = world.get_entity_mut(entity) else {
            continue;
        };
        if let Some(mut value) = entity.get_mut::<T>() {
            *value = script;
        }
        match entity.get_mut::<Hooked<T>>() {
            Some(mut hooked) => {
                if hooked.inside != inside {
                    hooked.inside = inside;
                }
            }
            None => {
                entity.insert(Hooked::<T> {
                    inside,
                    script: PhantomData,
                });
            }
        }
    }
}
