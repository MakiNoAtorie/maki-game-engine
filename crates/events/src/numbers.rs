//! Random numbers: a generator with a seed, and the command `RandomVariable`.

use bevy_ecs::resource::Resource;
use bevy_ecs::system::ResMut;
use bevy_ecs::world::World;
use maki_core::{App, Fields, ResetPlay, Settings};
use serde::{Deserialize, Serialize};

use crate::CommandRegistry;
use crate::command::Step;

/// The `[events]` table of the config file.
#[derive(Resource, Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EventsSettings {
    /// Seed of the random numbers. No key: the clock; a number: the same numbers in each run.
    pub seed: Option<u64>,
}

impl Settings for EventsSettings {
    const TABLE: &'static str = "events";
}

/// A xorshift64* generator: no dependency, and the same numbers on each platform.
#[derive(Resource, Debug, Clone)]
pub struct Random {
    /// The seed of the start: `ResetPlay` starts again from it.
    seed: u64,
    state: u64,
}

impl Random {
    pub fn new(seed: u64) -> Self {
        // Mixing spreads close seeds and keeps the state above 0 (0 would stay 0).
        let mixed = (seed ^ 0x9E37_79B9_7F4A_7C15).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        let state = if mixed == 0 { 1 } else { mixed };
        Self { seed, state }
    }

    fn next(&mut self) -> u64 {
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        self.state.wrapping_mul(0x2545_F491_4F6C_DD1D)
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
    app.insert_resource(Random::new(seed))
        .add_systems(ResetPlay, |mut random: ResMut<Random>| {
            *random = Random::new(random.seed);
        });
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
