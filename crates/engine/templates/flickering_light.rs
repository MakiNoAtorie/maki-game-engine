//! ScriptName: the brightness of a light by a pattern of letters, as in old games:
//! `a` is dark, `m` the brightness of the file, `z` about twice it. Give an object
//! with a light the component `"script_name": (pattern: "mmamammmmammamamaaamammma")`.

use bevy_ecs::component::Component;
use maki_engine::core::{App, Fields};
use maki_engine::events::{Script, ScriptContext, add_script};
use maki_engine::scene::Light;
use serde::{Deserialize, Serialize};

#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
#[fields(default)]
pub struct ScriptName {
    /// Letters from `a` to `z`, one after the other, again and again.
    pub pattern: String,
    /// Letters in a second.
    #[field(range = 0.1..=60.0, step = 0.5)]
    pub rate: f32,
    /// The brightness of the file.
    #[serde(skip)]
    #[field(skip)]
    base: f32,
    #[serde(skip)]
    #[field(skip)]
    time: f32,
}

impl Default for ScriptName {
    fn default() -> Self {
        Self {
            pattern: "mmamammmmammamamaaamammma".to_owned(),
            rate: 10.0,
            base: 0.0,
            time: 0.0,
        }
    }
}

impl Script for ScriptName {
    fn start(&mut self, context: &mut ScriptContext) {
        // The light of the object is on the entity of the object.
        self.base = context
            .get_mut::<Light>()
            .map_or(0.0, |light| light.intensity);
    }

    fn step(&mut self, context: &mut ScriptContext) {
        let letters = self.pattern.as_bytes();
        if letters.is_empty() {
            return;
        }
        self.time += context.seconds();
        let index = (self.time * self.rate) as usize % letters.len();
        let level = f32::from(letters[index].clamp(b'a', b'z') - b'a') / 12.0;
        let intensity = self.base * level;
        if let Some(mut light) = context.get_mut::<Light>() {
            // Only a different value is a write: a change of a light is a change for
            // the renderer.
            if light.intensity != intensity {
                light.intensity = intensity;
            }
        }
    }
}

pub fn add(app: &mut App) {
    add_script::<ScriptName>(app, "script_name");
}
