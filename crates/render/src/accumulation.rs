//! Accumulation: how many samples the picture has, and how much of its history is valid.
//!
//! In each frame the trace pass makes some random samples of the light for each pixel
//! (`samples_per_frame` of the config). One sample is noisy. The temporal pass of the
//! denoiser keeps the mean of the samples of each pixel over frames (see the `denoise`
//! module), so the noise gets smaller with each frame. This module is the CPU side:
//! it counts the samples since the last change, stops the passes when the picture has
//! `max_samples`, and tells the temporal pass how much history a pixel can keep.
//!
//! The renderer compares no data to find a change. The decision is made on the game
//! side: an `Extract` system that sees a change (with `bevy_ecs` change detection)
//! tells what kind it is:
//! - [`RenderWorld::wake_accumulation`]: the view changed (the camera). The old samples
//!   are correct, at another place of the picture: the temporal pass finds them with
//!   the `motion` image. Only the count starts again, so new samples come.
//! - [`RenderWorld::soften_accumulation`]: the lighting changed (a light, or an object
//!   that moved and so its shadow). The old samples are almost correct. The indirect
//!   light of each pixel keeps a short history (`light_history_samples` of the
//!   config) for some frames, so the new light replaces the old one: see
//!   [`fade_frames`]. The direct light keeps its history where the denoiser finds no
//!   change (the fast history of `shaders/denoise_temporal.comp`): the noise of the
//!   indirect light is too large for such a test at one sample for each frame.
//! - [`RenderWorld::reset_accumulation`]: the old samples are of no use (a new image
//!   size, the manual reset). Each pixel starts again.

use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Local, Res, ResMut};
use maki_core::{App, Extract};
use maki_platform::{Action, ActionState, Key, add_bindings};

use crate::{ExtractedFrame, FrameGraph, RenderSettings, RenderWorld, extract_frame};

/// Starts the picture again: the mean over frames gets no old sample.
pub const RESET_ACCUMULATION: Action = Action("reset_accumulation");

/// Progress of the picture. A resource of the render world.
#[derive(Resource, Debug)]
pub struct Accumulation {
    /// A reset is waiting. Stays true over frames that are not drawn (minimized window).
    reset: bool,
    /// A soft reset is waiting.
    soft_reset: bool,
    /// A wake is waiting.
    wake: bool,
    /// Number of samples since the last change, with the samples of this frame.
    samples: u32,
    /// Samples that this frame traces and adds to the mean. 0: the picture is complete.
    frame_samples: u32,
    /// The history that a pixel can keep in this frame, in samples.
    history_limit: u32,
    /// This frame started with a reset: no image of the frame before is valid.
    reset_frame: bool,
    /// Frames, with this one, in which the history stays short after a soft reset.
    fade_frames: u32,
}

/// The first frame has no history: the history images have no content.
impl Default for Accumulation {
    fn default() -> Self {
        Self {
            reset: true,
            soft_reset: false,
            wake: false,
            samples: 0,
            frame_samples: 0,
            history_limit: 0,
            reset_frame: true,
            fade_frames: 0,
        }
    }
}

impl Accumulation {
    /// The next drawn frame drops the history of each pixel.
    pub fn request_reset(&mut self) {
        self.reset = true;
    }

    /// The next drawn frame cuts the history of each pixel to a short one.
    pub fn request_soft_reset(&mut self) {
        self.soft_reset = true;
    }

    /// The next drawn frame starts the sample count again and keeps the history.
    pub fn request_wake(&mut self) {
        self.wake = true;
    }

    /// Number of samples since the last change, with the samples of this frame. A
    /// pixel has this many samples in its mean, or more (a wake keeps the history).
    pub fn samples(&self) -> u32 {
        self.samples
    }

    /// Samples for each pixel that this frame traces and adds to the mean.
    pub fn frame_samples(&self) -> u32 {
        self.frame_samples
    }

    /// The largest number of old samples that the mean of a pixel keeps in this frame.
    /// 0: no history at all.
    pub fn history_limit(&self) -> u32 {
        self.history_limit
    }

    /// True in a frame that started with a reset: a pass must not read an image of the
    /// frame before (the first frame, a new image size, another room).
    pub fn reset_frame(&self) -> bool {
        self.reset_frame
    }

    /// False when the picture has `max_samples` samples: the passes that make a sample
    /// do no GPU work, and the picture stays as it is.
    pub fn sampling(&self) -> bool {
        self.frame_samples > 0
    }

