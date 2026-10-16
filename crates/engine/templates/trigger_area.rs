//! ScriptName: does something when the player walks into the area of the object and
//! out of it. Give the object the components `"script_name": (...)` and
//! `"area": (width: 2.0, depth: 2.0)`.

use bevy_ecs::component::Component;
use maki_engine::core::{App, Fields};
use maki_engine::events::{Builtin, Script, ScriptContext, add_script};
use serde::{Deserialize, Serialize};

#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
#[fields(default)]
pub struct ScriptName {
    /// True: only the first time. The flag `flag` keeps that it happened.
    #[field(label = "one time only")]
    pub once: bool,
    /// Set when the player walks in.
    pub flag: String,
}

impl Default for ScriptName {
    fn default() -> Self {
        Self {
            once: true,
            flag: "script_name_entered".to_owned(),
        }
    }
}

impl Script for ScriptName {
    fn enter_area(&mut self, context: &mut ScriptContext) {
        if self.once && context.state().flag(&self.flag) {
            return;
        }
        context.call(Builtin::SetFlag(self.flag.clone(), true));
        // Change this: a message, a door (`Builtin::OpenDoor`), a light.
        maki_engine::tracing::info!("{} entered", context.name());
    }

    fn leave_area(&mut self, context: &mut ScriptContext) {
        maki_engine::tracing::info!("{} left", context.name());
    }
}

pub fn add(app: &mut App) {
    add_script::<ScriptName>(app, "script_name");
}
