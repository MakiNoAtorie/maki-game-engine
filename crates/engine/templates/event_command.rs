//! ScriptName: a command for the command rows of the events, and for scripts
//! (`context.call(ScriptNameCommand::ScriptName(1.0))`). A room file writes
//! `ScriptName(1.0)` in a list of commands.

use bevy_ecs::world::World;
use maki_engine::core::{App, Fields, Module};
use maki_engine::events::{CommandRegistry, Finding, Step};
use serde::{Deserialize, Serialize};

/// The commands of this module. The name of a variant must be unique in the game.
/// `Fields`: the event panel draws the arguments. A field that names an object of the
/// room is a `maki_engine::scene::ObjectRef`: the editor picks it by its name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
pub enum ScriptNameCommand {
    /// Change this: what the number means.
    ScriptName(#[field(label = "value")] f32),
}

pub struct ScriptNameModule;

impl Module for ScriptNameModule {
    fn build(&self, app: &mut App) {
        let registry = app.world().resource::<CommandRegistry>();
        registry.add(run);
        // The row and the help in the event panel of the editor.
        registry.add_texts(
            |command: &ScriptNameCommand| {
                let ScriptNameCommand::ScriptName(value) = command;
                Some(format!("ScriptName: {value}"))
            },
            |_| Some("Change this: what the command does."),
        );
        // The widgets of the arguments in the event panel.
        registry.add_fields::<ScriptNameCommand>();
        // What "Add" in the event panel makes.
        registry.add_defaults(|_, _| Some(ScriptNameCommand::ScriptName(1.0)));
        // A check when a room loads, and on the row in the editor.
        registry.add_check(|command: &ScriptNameCommand, _room| {
            let ScriptNameCommand::ScriptName(value) = command;
            (*value < 0.0).then(|| Finding::warning("the number is below 0".to_owned()))
        });
    }
}

/// One step of the command. True: it has ended. False: it waits, and runs again in
/// the next step (`step.memory` keeps a value from one step to the next).
fn run(command: &ScriptNameCommand, _step: &mut Step, _world: &mut World) -> bool {
    let ScriptNameCommand::ScriptName(value) = command;
    maki_engine::tracing::info!(value, "ScriptName runs");
    true
}

pub fn add(app: &mut App) {
    app.add_module(ScriptNameModule);
}
