use std::time::Duration;

use bevy_ecs::resource::Resource;

/// Clock values for the current frame. `App` writes them; systems only read them.
#[derive(Resource, Debug, Clone, Default)]
pub struct Time {
    /// Time between the start of the previous frame and the start of this one.
    /// Limited to `max_fixed_steps` fixed steps, so a slow frame does not cause a large jump.
    pub delta: Duration,
    /// Length of one FixedUpdate step. Constant during a run.
    pub fixed_delta: Duration,
    /// Sum of all `delta` values. Behind real time after slow frames.
    pub elapsed: Duration,
    /// Number of the current frame. The first frame is 1.
    pub frame: u64,
    /// Number of FixedUpdate steps done since startup.
    pub fixed_tick: u64,
    /// Position of this frame between the last two fixed steps, in 0.0..1.0.
    /// The renderer blends previous and current simulation state with it.
    pub alpha: f32,
    /// `Some`: each frame counts as this much time, whatever the real time was. The
    /// simulation then does the same steps in each run: for a benchmark with a script.
    /// A system can write it. It applies from the next frame.
    pub frame_time: Option<Duration>,
    /// True: no fixed step runs, so the simulation stands still. `Update` and the
    /// renderer go on. A system can write it (an editor, a pause).
    pub paused: bool,
    /// Frame time not yet consumed by fixed steps.
    pub(crate) accumulator: Duration,
}
