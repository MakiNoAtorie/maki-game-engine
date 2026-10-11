use std::time::Duration;

use bevy_ecs::resource::Resource;

/// Clock values for the current frame. `App` writes them; systems only read them.
#[derive(Resource, Debug, Clone, Default)]
pub struct Time {
    /// Real time between the start of the previous frame and the start of this one.
    pub delta: Duration,
    /// Length of one FixedUpdate step. Constant during a run.
    pub fixed_delta: Duration,
    /// Real time since the first frame.
    pub elapsed: Duration,
    /// Number of the current frame. The first frame is 1.
    pub frame: u64,
    /// Number of FixedUpdate steps done since startup.
    pub fixed_tick: u64,
    /// Position of this frame between the last two fixed steps, in 0.0..1.0.
    /// The renderer blends previous and current simulation state with it.
    pub alpha: f32,
    /// Real time not yet consumed by fixed steps.
    pub(crate) accumulator: Duration,
}
