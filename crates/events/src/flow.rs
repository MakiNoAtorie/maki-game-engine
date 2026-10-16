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
use crate::{CommandRegistry, GameState, LabeledLists};

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
            Flow::WaitUntil(conditions) => all_hold(conditions, world.resource::<GameState>()),
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

#[cfg(test)]
mod tests {
    use crate::GameState;
    use crate::tests::{directory, game, room, steps};

    fn variable(app: &maki_core::App, name: &str) -> i32 {
        app.world().resource::<GameState>().variable(name)
    }

    #[test]
    fn any_of_holds_when_one_of_its_conditions_holds() {
        let directory = directory("any-of");
        let events = r#"(name: "or", pages: [(trigger: Autorun, commands: [
            SetFlag("b", true),
            If([Any([Flag("a"), Flag("b")])], [SetVariable("one", 1)], [SetVariable("one", 2)]),
            If([Any([Flag("a"), Variable("one", Equals, 5)])], [SetVariable("none", 1)], [SetVariable("none", 2)]),
            If([Any([])], [SetVariable("empty", 1)], [SetVariable("empty", 2)])])])"#;
        std::fs::write(directory.join("a.ron"), room(events)).unwrap();
        game(&directory).run(|mut app| {
            steps(&mut app, 4);
            assert_eq!(variable(&app, "one"), 1);
            assert_eq!(variable(&app, "none"), 2);
            // An "any of" with no condition does not hold.
            assert_eq!(variable(&app, "empty"), 2);
        });
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_loop_with_a_break_ends_and_one_with_no_wait_goes_on_over_steps() {
        let directory = directory("loop");
        let events = r#"
            (name: "counted", pages: [(trigger: Autorun, commands: [
                Loop([
                    AddVariable("count", 1),
                    If([Variable("count", AtLeast, 3)], [Break], []),
                    AddVariable("after", 1)]),
                SetFlag("done", true)])]),
            (name: "busy", pages: [(trigger: Parallel, commands: [
                Loop([AddVariable("spins", 1)])])])"#;
        std::fs::write(directory.join("a.ron"), room(events)).unwrap();
        game(&directory).run(|mut app| {
            steps(&mut app, 3);
            // The break left the loop and its `If`: the rows after the loop ran.
            assert_eq!(variable(&app, "count"), 3);
            assert_eq!(variable(&app, "after"), 2);
            assert!(app.world().resource::<GameState>().flag("done"));
            // A loop with no wait stops at the step limit and goes on in the next step.
            let spins = variable(&app, "spins");
            assert!(spins > 0);
            steps(&mut app, 2);
            assert_eq!(app.world().resource::<maki_core::Time>().fixed_tick, 5);
            assert!(variable(&app, "spins") > spins);
        });
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn wait_until_goes_on_when_its_conditions_hold() {
        let directory = directory("wait-until");
        let events = r#"
            (name: "waits", pages: [(trigger: Autorun, commands: [
                WaitUntil([Variable("ticks", AtLeast, 5)]), SetFlag("done", true)])]),
            (name: "clock", pages: [(trigger: Parallel, commands: [AddVariable("ticks", 1)])])"#;
        std::fs::write(directory.join("a.ron"), room(events)).unwrap();
        game(&directory).run(|mut app| {
            let done = |app: &maki_core::App| app.world().resource::<GameState>().flag("done");
            steps(&mut app, 4);
            assert!(variable(&app, "ticks") < 5 && !done(&app));
            steps(&mut app, 4);
            assert!(variable(&app, "ticks") >= 5 && done(&app));
        });
        std::fs::remove_dir_all(directory).unwrap();
    }
}
