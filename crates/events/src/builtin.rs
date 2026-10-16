//! The commands of this crate. They are added to the registry as the commands of
//! any other module are: an enum and a handler.
//!
//! Each command writes a component or a resource of the scene crate or of this
//! crate. No command calls the renderer or the user interface: they read what the
//! commands wrote.

use bevy_ecs::entity::Entity;
use bevy_ecs::query::With;
use bevy_ecs::world::World;
use maki_core::Time;
use maki_core::fields::Value;
use maki_platform::FixedActionState;
use maki_scene::{
    ComponentTypes, CurrentRoom, Light, Live, Mover, MoverState, ObjectId, RoomRequest, RoomTarget,
    ScreenFade,
};
use serde::{Deserialize, Serialize};

use crate::CommandRegistry;
use crate::check::{CheckContext, Finding};
use crate::command::{Command, Step};
use crate::interpreter::{ChoiceBox, Choices};
use crate::model::{Compare, Condition, EventPages, all_hold};
use crate::script::ScriptContext;
use crate::{CHOICE_DOWN, CHOICE_UP, GameState, INTERACT, MessageBox};

/// Text arguments are names: of a flag or a variable. An object of the room file (a
/// door, a mover, a light) is its [`ObjectId`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Builtin {
    SetFlag(String, bool),
    SetVariable(String, i32),
    AddVariable(String, i32),
    /// Seconds.
    Wait(f32),
    /// The first list if all conditions hold, else the second one.
    If(Vec<Condition>, Vec<Command>, Vec<Command>),
    /// Sends a door or a mover to its pose B (a door: open) and waits until it is there.
    OpenDoor(ObjectId),
    /// To pose A (a door: closed).
    CloseDoor(ObjectId),
    LightOn(ObjectId),
    LightOff(ObjectId),
    LightIntensity(ObjectId, f32),
    /// Shows the text until the player presses interact.
    Message(String),
    /// A question: the text, and for each answer its text and the commands that run
    /// if the player takes it. `Choice("Open it?", [("Yes", [..]), ("No", [])])`.
    Choice(String, Vec<(String, Vec<Command>)>),
    /// To black, in this many seconds.
    FadeOut(f32),
    /// Back to the picture.
    FadeIn(f32),
    /// To a spawn point of another room. The text is the room file, from the
    /// directory of the current room file. The event goes on in the new room.
    Transfer(String, ObjectId),
    /// Changes a field of a component of an object: the object, the component, the
    /// field, and the value in the form of the tree of the fields (a number, `true`,
    /// a text, a list for a vector or a color).
    SetField(ObjectId, String, String, Value),
    /// Calls a function of the script crate (`add_script_function`) with the object
    /// of the event.
    CallScript(String),
    /// Sends an output of an object: the object, the output. Its wires start in the
    /// step of the interpreter after the commands that run.
    Signal(ObjectId, String),
}

