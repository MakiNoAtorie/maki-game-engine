//! Accumulation: the picture is the mean of many noisy frames.
//!
//! In each frame the trace pass makes some random samples of the light for each pixel
//! (`samples_per_frame` of the config). One sample is noisy. The accumulate pass keeps the mean of all samples since the
//! last reset, so the noise gets smaller with each frame.
//!
//! A change of the lighting alone does not start again: the mean keeps a short history
//! (a "soft reset"). The pixel still shows the same surface point, so the old samples
//! are almost correct, and they are much less noisy than one new frame. With a mean
//! that is limited to N samples, each new frame replaces a part of the old light.
//!
//! The mean is correct only while all its samples show the same scene. The decision to
//! start again is made on the game side: an `Extract` system that sees a change (with
//! `bevy_ecs` change detection) calls [`RenderWorld::reset_accumulation`]. The renderer
//! compares no data; it only reads the flag.

use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Local, Res, ResMut};
use maki_core::{App, Config, Extract};
use maki_platform::{Action, ActionState};

use crate::{ExtractedFrame, FrameGraph, RenderWorld, extract_frame};

/// Progress of the accumulated image. A resource of the render world.
#[derive(Resource, Debug, Default)]
pub struct Accumulation {
    /// A reset is waiting. Stays true over frames that are not drawn (minimized window).
    reset: bool,
    /// A soft reset is waiting.
    soft_reset: bool,
    /// This frame does a soft reset.
    soft_frame: bool,
    /// Number of samples in the mean, with the samples of this frame.
    samples: u32,
    /// Samples that this frame traces and adds to the mean. 0: the mean is complete.
    frame_samples: u32,
}

impl Accumulation {
    /// The next drawn frame starts the mean again.
    pub fn request_reset(&mut self) {
        self.reset = true;
    }

    /// The next drawn frame cuts the mean to a short history: see the module comment.
    pub fn request_soft_reset(&mut self) {
        self.soft_reset = true;
    }

    /// True in a frame that does a soft reset. The accumulate pass then drops the
    /// history of the pixels that show the light itself: see `shaders/accumulate.comp`.
    pub fn soft_frame(&self) -> bool {
        self.soft_frame
    }

    /// Number of samples in the mean, with the samples of this frame.
    pub fn samples(&self) -> u32 {
        self.samples
    }

    /// Samples for each pixel that this frame traces and adds to the mean.
    pub fn frame_samples(&self) -> u32 {
        self.frame_samples
    }

    /// False when the mean has `max_samples` samples: the passes that make a sample
    /// do no GPU work, and the picture stays as it is.
    pub fn sampling(&self) -> bool {
        self.frame_samples > 0
    }

    /// Call one time for each frame that is drawn, before the passes run.
    /// `history_samples`: the samples that the mean keeps on a soft reset.
    pub(crate) fn start_frame(
        &mut self,
        max_samples: u32,
        samples_per_frame: u32,
        history_samples: u32,
    ) {
        // The mean counts as one of `history_samples` samples: the samples of this
        // frame then get the weight `samples_per_frame / (history_samples + samples_per_frame)`,
        // and the old light fades by that part in each frame.
        self.soft_frame = std::mem::take(&mut self.soft_reset);
        if self.soft_frame {
            self.samples = self.samples.min(history_samples);
        }
        if std::mem::take(&mut self.reset) {
            // Not logged for a mean of one frame: a moving light resets in each frame.
            if self.samples > samples_per_frame {
                tracing::debug!(samples = self.samples, "accumulation reset");
            }
            self.samples = 0;
        }
        // The last frame of a mean traces only the samples that are missing.
        self.frame_samples = samples_per_frame.min(max_samples.saturating_sub(self.samples));
        self.samples += self.frame_samples;
        if self.sampling() && self.samples == max_samples {
            tracing::debug!(samples = self.samples, "accumulation complete");
        }
    }
}

/// Adds the accumulation systems. Part of `RenderModule`.
pub(crate) fn build(app: &mut App) {
    app.add_systems(
        Extract,
        (
            reset_on_action,
            choose_samples_per_frame.after(extract_frame),
        ),
    );
}

/// The largest sample count for one frame.
const MAX_SAMPLES_PER_FRAME: u32 = 64;
/// The GPU time of a frame arrives some frames later. The chooser keeps the sample
/// counts of this many frames to find the count that a time belongs to.
const COUNT_HISTORY: usize = 8;

/// State of `choose_samples_per_frame`.
struct SampleChooser {
    /// The choice for the next frame.
    count: u32,
    /// The samples that frame `f` traced, at index `f % COUNT_HISTORY`.
    traced: [u32; COUNT_HISTORY],
    /// The frame of the newest GPU time that was used.
    timed_frame: u64,
}

impl Default for SampleChooser {
    fn default() -> Self {
        Self {
            count: 1,
            traced: [0; COUNT_HISTORY],
            timed_frame: 0,
        }
    }
}

