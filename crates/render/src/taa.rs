//! Temporal anti-aliasing, and the scaling to the output resolution.
//!
//! The trace pass moves all camera rays of a frame by one part of a pixel (the
//! jitter, [`jitter`]). The pass `taa` keeps the mean of the pictures of some frames at
//! the output resolution: smooth edges, and with `render_scale` below 1 more detail
//! than one frame has. See `shaders/taa.comp` for the steps.
//!
//! The pass is the one step between the internal resolution and the output
//! resolution: `denoised` in, `resolved` out. It is the seam for an upscaler (FSR,
//! XeSS, MetalFX, DLSS). Such an upscaler is a second pass with the same images, in
//! place of this one: `denoised`, `motion`, the distance in `normal`, and the jitter of
//! the frame are its inputs too. No trait and no vendor code until one exists.

use maki_gpu::{Access, ComputePipelineHandle, Format, Gpu, SamplerDesc, SamplerHandle};

use crate::graph::{FrameGraph, Pass, PassContext, ResourceId, SizeMode, TransientDesc};
use crate::passes::{groups, push_constants};
use crate::{Accumulation, ExtractedFrame, shader_spirv};

/// The jitter repeats after this many frames.
const JITTER_FRAMES: u64 = 16;
/// The history of a pixel must be this many standard deviations near the mean of the
/// samples around it. Lower: less ghost behind a thing that moves, more flicker.
const HISTORY_CLIP: f32 = 1.25;

/// The part of a new frame in the picture for a point that moves on the screen, and
/// for a pixel whose light is new. In motion the history is read between pixels in
/// each frame, which softens it: a larger part of the new frame keeps the picture
/// sharp, and edges that move show less of their stairs than edges at rest.
const MOTION_BLEND: f32 = 0.3;

/// The offset of all camera rays of a frame from the pixel centers, in pixels: X and
/// Y, each from -0.5 to 0.5.
///
/// The Halton sequence with the bases 2 and 3: each new point falls into the largest
/// gap that the points before left, so any run of frames covers the pixel evenly.
pub(crate) fn jitter(frame: u64) -> [f32; 2] {
    // From 1: the point 0 of the sequence is the corner (0, 0).
    let index = (frame % JITTER_FRAMES) as u32 + 1;
    [halton(index, 2) - 0.5, halton(index, 3) - 0.5]
}

/// Point `index` of the Halton sequence: the digits of `index` in `base`, mirrored at
/// the point. From 0.0 to 1.0.
fn halton(mut index: u32, base: u32) -> f32 {
    let (mut place, mut value) = (1.0, 0.0);
    while index > 0 {
        place /= base as f32;
        value += place * (index % base) as f32;
        index /= base;
    }
    value
}

/// Adds the pass `taa` at the end of the graph. `denoised`, `motion`, `normal`,
/// `previous_normal` (`normal` of the frame before), and `direct` (the mean of the
/// direct light of the denoiser, with its sample count) are at the internal resolution.
/// Returns `resolved`: the picture at the output resolution.
pub(crate) fn add_pass(
    gpu: &mut Gpu,
    graph: &mut FrameGraph,
    denoised: ResourceId,
    motion: ResourceId,
    [normal, previous_normal]: [ResourceId; 2],
    direct: ResourceId,
) -> ResourceId {
    // The output is the history of the next frame.
    let (resolved, previous) = graph.add_history(
        TransientDesc {
            name: "resolved",
            format: Format::Rgba16Float,
            size: SizeMode::Output,
        },
        "resolved_previous",
    );
    graph.add_pass(Taa {
        pipeline: gpu
            .create_compute_pipeline(shader_spirv!("taa.comp"))
            .expect("taa pipeline"),
        // The place of a point in the frame before is between pixels.
        sampler: gpu
            .create_sampler(&SamplerDesc {
                linear: true,
                repeat: false,
                mipmaps: false,
            })
            .expect("taa sampler"),
        denoised,
        motion,
        normal,
        previous_normal,
        direct,
        previous,
        resolved,
    });
    resolved
}

struct Taa {
    pipeline: ComputePipelineHandle,
    sampler: SamplerHandle,
    denoised: ResourceId,
    motion: ResourceId,
    normal: ResourceId,
    previous_normal: ResourceId,
    direct: ResourceId,
    previous: ResourceId,
    resolved: ResourceId,
}

impl Pass for Taa {
    fn name(&self) -> &'static str {
        "taa"
    }

    fn accesses(&self) -> Vec<(ResourceId, Access)> {
        vec![
            (self.denoised, Access::StorageRead),
            (self.motion, Access::StorageRead),
            (self.normal, Access::StorageRead),
            (self.previous_normal, Access::StorageRead),
            (self.direct, Access::StorageRead),
            (self.previous, Access::Sampled),
            (self.resolved, Access::StorageWrite),
        ]
    }

    fn execute(&mut self, ctx: &mut PassContext) {
        let accumulation = ctx.world.resource::<Accumulation>();
        // No new picture in this frame: `resolved` of the frame before stays.
        if !accumulation.sampling() {
            return;
        }
        let frame = ctx.world.resource::<ExtractedFrame>();
        let (blend, clip) = if accumulation.reset_frame() || !frame.taa {
            // No history: the picture is the samples of this frame.
            (1.0, 0.0)
        } else if frame.taa_blend > 0.0 {
            (frame.taa_blend, HISTORY_CLIP)
        } else {
            // The reference mode: the plain mean of all frames since the last change.
            // Only for a scene at rest: nothing tests the history.
            let frames = accumulation.samples() / accumulation.frame_samples().max(1);
            (1.0 / frames.max(1) as f32, 0.0)
        };
        let sampler = ctx
            .gpu
            .sampler_index(self.sampler)
            .expect("taa sampler is alive");
        let push = push_constants(&[
            ctx.bindless_index(self.denoised),
            ctx.bindless_index(self.motion),
            ctx.bindless_index(self.previous),
            sampler,
            ctx.bindless_index(self.resolved),
            blend.to_bits(),
            frame.jitter[0].to_bits(),
            frame.jitter[1].to_bits(),
            clip.to_bits(),
            ctx.bindless_index(self.normal),
            ctx.bindless_index(self.previous_normal),
            MOTION_BLEND.to_bits(),
            ctx.bindless_index(self.direct),
            (accumulation.frame_samples() as f32).to_bits(),
        ]);
        ctx.gpu
            .dispatch_compute(self.pipeline, &push, groups(ctx.extent(self.resolved)))
            .expect("taa dispatch");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_jitter_covers_the_pixel_and_repeats() {
        assert_eq!(halton(1, 2), 0.5);
        assert_eq!(halton(2, 2), 0.25);
        assert_eq!(halton(3, 2), 0.75);
        assert!((halton(2, 3) - 2.0 / 3.0).abs() < 1e-6);
        let points: Vec<_> = (0..JITTER_FRAMES).map(jitter).collect();
        // Each point is in the pixel, and each quarter of the pixel has a quarter of
        // the points.
        assert!(points.iter().flatten().all(|v| (-0.5..0.5).contains(v)));
        for (x_sign, y_sign) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
            let in_quarter = |p: &&[f32; 2]| p[0] * x_sign >= 0.0 && p[1] * y_sign >= 0.0;
            let count = points.iter().filter(in_quarter).count();
            assert!((3..=5).contains(&count), "{count}");
        }
        assert_eq!(jitter(3), jitter(3 + JITTER_FRAMES));
        assert_ne!(jitter(3), jitter(4));
    }
}
