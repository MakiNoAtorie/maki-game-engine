//! Events: what happens in a room, as data (in the style of RPG Maker).
//!
//! - [`GameState`]: flags and variables by name. It is a resource, so it stays when
//!   the room changes.
//! - `model`: an event has pages; a page has conditions, a trigger, and commands.
//!   The room file holds the events of a room.
//! - `interpreter`: starts the events and runs their commands, one step for each
//!   fixed step of the game.
//!
//! This crate must not depend on `gpu`, `render`, or `ui`. A command reaches the
//! picture only through data: a `Light`, a `Mover`, [`MessageBox`],
//! `maki_scene::ScreenFade`.

mod interpreter;
mod model;

use std::collections::BTreeMap;

use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::ResMut;
use maki_core::{App, FixedUpdate, Module, Startup, Update};
use maki_scene::{RoomValidators, load_requested_room, move_movers};

pub use interpreter::{EventRunner, MessageBox, RunningEvent, run_events};
pub use model::{
    Area, Command, Condition, EventDef, Page, Trigger, active_page, parse_events, validate_events,
};

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
        app.init_resource::<GameState>()
            .init_resource::<EventRunner>()
            .init_resource::<MessageBox>()
            .init_resource::<RoomValidators>()
            // A room with a bad event is not loaded.
            .add_systems(Startup, |mut validators: ResMut<RoomValidators>| {
                validators.0.push(validate_events);
            })
            // Before the movers: a door starts to turn in the step of its command.
            .add_systems(FixedUpdate, run_events.before(move_movers))
            .add_systems(
                Update,
                interpreter::load_room_events.after(load_requested_room),
            );
    }
}

#[cfg(test)]
mod tests;
