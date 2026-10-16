//! ScriptName: a message when the player interacts with the object. Give the object
//! the components `"script_name": (...)` and `"area": ()`.

use bevy_ecs::component::Component;
use maki_engine::core::{App, Fields};
use maki_engine::events::{Builtin, Script, ScriptContext, add_script};
use serde::{Deserialize, Serialize};

#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
#[fields(default)]
pub struct ScriptName {
    /// What the message box shows.
    pub message: String,
    /// True: only the first press shows it. The flag `flag` keeps that it was shown,
    /// also in the save file.
    #[field(label = "one time only")]
    pub once: bool,
    /// The name of the flag of `once`.
    pub flag: String,
}

impl Default for ScriptName {
    fn default() -> Self {
        Self {
            message: "Nothing happens.".to_owned(),
            once: false,
            flag: "script_name_done".to_owned(),
        }
    }
}

impl Script for ScriptName {
    fn interact(&mut self, context: &mut ScriptContext) {
        if self.once && context.state().flag(&self.flag) {
            return;
        }
        // The commands of the events, as functions: the same as a row of an event.
        context.call(Builtin::SetFlag(self.flag.clone(), true));
        context.call(Builtin::Message(self.message.clone()));
    }
}

pub fn add(app: &mut App) {
    add_script::<ScriptName>(app, "script_name");
}
