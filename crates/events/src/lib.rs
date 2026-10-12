//! Events: what happens in a room, as data (in the style of RPG Maker).
//!
//! - [`GameState`]: flags and variables by name. It is a resource, so it stays when
//!   the room changes.
//! - `model`: an event has pages; a page has conditions, a trigger, and commands.
//!   The room file holds the events of a room.
//! - `interpreter`: starts the events and runs their commands, one step for each
//!   fixed step of the game.
//! - `command`: the commands are an open set. A module adds its own to the
//!   [`CommandRegistry`]; `builtin` has those of this crate.
//!
//! This crate must not depend on `gpu`, `render`, or `ui`. A command reaches the
//! picture only through data: a `Light`, a `Mover`, [`MessageBox`],
//! `maki_scene::ScreenFade`.

mod builtin;
mod check;
mod command;
mod interpreter;
mod model;

use std::collections::BTreeMap;

use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use maki_core::{App, FixedUpdate, Module, ResetPlay, Update};
use maki_platform::{Action, Key, add_bindings};
use maki_scene::{RoomValidators, load_requested_room, move_movers};

pub use builtin::Builtin;
pub use check::{CheckContext, Finding, Problem};
pub use command::{Command, CommandRegistry, CommandSet, LabeledLists, Step};
pub use interpreter::{
    ChoiceBox, Choices, EventRunner, MessageBox, RunningEvent, load_room_events, run_events,
};
pub use model::{Area, Compare, Condition, EventDef, Page, Trigger, active_page, all_hold};

/// Starts the event that the player faces, closes a message, and takes the marked
/// answer of a question.
pub const INTERACT: Action = Action("interact");
/// Move the mark of a question (`Choice`) to the answer above or below.
pub const CHOICE_UP: Action = Action("choice_up");
pub const CHOICE_DOWN: Action = Action("choice_down");

/// What the player has done, as flags (yes or no) and variables (whole numbers), each
/// with a name. Events set them and have them as conditions. A name that was never
/// set is a flag that is not set and a variable of 0.
// Sorted maps: a debug panel shows the names in the same order in each frame.
#[derive(Resource, Debug, Clone, PartialEq, Eq, Default)]
pub struct GameState {
    pub flags: BTreeMap<String, bool>,
    pub variables: BTreeMap<String, i32>,
}

impl GameState {
    pub fn flag(&self, name: &str) -> bool {
        self.flags.get(name).copied().unwrap_or(false)
    }

    pub fn variable(&self, name: &str) -> i32 {
        self.variables.get(name).copied().unwrap_or(0)
    }

    pub fn set_flag(&mut self, name: &str, value: bool) {
        self.flags.insert(name.to_owned(), value);
    }

    pub fn set_variable(&mut self, name: &str, value: i32) {
        self.variables.insert(name.to_owned(), value);
    }
}

/// Adds the game state and the event interpreter. Needs `maki_scene::SceneModule` and
/// `maki_platform::PlatformModule`.
pub struct EventsModule;

impl Module for EventsModule {
    fn build(&self, app: &mut App) {
        let registry = CommandRegistry::default();
        registry.add(builtin::run);
        registry.add_check(builtin::check);
        registry.add_texts(builtin::row_text, builtin::help);
        registry.add_lists(builtin::lists, builtin::with_lists);
        add_bindings(
            app,
            &[
                (Key::Z, INTERACT, "Interact"),
                (Key::Enter, INTERACT, "Interact"),
                (Key::ArrowUp, CHOICE_UP, "Question: the answer above"),
                (Key::W, CHOICE_UP, "Question: the answer above"),
                (Key::ArrowDown, CHOICE_DOWN, "Question: the answer below"),
                (Key::S, CHOICE_DOWN, "Question: the answer below"),
            ],
        );
        app.init_resource::<GameState>()
            .init_resource::<EventRunner>()
            .init_resource::<MessageBox>()
            .init_resource::<ChoiceBox>()
            .init_resource::<RoomValidators>();
        // A room with a bad event is not loaded. The check has the registry, with
        // the commands and the checks that other modules add later.
        let check = registry.validator();
        app.resource_mut::<RoomValidators>().0.push(check);
        app.insert_resource(registry)
            // Before the movers: a door starts to turn in the step of its command.
            .add_systems(FixedUpdate, run_events.before(move_movers))
            .add_systems(ResetPlay, interpreter::reset_play)
            .add_systems(
                Update,
                interpreter::load_room_events.after(load_requested_room),
            );
    }
}

#[cfg(test)]
mod tests;