/// One step of a built-in command. False: the command waits.
pub(crate) fn run(command: &Builtin, step: &mut Step, world: &mut World) -> bool {
    match command {
        Builtin::SetFlag(name, value) => world.resource_mut::<GameState>().set_flag(name, *value),
        Builtin::SetVariable(name, value) => step.set_variable(world, name, *value),
        Builtin::AddVariable(name, value) => {
            let sum = step.state(world).variable(name).saturating_add(*value);
            step.set_variable(world, name, sum);
        }
        Builtin::Wait(seconds) => {
            let passed = world.resource::<Time>().fixed_delta.as_secs_f32();
            let start = step.start;
            let left = step.memory::<f32>();
            if start {
                *left = *seconds;
            }
            *left -= passed;
            // The small number: the sum of 60 steps of 1/60 s is not exactly 1 s.
            return *left <= 1e-5;
        }
        Builtin::If(conditions, then, otherwise) => {
            let branch = if all_hold(conditions, &step.state(world)) {
                then
            } else {
                otherwise
            };
            step.open(branch.clone());
        }
        Builtin::OpenDoor(object) => return send_mover(world, object, true, step.start),
        Builtin::CloseDoor(object) => return send_mover(world, object, false, step.start),
        Builtin::LightOn(object) => set_light(world, object, |light| Light { on: true, ..*light }),
        Builtin::LightOff(object) => set_light(world, object, |light| Light {
            on: false,
            ..*light
        }),
        Builtin::LightIntensity(object, intensity) => set_light(world, object, |light| Light {
            intensity: *intensity,
            ..*light
        }),
        Builtin::Message(text) => {
            if step.start {
                tracing::debug!(event = step.event, text, "message");
                world.resource_mut::<MessageBox>().0 = Some(text.clone());
                return false;
            }
            if !world.resource::<FixedActionState>().just_pressed(INTERACT) {
                return false;
            }
            world.resource_mut::<MessageBox>().0 = None;
        }
        Builtin::Choice(question, answers) => {
            if step.start {
                tracing::debug!(event = step.event, question, "question");
                world.resource_mut::<MessageBox>().0 = Some(question.clone());
                world.resource_mut::<ChoiceBox>().0 = Some(Choices {
                    answers: answers.iter().map(|answer| answer.0.clone()).collect(),
                    selected: 0,
                });
                // Not in this step: the press that started the event, or that
                // closed a message before, is not the answer.
                return false;
            }
            let actions = world.resource::<FixedActionState>();
            let pressed = |action| actions.just_pressed(action);
            let (up, down, take) = (pressed(CHOICE_UP), pressed(CHOICE_DOWN), pressed(INTERACT));
            let mut choice = world.resource_mut::<ChoiceBox>();
            let Some(choices) = &mut choice.0 else {
                // Another module took the question away: the event goes on.
                return true;
            };
            // The mark stops at the first and at the last answer.
            let last = answers.len().saturating_sub(1);
            let selected = (choices.selected + down as usize).min(last);
            let selected = selected.saturating_sub(up as usize);
            if choices.selected != selected {
                choices.selected = selected;
            }
            if !take {
                return false;
            }
            choice.0 = None;
            world.resource_mut::<MessageBox>().0 = None;
            if let Some((answer, commands)) = answers.get(selected) {
                tracing::debug!(event = step.event, answer, "answer");
                step.open(commands.clone());
            }
        }
        Builtin::FadeOut(seconds) | Builtin::FadeIn(seconds) => {
            let passed = world.resource::<Time>().fixed_delta.as_secs_f32();
            let part = if *seconds > 0.0 {
                passed / seconds
            } else {
                1.0
            };
            let out = matches!(command, Builtin::FadeOut(_));
            let mut fade = world.resource_mut::<ScreenFade>();
            fade.0 = (fade.0 + if out { part } else { -part }).clamp(0.0, 1.0);
            return fade.0 == if out { 1.0 } else { 0.0 };
        }
        Builtin::Transfer(room, spawn) => {
            let current = world.resource::<CurrentRoom>();
            let loads = current.loads;
            if step.start {
                let directory = current.path.parent().unwrap_or(std::path::Path::new(""));
                let path = directory.join(room);
                world.resource_mut::<RoomRequest>().0 = Some(RoomTarget {
                    path,
                    spawn: Some(*spawn),
                    text: None,
                });
                // `CurrentRoom::loads` when the room was asked for.
                *step.memory::<u64>() = loads;
                return false;
            }
            if loads == *step.memory::<u64>() {
                // The loader has not run yet (two fixed steps in one frame).
                if world.resource::<RoomRequest>().0.is_some() {
                    return false;
                }
                // The loader logged why. The event goes on in the old room.
                tracing::error!(event = step.event, room, "the transfer did not happen");
            }
        }
        Builtin::SetField(object, component, field, value) => {
            set_field(world, object, [component, field], value);
        }
        Builtin::Signal(object, output) => {
            let mut events =
                world.query_filtered::<(Entity, Option<&ObjectId>), With<EventPages>>();
            let found = events.iter(world).find(|(_, id)| *id == Some(object));
            match found.map(|(entity, _)| entity) {
                Some(entity) => crate::interpreter::Signals::send(world, entity, output),
                None => {
                    tracing::warn!(event = step.event, %object, "Signal: no object with wires");
                }
            }
        }
        Builtin::CallScript(name) => {
            let function = world.resource::<CommandRegistry>().function(name);
            match (function, step.entity) {
                (Some(function), Some(entity)) => {
                    step.open(ScriptContext::call_function(function, world, entity));
                }
                (None, _) => tracing::warn!(event = step.event, name, "no such script function"),
                (_, None) => tracing::warn!(name, "a script function needs the object of an event"),
            }
        }
    }
    true
}

