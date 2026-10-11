//! Accumulation: the picture is the mean of many noisy frames.
//!
//! In each frame the trace pass makes one random sample of the light for each pixel.
//! One sample is noisy. The accumulate pass keeps the mean of all samples since the
//! last reset, so the noise gets smaller with each frame.
//!
//! The mean is correct only while all its samples show the same scene. The decision to
//! start again is made on the game side: an `Extract` system that sees a change (with
//! `bevy_ecs` change detection) calls [`RenderWorld::reset_accumulation`]. The renderer
//! compares no data; it only reads the flag.

use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Res, ResMut};
use maki_core::{App, Extract};
use maki_platform::{Action, ActionState};

use crate::RenderWorld;

/// Progress of the accumulated image. A resource of the render world.
#[derive(Resource, Debug, Default)]
pub struct Accumulation {
    /// A reset is waiting. Stays true over frames that are not drawn (minimized window).
    reset: bool,
    /// Number of samples in the mean, with the sample of this frame.
    samples: u32,
    /// True: this frame traces a sample and adds it to the mean.
    sampling: bool,
}

impl Accumulation {
    /// The next drawn frame starts the mean again.
    pub fn request_reset(&mut self) {
        self.reset = true;
    }

    /// Number of samples in the mean, with the sample of this frame.
    pub fn samples(&self) -> u32 {
        self.samples
    }

    /// False when the mean has `max_samples` samples: the passes that make a sample
    /// do no GPU work, and the picture stays as it is.
    pub fn sampling(&self) -> bool {
        self.sampling
    }

    /// Call one time for each frame that is drawn, before the passes run.
    pub(crate) fn start_frame(&mut self, max_samples: u32) {
        if std::mem::take(&mut self.reset) {
            // Not logged for a mean of one sample: a moving light resets in each frame.
            if self.samples > 1 {
                tracing::debug!(samples = self.samples, "accumulation reset");
            }
            self.samples = 0;
        }
        self.sampling = self.samples < max_samples;
        if self.sampling {
            self.samples += 1;
            if self.samples == max_samples {
                tracing::debug!(samples = self.samples, "accumulation complete");
            }
        }
    }
}

/// Adds the accumulation systems. Part of `RenderModule`.
pub(crate) fn build(app: &mut App) {
    app.add_systems(Extract, reset_on_action);
}

fn reset_on_action(actions: Res<ActionState>, mut render_world: ResMut<RenderWorld>) {
    if actions.just_pressed(Action::ResetAccumulation) {
        render_world.reset_accumulation();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_count_to_the_cap_and_a_reset_starts_again() {
        let mut accumulation = Accumulation::default();
        let frame = |accumulation: &mut Accumulation| {
            accumulation.start_frame(3);
            (accumulation.samples(), accumulation.sampling())
        };
        assert_eq!(frame(&mut accumulation), (1, true));
        assert_eq!(frame(&mut accumulation), (2, true));
        assert_eq!(frame(&mut accumulation), (3, true));
        // At the cap: no more samples.
        assert_eq!(frame(&mut accumulation), (3, false));

        // Two requests before one frame are one reset. The flag is used one time.
        accumulation.request_reset();
        accumulation.request_reset();
        assert_eq!(frame(&mut accumulation), (1, true));
        assert_eq!(frame(&mut accumulation), (2, true));
    }
}