/// The sample count that fills `budget_ms`, if one sample costs `sample_ms`. At most
/// two times and at least half of `current`: one slow frame (a mesh upload, another
/// program) must not move the count far.
fn next_sample_count(current: u32, sample_ms: f32, budget_ms: f32) -> u32 {
    let fill = (budget_ms / sample_ms).floor() as u32;
    fill.clamp(current.div_ceil(2), current * 2)
        .clamp(1, MAX_SAMPLES_PER_FRAME)
}

/// With `samples_per_frame = 0` in the config: sets the sample count of each frame so
/// that the trace pass takes `trace_budget_ms`. It measures: the time of the pass
/// `trace` in an earlier frame, divided by the samples of that frame, is the cost of
/// one sample. On a GPU without pass timings the count stays 1.
fn choose_samples_per_frame(
    config: Res<Config>,
    graph: Option<Res<FrameGraph>>,
    mut render_world: ResMut<RenderWorld>,
    mut chooser: Local<SampleChooser>,
) {
    if config.samples_per_frame != 0 {
        return;
    }
    // The new `ExtractedFrame` has the number of this frame; the accumulation still
    // has the count of the frame before.
    let frame = render_world.0.resource::<ExtractedFrame>().frame;
    let last_traced = render_world.0.resource::<Accumulation>().frame_samples();
    chooser.traced[(frame.wrapping_sub(1) % COUNT_HISTORY as u64) as usize] = last_traced;

    if let Some(timings) = graph.as_ref().and_then(|graph| graph.timings())
        && timings.frame > chooser.timed_frame
        && frame - timings.frame < COUNT_HISTORY as u64
    {
        chooser.timed_frame = timings.frame;
        let traced = chooser.traced[(timings.frame % COUNT_HISTORY as u64) as usize];
        let trace_ms = timings.passes.iter().find(|(name, _)| *name == "trace");
        // No samples: the picture was complete in that frame, and the pass did nothing.
        if let Some((_, trace_ms)) = trace_ms
            && traced > 0
            && *trace_ms > 0.0
        {
            let sample_ms = trace_ms / traced as f32;
            chooser.count = next_sample_count(chooser.count, sample_ms, config.trace_budget_ms);
        }
    }
    render_world
        .0
        .resource_mut::<ExtractedFrame>()
        .samples_per_frame = chooser.count;
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
            accumulation.start_frame(3, 1, 0);
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

    #[test]
    fn a_frame_adds_its_samples_and_the_last_frame_only_the_missing_ones() {
        let mut accumulation = Accumulation::default();
        let frame = |accumulation: &mut Accumulation| {
            accumulation.start_frame(8, 3, 0);
            (accumulation.samples(), accumulation.frame_samples())
        };
        assert_eq!(frame(&mut accumulation), (3, 3));
        assert_eq!(frame(&mut accumulation), (6, 3));
        assert_eq!(frame(&mut accumulation), (8, 2));
        assert_eq!(frame(&mut accumulation), (8, 0));
        assert!(!accumulation.sampling());
    }

    #[test]
    fn the_sample_count_fills_the_budget_in_steps_of_a_factor_of_two() {
        // 0.5 ms for each sample and 6 ms: 12 samples, reached in steps.
        assert_eq!(next_sample_count(1, 0.5, 6.0), 2);
        assert_eq!(next_sample_count(4, 0.5, 6.0), 8);
        assert_eq!(next_sample_count(8, 0.5, 6.0), 12);
        assert_eq!(next_sample_count(12, 0.5, 6.0), 12);
        // A sample got slower: down in steps, and never below 1.
        assert_eq!(next_sample_count(12, 2.0, 6.0), 6);
        assert_eq!(next_sample_count(6, 2.0, 6.0), 3);
        assert_eq!(next_sample_count(1, 50.0, 6.0), 1);
        assert_eq!(next_sample_count(64, 0.001, 6.0), 64);
    }

    #[test]
    fn a_soft_reset_keeps_a_short_history_and_a_full_reset_wins() {
        let mut accumulation = Accumulation::default();
        for _ in 0..25 {
            accumulation.start_frame(1000, 4, 32);
        }
        assert_eq!(accumulation.samples(), 100);
        accumulation.request_soft_reset();
        accumulation.start_frame(1000, 4, 32);
        assert_eq!(accumulation.samples(), 36);
        // A complete mean samples again.
        accumulation.start_frame(36, 4, 32);
        assert!(!accumulation.sampling());
        accumulation.request_soft_reset();
        accumulation.start_frame(36, 4, 32);
        assert_eq!(
            (accumulation.samples(), accumulation.frame_samples()),
            (36, 4)
        );

        accumulation.request_soft_reset();
        accumulation.request_reset();
        accumulation.start_frame(1000, 4, 32);
        assert_eq!(accumulation.samples(), 4);
    }
}
