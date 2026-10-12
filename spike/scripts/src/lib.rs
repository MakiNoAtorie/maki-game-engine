//! Spike of part 6. The Rust code of a project: the shake command of the game, and
//! one module that the reload test changes.

// The same file as the game has: no copy.
#[path = "../../../game/src/shake.rs"]
mod shake;

// Without this line nothing names the engine DLL, and this DLL gets its own copy
// of each engine crate.
extern crate maki_engine as _;

use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Local, Res};
use maki_core::{App, Module, Update};
use maki_events::CommandRegistry;
use serde::{Deserialize, Serialize};

// `tools/spike_reload.py` changes these two lines between two builds.
const LABEL: &str = "v1";
const PAD: usize = 1;

/// A resource whose size changes with `PAD`: two builds give one type name with two
/// layouts. Each lives in the world of its own app.
#[derive(Resource)]
struct Stamp {
    pad: [u64; PAD],
    label: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum SpikeCommand {
    Stamp,
}

struct SpikeModule;

impl Module for SpikeModule {
    fn build(&self, app: &mut App) {
        app.insert_resource(Stamp { pad: [7; PAD], label: LABEL })
            .add_systems(Update, |stamp: Res<Stamp>, mut done: Local<bool>| {
                if !std::mem::replace(&mut *done, true) {
                    let sum: u64 = stamp.pad.iter().sum();
                    tracing::info!(label = stamp.label, sum, "system of the script DLL");
                }
            });
        let registry = app.world().resource::<CommandRegistry>();
        registry.add(|_: &SpikeCommand, _step, _world| true);
        registry.add_texts(|_: &SpikeCommand| Some(format!("Stamp {LABEL}")), |_| None);
    }
}

/// What the program calls after the load (`maki_engine::ENTRY`).
#[unsafe(no_mangle)]
pub fn maki_project(app: &mut App) {
    app.add_module(shake::ShakeModule).add_module(SpikeModule);
}
