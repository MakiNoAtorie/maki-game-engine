//! ScriptName: a script with no behavior yet. Give an object the component
//! `"script_name": ()` in the editor or in the room file.

use bevy_ecs::component::Component;
use maki_engine::core::{App, Fields};
use maki_engine::events::{Script, ScriptContext, add_script};
use serde::{Deserialize, Serialize};

/// The fields show in the inspector and are in the room file. A field with
/// `#[serde(skip)]` and `#[field(skip)]` is state of the game only.
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
#[fields(default)]
pub struct ScriptName {
    /// Change this field, or add others.
    #[field(range = 0.0..=10.0)]
    pub speed: f32,
}

impl Default for ScriptName {
    fn default() -> Self {
        Self { speed: 1.0 }
    }
}

impl Script for ScriptName {
    /// The first step of the object: after the room loads.
    fn start(&mut self, _context: &mut ScriptContext) {}

    /// Each fixed step. `context.seconds()` is its length.
    fn step(&mut self, _context: &mut ScriptContext) {}

    /// The player faces the area of the object (the component `area`) and presses
    /// interact.
    fn interact(&mut self, _context: &mut ScriptContext) {}

    /// The player walks into the area of the object.
    fn enter_area(&mut self, _context: &mut ScriptContext) {}

    /// The player walks out of the area.
    fn leave_area(&mut self, _context: &mut ScriptContext) {}
}

/// `lib.rs` of the script crate calls this in `add_modules`.
pub fn add(app: &mut App) {
    add_script::<ScriptName>(app, "script_name");
}
