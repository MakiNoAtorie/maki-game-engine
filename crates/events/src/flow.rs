//! The commands of the flow of an event: a loop, its break, and "wait until".
//!
//! `Loop` holds a command list, so it has no field table (a list of commands is
//! not a field); it is an enum of its own with its lists (`add_lists`). The others
//! have a field table, so the event panel draws their arguments with no editor code.

use bevy_ecs::world::World;
use maki_core::Fields;
use serde::{Deserialize, Serialize};

use crate::builtin::conditions_text;
use crate::command::{Command, Jump, Step};
use crate::model::{Condition, all_hold};
use crate::{CommandRegistry, LabeledLists};

/// A loop: its list runs again each time it ends, until a `Break` in it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Loop {
    Loop(Vec<Command>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
pub enum Flow {
    /// Ends the innermost loop: the command after it runs next.
    #[field(label = "break")]
    Break,
    /// The event waits until all conditions hold.
    #[field(label = "wait until")]
    WaitUntil(#[field(label = "conditions (all must hold)")] Vec<Condition>),
}

/// Adds the commands of the flow to the registry.
pub(crate) fn add(registry: &CommandRegistry) {
    registry.add(|command: &Loop, step: &mut Step, _: &mut World| {
        let Loop::Loop(commands) = command;
        step.open(commands.clone());
        *step.jump = Jump::Loop;
        true
    });
    registry.add_lists(
        |Loop::Loop(commands): &Loop| -> LabeledLists<'_> { vec![(String::new(), commands)] },
        |_, lists| Loop::Loop(lists.into_iter().next().unwrap_or_default()),
    );
    registry.add_texts(
        |_: &Loop| Some("Loop".to_owned()),
        |name| (name == "Loop").then_some("Runs its list again and again, until a Break."),
    );
    registry.add_defaults(|name, _| (name == "Loop").then(|| Loop::Loop(Vec::new())));

    registry.add(
        |command: &Flow, step: &mut Step, world: &mut World| match command {
            Flow::Break => {
                *step.jump = Jump::Break;
                true
            }
            Flow::WaitUntil(conditions) => all_hold(conditions, &step.state(world)),
        },
    );
    registry.add_fields::<Flow>();
    registry.add_texts(
        |command: &Flow| {
            Some(match command {
                Flow::Break => "Break".to_owned(),
                Flow::WaitUntil(conditions) if conditions.is_empty() => {
                    "Wait until (no condition)".to_owned()
                }
                Flow::WaitUntil(conditions) => {
                    format!("Wait until {}", conditions_text(conditions, " and "))
                }
            })
        },
        |name| match name {
            "Break" => Some("Ends the loop around it: the command after the loop runs."),
            "WaitUntil" => Some("The event waits until all conditions hold."),
            _ => None,
        },
    );
    registry.add_defaults(|name, _| (name == "WaitUntil").then(|| Flow::WaitUntil(Vec::new())));
}
