//! Object state: named switches and numbers of one object, as the flags and variables
//! of the game are of all. The component `state` gives the names and their first
//! values; the values live in [`ObjectStates`], by the room file and the id of the
//! object, so they stay over a change of room and go into the save.

use std::collections::BTreeMap;
use std::path::Path;

use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::component::Component;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::schedule::common_conditions::resource_changed;
use bevy_ecs::system::{Query, Res, ResMut};
use bevy_ecs::world::World;
use maki_core::{App, Fields, ResetPlay, Update};
use maki_scene::{
    CurrentRoom, ObjectId, RoomDef, RoomProblem, RoomValidators, load_requested_room,
};
use serde::{Deserialize, Serialize};

use crate::CommandRegistry;
use crate::check::{CheckContext, Finding};
use crate::command::Step;

/// The component `state`: the switches and numbers of the object, with their first
/// values. An object needs an id to have state.
#[derive(Component, Debug, Clone, PartialEq, Default, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
pub struct ObjectState {
    /// Yes-or-no values: a chest is open.
    pub switches: Vec<NamedSwitch>,
    /// Whole numbers: the coins in a chest.
    pub numbers: Vec<NamedNumber>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
pub struct NamedSwitch {
    pub name: String,
    /// The first value.
    pub on: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
pub struct NamedNumber {
    pub name: String,
    /// The first value.
    pub value: i32,
}

/// The values of the state of one object. A name that was never set is a switch
/// that is off and a number of 0.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StateValues {
    pub switches: BTreeMap<String, bool>,
    pub numbers: BTreeMap<String, i32>,
}

impl StateValues {
    pub fn switch(&self, name: &str) -> bool {
        self.switches.get(name).copied().unwrap_or(false)
    }

    pub fn number(&self, name: &str) -> i32 {
        self.numbers.get(name).copied().unwrap_or(0)
    }
}

/// The state of the objects of one room, by id.
pub type RoomStates = BTreeMap<ObjectId, StateValues>;

/// The state of the objects of each room that was loaded, by the room file
/// ([`room_key`]). A resource: it stays when the room changes, and the save has it.
// ponytail: keyed by the room path; a rename loses the state. Use the file id when needed.
#[derive(Resource, Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ObjectStates(pub BTreeMap<String, RoomStates>);

impl ObjectStates {
    pub fn get(&self, room: &str, object: ObjectId) -> Option<&StateValues> {
        self.0.get(room)?.get(&object)
    }

