//! ScriptName: a module of the game with a table of the config file, a resource,
//! and a system. The table:
//!
//! ```toml
//! [script_name]
//! rate = 1.0
//! ```

use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Res, ResMut};
use maki_engine::core::{App, Module, Settings, Time, Update};
use serde::{Deserialize, Serialize};

/// The table `[script_name]` of the config file.
#[derive(Resource, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScriptNameSettings {
    /// Counts in a second.
    pub rate: f32,
}

impl Default for ScriptNameSettings {
    fn default() -> Self {
        Self { rate: 1.0 }
    }
}

impl Settings for ScriptNameSettings {
    const TABLE: &'static str = "script_name";

    fn check(&self) -> Result<(), String> {
        if self.rate < 0.0 {
            return Err("rate must not be below 0".to_owned());
        }
        Ok(())
    }
}

/// The state of the module. A resource stays when the room changes.
#[derive(Resource, Debug, Default)]
pub struct ScriptNameState {
    pub count: f32,
}

pub struct ScriptNameModule;

impl Module for ScriptNameModule {
    fn build(&self, app: &mut App) {
        app.add_settings::<ScriptNameSettings>()
            .init_resource::<ScriptNameState>()
            .add_systems(Update, count);
    }
}

fn count(settings: Res<ScriptNameSettings>, time: Res<Time>, mut state: ResMut<ScriptNameState>) {
    state.count += settings.rate * time.delta.as_secs_f32();
}

pub fn add(app: &mut App) {
    app.add_module(ScriptNameModule);
}
