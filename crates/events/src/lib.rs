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

mod area;
mod builtin;
mod check;
mod command;
mod flow;
mod interpreter;
mod items;
mod menu;
mod model;
mod numbers;
mod objects;
mod routine;
mod script;
mod state;
mod text;
mod timeline;

use std::collections::BTreeMap;

use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::schedule::common_conditions::resource_exists_and_changed;
use maki_core::{App, FixedUpdate, Module, ResetPlay, Update};
use maki_physics::PhysicsSet;
use maki_platform::{Action, Key, add_bindings};
use maki_scene::{RoomValidators, load_requested_room, move_movers};

pub use builtin::Builtin;
pub use check::{CheckContext, FallbackTexts, Finding, Problem};
pub use command::{Command, CommandRegistry, CommandSet, LabeledLists, ScriptFunction, Step};
pub use flow::{Flow, Loop};
pub use interpreter::{
    ChoiceBox, Choices, EventRunner, MessageBox, RunningEvent, Signals, Start, load_room_events,
    run_events, start_event,
};
pub use items::{Inventory, ItemCommand, ItemKind, item_id};
pub use menu::{
    Menu, MenuCommand, MenuFiles, MenuPlace, MenuRow, Menus, OpenMenu, ShownMenu, ShownMenus,
    ShownRow, menu_text, read_menu,
};
pub use model::{
    Area, Compare, Condition, EventDef, EventPages, Facts, Page, SavedGame, Trigger, Wire,
    active_page, all_hold, short_events,
};
pub use numbers::{EventsSettings, Numbers, Random};
pub use objects::ObjectCommand;
pub use routine::{
    Arguments, ROUTINE_EXTENSION, RoutineCommand, RoutineDef, Routines, read_routine, routine_text,
};
pub use script::{
    EYE_HEIGHT, Script, ScriptContext, ScriptPanics, TriggerArea, WalkGoal, add_outputs,
    add_script, add_script_function, object_entity,
};
pub use state::{
    NamedNumber, NamedSwitch, ObjectState, ObjectStates, RoomStates, StateCommand, StateValues,
    room_key,
};
pub use text::{
    TEXT_FOLDER, TextCommand, TextEntry, TextKey, TextSettings, TextTable, Texts, fill,
    table_files, table_path, text, texts,
};
pub use timeline::TimelineCommand;

/// Starts the event that the player faces, closes a message, and takes the marked
/// answer of a question.
pub const INTERACT: Action = Action("interact");
/// Move the mark of a question (`Choice`) to the answer above or below.
pub const CHOICE_UP: Action = Action("choice_up");
pub const CHOICE_DOWN: Action = Action("choice_down");
/// Closes the top menu with focus, if its `back` is on.
pub const BACK: Action = Action("back");

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