    pub fn get_mut(&mut self, room: &str, object: ObjectId) -> &mut StateValues {
        let room = self.0.entry(room.to_owned()).or_default();
        room.entry(object).or_default()
    }
}

/// The key of a room file in [`ObjectStates`]: its path with forward slashes, as
/// the save file names it.
pub fn room_key(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// The key of the current room.
pub(crate) fn current_room(world: &World) -> Option<String> {
    let room = world.get_resource::<CurrentRoom>()?;
    Some(room_key(&room.path))
}

/// The commands that set the state of an object. Object 0: the object of the event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
pub enum StateCommand {
    SetSwitch(
        #[field(label = "object")] ObjectId,
        #[field(label = "switch")] String,
        #[field(label = "on")] bool,
    ),
    SetNumber(
        #[field(label = "object")] ObjectId,
        #[field(label = "number")] String,
        #[field(label = "value")] i32,
    ),
    AddNumber(
        #[field(label = "object")] ObjectId,
        #[field(label = "number")] String,
        #[field(label = "add")] i32,
    ),
}

pub(crate) fn add(app: &mut App, registry: &CommandRegistry) {
    maki_scene::add_object_component::<ObjectState>(app, "state");
    app.init_resource::<ObjectStates>()
        .add_systems(
            Update,
            first_values
                .run_if(resource_changed::<CurrentRoom>)
                .after(load_requested_room),
        )
        // A play starts with no values: the next room load gives the first values.
        .add_systems(ResetPlay, |mut states: ResMut<ObjectStates>| {
            states.set_if_neq(ObjectStates::default());
        });
    app.resource_mut::<RoomValidators>()
        .0
        .push(Box::new(|check| no_id(check.room)));
    registry.add(run);
    registry.add_texts(row_text, help);
    registry.add_fields::<StateCommand>();
    registry.add_defaults(|name, room| {
        let first = room.objects.iter().find(|object| object.id.is_some());
        let id = first.and_then(|object| object.id).unwrap_or_default();
        Some(match name {
            "SetSwitch" => StateCommand::SetSwitch(id, String::new(), true),
            "SetNumber" => StateCommand::SetNumber(id, String::new(), 0),
            "AddNumber" => StateCommand::AddNumber(id, String::new(), 1),
            _ => return None,
        })
    });
    registry.add_check(|command: &StateCommand, context| {
        let object = target(command);
        // Object 0 is the object of the event: the check does not know it.
        if *object == ObjectId::default() {
            return None;
        }
        if context.room.find(object).is_none() {
            return Some(Finding::error(format!("the room has no object `{object}`")));
        }
        let has = context.reaches(object, &["state"]);
        (!has).then(|| Finding::error(format!("`{object}` has no component `state`")))
    });
}

/// The problem of a `state` component on an object with no id: no row can name it,
/// and its values have no key.
fn no_id(room: &RoomDef) -> Vec<RoomProblem> {
    let without = room.objects.iter().enumerate();
    let without = without
        .filter(|(_, object)| object.id.is_none() && object.components.contains_key("state"));
    let problem = |(index, object): (usize, &maki_scene::ObjectDef)| RoomProblem {
        warning: false,
        text: format!(
            "object {index} {}: a `state` component needs an object with an id",
            object.name.as_deref().unwrap_or("")
        ),
    };
    without.map(problem).collect()
}

/// Runs after a room load: each switch and number of a `state` component that has
/// no value yet gets its first value. A value that an event set stays.
fn first_values(
    room: Res<CurrentRoom>,
    mut states: ResMut<ObjectStates>,
    objects: Query<(&ObjectId, &ObjectState)>,
) {
    let key = room_key(&room.path);
    for (id, state) in &objects {
        let values = states.get_mut(&key, *id);
        for switch in &state.switches {
            let value = values.switches.entry(switch.name.clone());
            value.or_insert(switch.on);
        }
        for number in &state.numbers {
            let value = values.numbers.entry(number.name.clone());
            value.or_insert(number.value);
        }
    }
}

/// What is wrong with a condition of object state: no object, or no `state` on it.
/// `this`: the id of the object of the event.
pub(crate) fn condition_problem(
    condition: &crate::Condition,
    this: Option<ObjectId>,
    context: &CheckContext,
) -> Option<String> {
    use crate::Condition;
    let object = match condition {
        Condition::Any(conditions) => {
            let mut problems = conditions.iter();
            return problems.find_map(|condition| condition_problem(condition, this, context));
        }
        Condition::Switch(object, _)
        | Condition::NotSwitch(object, _)
        | Condition::Number(object, ..) => *object,
        Condition::Item(item, ..) => {
            return crate::items::item_problem(&item.file, context.directory);
        }
        _ => return None,
    };
    let Some(object) = resolve(object, this) else {
        return Some("object 0 (this): the object of the event has no id".to_owned());
    };
    if context.room.find(&object).is_none() {
        return Some(format!("the room has no object `{object}`"));
    }
    let has = context.reaches(&object, &["state"]);
    (!has).then(|| format!("`{object}` has no component `state`"))
}

fn target(command: &StateCommand) -> &ObjectId {
    let (StateCommand::SetSwitch(object, ..)
    | StateCommand::SetNumber(object, ..)
    | StateCommand::AddNumber(object, ..)) = command;
    object
}

/// The object that an argument names: object 0 is `this`, the object of the event.
pub(crate) fn resolve(object: ObjectId, this: Option<ObjectId>) -> Option<ObjectId> {
    match object == ObjectId::default() {
        true => this,
        false => Some(object),
    }
}

/// The values of an object of the current room, to write. `None`: no room, or object
/// 0 with no object of the event.
pub(crate) fn values_mut(
    world: &mut World,
    object: ObjectId,
    this: Option<ObjectId>,
) -> Option<bevy_ecs::world::Mut<'_, StateValues>> {
    let object = resolve(object, this)?;
    let room = current_room(world)?;
    let states = world.resource_mut::<ObjectStates>();
    Some(bevy_ecs::world::Mut::map_unchanged(states, |states| {
        states.get_mut(&room, object)
    }))
}

/// The values of an object of the current room, to read.
pub(crate) fn values(
    world: &World,
    object: ObjectId,
    this: Option<ObjectId>,
) -> Option<&StateValues> {
    let object = resolve(object, this)?;
    let room = current_room(world)?;
    world.get_resource::<ObjectStates>()?.get(&room, object)
}

fn run(command: &StateCommand, step: &mut Step, world: &mut World) -> bool {
    let Some(mut values) = values_mut(world, *target(command), step.object) else {
        tracing::warn!(event = step.event, "{command:?}: no object of the event");
        return true;
    };
    match command {
        StateCommand::SetSwitch(_, name, on) => {
            values.switches.insert(name.clone(), *on);
        }
        StateCommand::SetNumber(_, name, value) => {
            values.numbers.insert(name.clone(), *value);
        }
        StateCommand::AddNumber(_, name, value) => {
            let sum = values.number(name).saturating_add(*value);
            values.numbers.insert(name.clone(), sum);
        }
    }
    true
}

/// "this" for object 0, else the id.
pub(crate) fn object_text(object: &ObjectId) -> String {
    match *object == ObjectId::default() {
        true => "this".to_owned(),
        false => object.to_string(),
    }
}

fn row_text(command: &StateCommand) -> Option<String> {
    let object = object_text(target(command));
    Some(match command {
        StateCommand::SetSwitch(_, name, true) => format!("Switch {object}.{name}: on"),
        StateCommand::SetSwitch(_, name, false) => format!("Switch {object}.{name}: off"),
        StateCommand::SetNumber(_, name, value) => format!("Number {object}.{name} = {value}"),
        StateCommand::AddNumber(_, name, value) => format!("Number {object}.{name}: add {value}"),
    })
}

fn help(name: &str) -> Option<&'static str> {
    Some(match name {
        "SetSwitch" => "Sets a switch of the state of an object. Object 0: this object.",
        "SetNumber" => "Sets a number of the state of an object. Object 0: this object.",
        "AddNumber" => "Adds to a number of the state of an object. Object 0: this object.",
        _ => return None,
    })
}