/// Writes `value` into the field of the component of the first entity of the object
/// that has the component. A name that the game does not have is a warning.
fn set_field(
    world: &mut World,
    object: &ObjectId,
    [component, field]: [&String; 2],
    value: &Value,
) {
    let types = world.resource::<ComponentTypes>();
    let live = types
        .get(component)
        .and_then(|component_type| component_type.live);
    let Some(Live { get, set, .. }) = live else {
        tracing::warn!(
            component,
            "SetField: the component has no value in the game"
        );
        return;
    };
    // The entities of the reference first: an instance has its id and the prefab names.
    let mut candidates = crate::objects::entities_of(world, object).into_iter();
    let target = candidates.find_map(|entity| Some((entity, get(world, entity)?)));
    let Some((entity, mut tree)) = target else {
        tracing::warn!(
            %object,
            component,
            "SetField: no such object with the component"
        );
        return;
    };
    let Some(slot) = tree.get_mut(field.as_str()) else {
        tracing::warn!(
            %object,
            component,
            field,
            "SetField: the component has no such field"
        );
        return;
    };
    // Only a new value is a write: a write is a change.
    if slot == value {
        return;
    }
    *slot = value.clone();
    match set(world, entity, tree) {
        Ok(()) => tracing::debug!(%object, component, field, %value, "SetField"),
        Err(error) => tracing::warn!(%object, component, field, "SetField: {error}"),
    }
}

/// The command lists of a built-in command (`CommandRegistry::add_lists`): the two
/// of an `If`, and one for each answer of a `Choice`.
pub(crate) fn lists(command: &Builtin) -> Vec<(String, &[Command])> {
    match command {
        Builtin::If(_, then, otherwise) => {
            vec![(String::new(), then), ("Else".to_owned(), otherwise)]
        }
        Builtin::Choice(_, answers) => {
            let answers = answers.iter();
            let list = answers.map(|answer| (format!("Answer: {}", answer.0), answer.1.as_slice()));
            list.collect()
        }
        _ => Vec::new(),
    }
}

/// A built-in command with other lists, in the order of [`lists`].
pub(crate) fn with_lists(command: &Builtin, lists: Vec<Vec<Command>>) -> Builtin {
    let mut lists = lists.into_iter();
    let mut next = || lists.next().unwrap_or_default();
    match command {
        Builtin::If(conditions, ..) => Builtin::If(conditions.clone(), next(), next()),
        Builtin::Choice(question, answers) => {
            let answers = answers.iter().map(|answer| (answer.0.clone(), next()));
            Builtin::Choice(question.clone(), answers.collect())
        }
        other => other.clone(),
    }
}

/// Conditions as words for a reader, joined by `and` or `or`.
pub(crate) fn conditions_text(conditions: &[Condition], join: &str) -> String {
    let texts: Vec<String> = conditions.iter().map(condition_text).collect();
    texts.join(join)
}

fn condition_text(condition: &Condition) -> String {
    match condition {
        Condition::Flag(name) => name.clone(),
        Condition::NotFlag(name) => format!("not {name}"),
        Condition::Variable(name, compare, value) => {
            let sign = match compare {
                Compare::Equals => "=",
                Compare::NotEquals => "!=",
                Compare::Less => "<",
                Compare::AtLeast => ">=",
            };
            format!("{name} {sign} {value}")
        }
        Condition::Any(conditions) => format!("({})", conditions_text(conditions, " or ")),
    }
}

/// A built-in command as a line for a reader (`CommandRegistry::add_texts`).
pub(crate) fn row_text(command: &Builtin) -> Option<String> {
    Some(match command {
        Builtin::SetFlag(name, true) => format!("Flag {name}: set"),
        Builtin::SetFlag(name, false) => format!("Flag {name}: not set"),
        Builtin::SetVariable(name, value) => format!("Variable {name} = {value}"),
        Builtin::AddVariable(name, value) => format!("Variable {name}: add {value}"),
        Builtin::Wait(seconds) => format!("Wait {seconds} s"),
        Builtin::If(conditions, ..) if conditions.is_empty() => "If (no condition)".to_owned(),
        Builtin::If(conditions, ..) => format!("If {}", conditions_text(conditions, " and ")),
        Builtin::OpenDoor(object) => format!("Open {object}"),
        Builtin::CloseDoor(object) => format!("Close {object}"),
        Builtin::LightOn(object) => format!("Light {object}: on"),
        Builtin::LightOff(object) => format!("Light {object}: off"),
        Builtin::LightIntensity(object, value) => format!("Light {object}: intensity {value}"),
        Builtin::Message(text) => format!("Message: {text}"),
        Builtin::Choice(question, _) => format!("Question: {question}"),
        Builtin::FadeOut(seconds) => format!("Fade to black, {seconds} s"),
        Builtin::FadeIn(seconds) => format!("Fade to the picture, {seconds} s"),
        Builtin::Transfer(room, spawn) => format!("Go to {room}, at {spawn}"),
        Builtin::SetField(object, component, field, value) => {
            format!("Set {object}: {component}.{field} = {value}")
        }
        Builtin::CallScript(name) => format!("Call the script function {name}"),
        Builtin::Signal(object, output) => format!("Signal {output} of {object}"),
    })
}