/// The console commands of the game state and the inventory.
fn add_console_commands(app: &mut App) {
    use maki_core::add_console_command;
    add_console_command(app, "flag", "NAME on|off", "Sets a flag", |world, args| {
        let (name, value) = match args {
            [name, "on"] => (name, true),
            [name, "off"] => (name, false),
            _ => return Err("a name and on or off".into()),
        };
        world.resource_mut::<GameState>().set_flag(name, value);
        Ok(format!("flag {name} {}", if value { "on" } else { "off" }))
    });
    add_console_command(
        app,
        "var",
        "NAME NUMBER",
        "Sets a variable",
        |world, args| {
            let [name, value] = args else {
                return Err("a name and a number".into());
            };
            let value: i32 = value
                .parse()
                .map_err(|_| format!("`{value}` is not a number"))?;
            world.resource_mut::<GameState>().set_variable(name, value);
            Ok(format!("var {name} = {value}"))
        },
    );
    let help = "Gives the player an item: its file, from the folder of the room";
    add_console_command(app, "give", "ITEM_FILE [COUNT]", help, |world, args| {
        let (file, count) = match args {
            [file] => (file, 1),
            [file, count] => (
                file,
                count
                    .parse()
                    .map_err(|_| format!("`{count}` is not a number"))?,
            ),
            _ => return Err("an item file".into()),
        };
        let room = &world.resource::<maki_scene::CurrentRoom>().path;
        let path = room.parent().unwrap_or(std::path::Path::new("")).join(file);
        let id = maki_assets::FileId::read(&path)
            .ok_or_else(|| format!("{} has no file id", path.display()))?;
        world.resource_mut::<Inventory>().add(id, count);
        Ok(format!("gave {count} of {}", path.display()))
    });
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
        flow::add(&registry);
        numbers::add(app, &registry);
        routine::add(app, &registry);
        add_console_commands(app);
        add_bindings(
            app,
            &[
                (Key::PadA, INTERACT, "Interact"),
                (Key::Z, INTERACT, "Interact"),
                (Key::Enter, INTERACT, "Interact"),
                (Key::PadUp, CHOICE_UP, "Question: the answer above"),
                (Key::LeftStickUp, CHOICE_UP, "Question: the answer above"),
                (Key::PadDown, CHOICE_DOWN, "Question: the answer below"),
                (
                    Key::LeftStickDown,
                    CHOICE_DOWN,
                    "Question: the answer below",
                ),
                (Key::PadB, BACK, "Menu: back"),
                (Key::ArrowUp, CHOICE_UP, "Question: the answer above"),
                (Key::W, CHOICE_UP, "Question: the answer above"),
                (Key::ArrowDown, CHOICE_DOWN, "Question: the answer below"),
                (Key::S, CHOICE_DOWN, "Question: the answer below"),
                // Not `Esc` (the debug quit) and not `P` (the pause).
                (Key::X, BACK, "Menu: back"),
                (Key::Backspace, BACK, "Menu: back"),
            ],
        );
        app.init_resource::<GameState>()
            .init_resource::<EventRunner>()
            .init_resource::<interpreter::Signals>()
            .init_resource::<MessageBox>()
            .init_resource::<ChoiceBox>()
            .init_resource::<RoomValidators>()
            .init_resource::<script::ScriptRunners>()
            .init_resource::<ScriptPanics>()
            // Areas need `PhysicsModule`; with no module the world is empty.
            .init_resource::<maki_physics::Physics>()
            .add_systems(
                FixedUpdate,
                (
                    area::add_sensors.after(run_events).before(PhysicsSet::Step),
                    area::track_player.after(run_events).after(PhysicsSet::Step),
                ),
            );
        // The area of an object, for the hooks of its scripts and for its event.
        maki_scene::add_object_component::<TriggerArea>(app, "area");
        registry.add_outputs("area", TriggerArea::OUTPUTS);
        // The pages of an event: commands of all modules, read with the registry.
        maki_scene::add_component_reader(app, "event", interpreter::insert_event);
        // A room with a bad event is not loaded. The check has the registry, with
        // the commands and the checks that other modules add later.
        // Show, hide, and move of any object.
        objects::add(app, &registry);
        // The switches and numbers of an object.
        state::add(app, &registry);
        // Item kinds and the inventory.
        items::add(app, &registry);
        // Play and stop of a timeline.
        timeline::add(app, &registry);
        // The menus and their stack.
        menu::add(app, &registry);
        // The text tables and the texts of the game.
        text::add(app, &registry);
        let fallback = &app.world().resource::<TextSettings>().fallback;
        registry.texts.lock().expect("no holder panics").fallback = fallback.clone();
        let check = registry.validator();
        app.resource_mut::<RoomValidators>().0.push(check);
        app.insert_resource(registry)
            // A door turns in the step of its command; the animator runs first, so no system needs an order to it.
            .add_systems(
                FixedUpdate,
                run_events.before(move_movers).after(maki_scene::animate),
            )
            .add_systems(ResetPlay, interpreter::reset_play)
            .add_systems(
                Update,
                interpreter::load_room_events.after(load_requested_room),
            )
            .add_systems(
                Update,
                check::copy_component_types
                    .run_if(resource_exists_and_changed::<maki_scene::ComponentTypes>)
                    .before(load_requested_room),
            );
    }
}

#[cfg(test)]
mod tests;
