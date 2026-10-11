//! Example module: shows that FixedUpdate runs at the configured rate. Remove it when not needed.

use bevy_ecs::system::Res;
use maki_core::{App, FixedUpdate, Module, Time};

pub struct TickLoggerModule;

impl Module for TickLoggerModule {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, log_ticks);
    }
}

fn log_ticks(time: Res<Time>) {
    let ticks_per_second = (1.0 / time.fixed_delta.as_secs_f64()).round() as u64;
    if time.fixed_tick.is_multiple_of(ticks_per_second) {
        // `elapsed` follows real time when no frame is slow, so the two values show the real rate.
        tracing::info!(fixed_tick = time.fixed_tick, elapsed = ?time.elapsed, frame = time.frame, "tick");
    }
}
