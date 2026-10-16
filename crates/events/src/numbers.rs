//! Random numbers: a generator with a seed, and the command `RandomVariable`.

use bevy_ecs::resource::Resource;
use bevy_ecs::world::World;
use maki_core::{App, Fields, Settings};
use serde::{Deserialize, Serialize};

use crate::CommandRegistry;
use crate::command::Step;

/// The `[events]` table of the config file.
#[derive(Resource, Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EventsSettings {
    /// Seed of the random numbers. No key: the clock (a game); a number: the same
    /// numbers in each run (a test, a benchmark).
    pub seed: Option<u64>,
}

impl Settings for EventsSettings {
    const TABLE: &'static str = "events";
}

/// A xorshift64* generator. Own code: it needs no dependency, and a seed gives the
/// same numbers on every platform.
#[derive(Resource, Debug, Clone)]
pub struct Random(u64);

impl Random {
    pub fn new(seed: u64) -> Self {
        // Mixing spreads close seeds and keeps the state above 0 (0 would stay 0).
        let mixed = (seed ^ 0x9E37_79B9_7F4A_7C15).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        Self(if mixed == 0 { 1 } else { mixed })
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number from `min` to `max`, both included. A `min` above `max` swaps them.
    pub fn range(&mut self, min: i32, max: i32) -> i32 {
        let (min, max) = (min.min(max), min.max(max));
        let size = (max as i64 - min as i64 + 1) as u64;
        // The modulo bias is below 2^-32 for any range: no rejection loop.
        (min as i64 + (self.next() % size) as i64) as i32
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
pub enum Numbers {
    /// A variable gets a random number from `min` to `max`, both included.
    #[field(label = "random variable")]
    RandomVariable(
        #[field(pick = "variables")] String,
        #[field(label = "min")] i32,
        #[field(label = "max")] i32,
    ),
}

/// Adds the settings, the generator, and the command.
pub(crate) fn add(app: &mut App, registry: &CommandRegistry) {
    app.add_settings::<EventsSettings>();
    let seed = app
        .world()
        .resource::<EventsSettings>()
        .seed
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(1, |time| time.as_nanos() as u64)
        });
    app.insert_resource(Random::new(seed));
    registry.add(|command: &Numbers, step: &mut Step, world: &mut World| {
        let Numbers::RandomVariable(name, min, max) = command;
        let number = world.resource_mut::<Random>().range(*min, *max);
        step.set_variable(world, name, number);
        true
    });
    registry.add_fields::<Numbers>();
    registry.add_texts(
        |command: &Numbers| {
            let Numbers::RandomVariable(name, min, max) = command;
            Some(format!("Variable {name} = random {min} to {max}"))
        },
        |_| Some("Sets the variable to a random number from min to max, both included."),
    );
    registry.add_defaults(|name, _| {
        (name == "RandomVariable").then(|| Numbers::RandomVariable(String::new(), 1, 6))
    });
}