    /// Call one time for each frame that is drawn, before the passes run.
    /// `history_samples`: the samples that a pixel keeps on a soft reset.
    pub(crate) fn start_frame(
        &mut self,
        max_samples: u32,
        samples_per_frame: u32,
        history_samples: u32,
    ) {
        let reset = std::mem::take(&mut self.reset);
        self.reset_frame = reset;
        let wake = std::mem::take(&mut self.wake);
        if std::mem::take(&mut self.soft_reset) {
            self.fade_frames = fade_frames(history_samples, samples_per_frame);
        }
        if reset {
            self.fade_frames = 0;
        }
        let fading = self.fade_frames > 0;
        self.fade_frames = self.fade_frames.saturating_sub(1);
        self.history_limit = if reset {
            0
        } else if fading {
            history_samples.min(max_samples)
        } else {
            max_samples
        };
        if fading {
            self.samples = self.samples.min(history_samples);
        }
        if reset || wake {
            // Not logged for a count of one frame: a moving camera wakes in each frame.
            if self.samples > samples_per_frame {
                tracing::debug!(samples = self.samples, reset, "accumulation starts again");
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

/// The number of frames in which the history stays short after a change of the
/// lighting. With a history of `h` samples and `s` new samples in a frame, each frame
/// keeps `h / (h + s)` of the old light. One short frame is not sufficient: after it
/// the count would grow again, and the old light would fade only as `h / count`. A
/// lamp that goes off would stay in the picture as a ghost.
/// The result: after this many frames, less than 1/1024 of the old light is left.
fn fade_frames(history_samples: u32, samples_per_frame: u32) -> u32 {
    let (history, new) = (history_samples as f32, samples_per_frame.max(1) as f32);
    if history <= 0.0 {
        return 1;
    }
    let frames = 1024.0_f32.ln() / ((history + new) / history).ln();
    // The limit: 10 s at 60 frames in a second, for a history of some thousand samples.
    (frames.ceil() as u32).clamp(1, 600)
}

/// Adds the accumulation systems. Part of `RenderModule`.
pub(crate) fn build(app: &mut App) {
    add_bindings(app, &[(Key::R, RESET_ACCUMULATION, "Reset accumulation")]);
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
    config: Res<RenderSettings>,
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
    if actions.just_pressed(RESET_ACCUMULATION) {
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
    fn each_kind_of_change_gives_the_temporal_pass_its_history_limit() {
        let mut accumulation = Accumulation::default();
        let frame = |accumulation: &mut Accumulation| {
            accumulation.start_frame(100, 4, 32);
            (accumulation.samples(), accumulation.history_limit())
        };
        // The first frame: the history images have no content.
        assert_eq!(frame(&mut accumulation), (4, 0));
        assert_eq!(frame(&mut accumulation), (8, 100));
        // The view changed: the count starts again, and the full history stays.
        accumulation.request_wake();
        assert_eq!(frame(&mut accumulation), (4, 100));
        // The lighting changed: a short history.
        for _ in 0..20 {
            frame(&mut accumulation);
        }
        accumulation.request_soft_reset();
        assert_eq!(frame(&mut accumulation), (36, 32));
        // A wake with a soft reset: the short history, and the count from zero.
        accumulation.request_wake();
        accumulation.request_soft_reset();
        assert_eq!(frame(&mut accumulation), (4, 32));
        accumulation.request_reset();
        assert_eq!(frame(&mut accumulation), (4, 0));
    }

    #[test]
    fn the_old_light_fades_in_few_frames_at_many_samples_and_in_more_at_few() {
        // 64 new samples on 32 old ones: one third of the old light stays in each frame.
        assert_eq!(fade_frames(32, 64), 7);
        assert!((1.0_f32 / 3.0).powi(7) < 1.0 / 1024.0);
        // One new sample on 32: 32/33 stays.
        assert_eq!(fade_frames(32, 1), 226);
        // No history: the frame of the change.
        assert_eq!(fade_frames(0, 4), 1);
        assert_eq!(fade_frames(100_000, 1), 600);
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
        // The history stays short until the old light is gone, then the count grows.
        for _ in 1..fade_frames(32, 4) {
            accumulation.start_frame(1000, 4, 32);
            assert_eq!(
                (accumulation.samples(), accumulation.history_limit()),
                (36, 32)
            );
        }
        accumulation.start_frame(1000, 4, 32);
        assert_eq!(
            (accumulation.samples(), accumulation.history_limit()),
            (40, 1000)
        );
        // A complete mean samples again.
        let mut complete = Accumulation::default();
        for _ in 0..10 {
            complete.start_frame(36, 4, 32);
        }
        assert!(!complete.sampling());
        complete.request_soft_reset();
        complete.start_frame(36, 4, 32);
        assert_eq!((complete.samples(), complete.frame_samples()), (36, 4));

        accumulation.request_soft_reset();
        accumulation.request_reset();
        accumulation.start_frame(1000, 4, 32);
        assert_eq!(accumulation.samples(), 4);
    }
}