/// One line of help for the name of a built-in command.
pub(crate) fn help(name: &str) -> Option<&'static str> {
    Some(match name {
        "SetFlag" => "Sets a flag, or sets it back.",
        "SetVariable" => "Gives a variable a value.",
        "AddVariable" => "Adds a number to a variable.",
        "Wait" => "The event waits for some seconds.",
        "If" => "The first list if all conditions hold, else the second one.",
        "OpenDoor" => "Sends a door or a mover to its second pose, and waits for it.",
        "CloseDoor" => "Sends a door or a mover to its first pose, and waits for it.",
        "LightOn" => "Turns a light of the room on.",
        "LightOff" => "Turns a light of the room off.",
        "LightIntensity" => "Sets how bright a light of the room is.",
        "Message" => "Shows a text until the player presses interact.",
        "Choice" => "Asks a question. The commands of the answer that the player takes run.",
        "FadeOut" => "The picture goes to black.",
        "FadeIn" => "The picture comes back from black.",
        "Transfer" => "Goes to a spawn point of a room. The event goes on there.",
        "SetField" => "Changes a field of a component of an object.",
        "CallScript" => "Calls a function of the script crate, with the object of the event.",
        "Signal" => "Sends an output of an object: its wires start.",
        _ => return None,
    })
}

/// The check of the built-in commands (`CommandRegistry::add_check`): each object of
/// a command is an object that the room has.
pub(crate) fn check(command: &Builtin, context: &CheckContext) -> Option<Finding> {
    match command {
        Builtin::LightOn(object)
        | Builtin::LightOff(object)
        | Builtin::LightIntensity(object, _) => {
            let found = context.reaches(object, &["light"]);
            (!found).then(|| Finding::error(format!("the room has no light `{object}`")))
        }
        Builtin::OpenDoor(object) | Builtin::CloseDoor(object) => {
            let found = context.reaches(object, &["door", "mover"]);
            (!found).then(|| Finding::error(format!("the room has no door or mover `{object}`")))
        }
        Builtin::Choice(_, answers) if answers.is_empty() => {
            Some(Finding::error("a question needs an answer".to_owned()))
        }
        // A warning: a room is made before the room that it leads to.
        Builtin::Transfer(file, spawn) => match context.has_spawn(file, spawn) {
            Some(true) => None,
            Some(false) => Some(Finding::warning(format!(
                "`{file}` has no spawn point `{spawn}`"
            ))),
            None => Some(Finding::warning(format!("no room file `{file}`"))),
        },
        Builtin::SetField(object, component, field, value) => context
            .field_problem(object, component, field, value)
            .map(Finding::error),
        Builtin::Signal(object, output) => {
            context.output_problem(object, output).map(Finding::error)
        }
        Builtin::CallScript(name) => context
            .registry
            .function(name)
            .is_none()
            .then(|| Finding::error(format!("no module added the script function `{name}`"))),
        _ => None,
    }
}

/// Sends each mover of the object to pose B or A (only `start`), and tells if all of
/// them are there.
fn send_mover(world: &mut World, object: &ObjectId, to_b: bool, start: bool) -> bool {
    let (mut found, mut arrived) = (false, true);
    let mut movers = world.query::<(Option<&ObjectId>, &mut Mover)>();
    for (id, mut mover) in movers.iter_mut(world) {
        if id != Some(object) {
            continue;
        }
        found = true;
        let target = if to_b {
            MoverState::AtB
        } else {
            MoverState::AtA
        };
        if mover.state == target {
            continue;
        }
        // Only a mover that is not there is written: a write is a change.
        if start {
            if to_b {
                mover.go_to_b();
            } else {
                mover.go_to_a();
            }
        }
        arrived = false;
    }
    if !found && start {
        tracing::warn!(
            %object,
            "an event names a door or mover that the room does not have"
        );
    }
    arrived
}

fn set_light(world: &mut World, object: &ObjectId, change: impl Fn(&Light) -> Light) {
    let mut found = false;
    let mut lights = world.query::<(Option<&ObjectId>, &mut Light)>();
    for (id, mut light) in lights.iter_mut(world) {
        if id == Some(object) {
            found = true;
            let new = change(&light);
            // A write is a change of the lighting for the renderer.
            if *light != new {
                *light = new;
            }
        }
    }
    if !found {
        tracing::warn!(%object, "an event names a light that the room does not have");
    }
}
