//! The denoiser: from the noisy samples of each frame to the picture.
//!
//! One frame of the path tracer has few samples for each pixel: it is noisy. The
//! denoiser takes the mean of many samples, over time and over the pixels around. The
//! method is that of SVGF (Schied et al., "Spatiotemporal Variance-Guided Filtering",
//! 2017). The passes, between `trace` and `tonemap`:
//!
//! 1. `denoise_temporal`: the mean over time. It divides the samples of the frame by
//!    the surface color (the `albedo` image) and blends them into the mean of the
//!    frames before. The `motion` image tells where each surface point was one frame
//!    before, so the mean follows a moving object and a moving camera. Where the frame
//!    before showed another surface, the pixel starts again. While nothing changes,
//!    the mean gets all samples up to `max_samples`: the picture converges.
//! 2. `denoise_prepare`: the noise that is left in the mean of each pixel (a variance).
//! 3. `denoise_filter_1` to `_N` (`denoise_passes` of the config): each one blurs with
//!    a wider pattern. The `normal` image (normal and distance) and the variance tell
//!    which neighbors count. With no noise left, only equal neighbors count: the
//!    filter turns itself off. A pixel whose mean has many samples also gets fewer
//!    passes (`denoise_reach` of the config): the filter is as wide as the noise needs.
//! 4. `denoise_resolve`: multiplies by the surface color again and writes `denoised`.
//!
//! The mean of step 1 and the surface image are history images of the frame graph: the
//! temporal pass reads those of the frame before. The CPU side (`Accumulation`) tells
//! the temporal pass how much history it can keep in this frame, and stops all these
//! passes when the picture is complete.
//!
//! All passes are at the internal resolution. An upscaler comes after them.
//! The shaders are `shaders/denoise_*.comp`.
// ponytail: one signal. Direct light and bounce light are filtered together, so a
// sharp shadow edge in a noisy area gets soft. Filter the two apart (two images from
// the trace pass) when that shows.
// ponytail: the history is the mean before the filter passes. SVGF feeds the result
// of the first filter pass back into the history, which is calmer in motion and
// blurs more. Do that if new pixels behind a moving object flicker too much.

use maki_gpu::{Access, ComputePipelineHandle, Format, Gpu};

use crate::graph::{FrameGraph, Pass, PassContext, ResourceId, SizeMode, TransientDesc};
use crate::passes::{frame_ray_basis, groups, push_constants, vec3_bits};
use crate::{Accumulation, ExtractedFrame, shader_spirv};

/// The largest number of filter passes (`denoise_passes` of the config).
pub const MAX_DENOISE_PASSES: u32 = 6;

const FILTER_NAMES: [&str; MAX_DENOISE_PASSES as usize] = [
    "denoise_filter_1",
    "denoise_filter_2",
    "denoise_filter_3",
    "denoise_filter_4",
    "denoise_filter_5",
    "denoise_filter_6",
];

/// The images of the trace pass that the denoiser reads.
#[derive(Clone, Copy)]
pub(crate) struct TraceImages {
    pub radiance: ResourceId,
    pub albedo: ResourceId,
    pub normal: ResourceId,
    /// `normal` of the frame before.
    pub previous_normal: ResourceId,
    pub motion: ResourceId,
}

/// The images that the denoiser makes.
#[derive(Clone, Copy)]
pub(crate) struct DenoiseImages {
    /// The picture.
    pub denoised: ResourceId,
    /// The mean light of each pixel, with its sample count in the fourth channel.
    pub color: ResourceId,
    /// The mean brightness of one frame and the mean of its square.
    pub moments: ResourceId,
    /// What the filter passes get: the mean light, with its variance in the fourth
    /// channel.
    pub input: ResourceId,
    /// The filter passes write these two, in turns.
    pub work: [ResourceId; 2],
}

impl DenoiseImages {
    /// The image that the last filter pass of this frame wrote. With no filter pass,
    /// that of `denoise_prepare`.
    pub fn filter_output(&self, ctx: &PassContext) -> ResourceId {
        match filter_passes(ctx) {
            0 => self.input,
            passes => self.work[(passes as usize - 1) % 2],
        }
    }
}

/// Adds the denoiser passes at the end of the graph: call after the pass that writes
/// the trace images.
///
/// The graph has all `MAX_DENOISE_PASSES` filter passes. Each frame runs the first
/// `denoise_passes` of them, so the count can change while the game runs.
pub(crate) fn add_passes(
    gpu: &mut Gpu,
    graph: &mut FrameGraph,
    trace: TraceImages,
) -> DenoiseImages {
    let internal = |name, format| TransientDesc {
        name,
        format,
        size: SizeMode::Internal,
    };
    // 32-bit floats: a 16-bit float is too coarse for the small change that a late
    // frame makes in a mean of some hundred samples.
    let (color, previous_color) = graph.add_history(
        internal("denoise_color", Format::Rgba32Float),
        "denoise_color_previous",
    );
    let (moments, previous_moments) = graph.add_history(
        internal("denoise_moments", Format::Rgba32Float),
        "denoise_moments_previous",
    );
    // 16 bits are sufficient: these images are made again in each frame.
    // The first filter pass reads `input`. Then the passes read one work image and
    // write the other, in turns. `input` stays as it is, for its debug views.
    let input = graph.add_transient(internal("denoise_input", Format::Rgba16Float));
    let work = [
        graph.add_transient(internal("denoise_work_a", Format::Rgba16Float)),
        graph.add_transient(internal("denoise_work_b", Format::Rgba16Float)),
    ];
    let denoised = graph.add_transient(internal("denoised", Format::Rgba16Float));
    let mut pipeline = |spirv| {
        gpu.create_compute_pipeline(spirv)
            .expect("denoise pipeline")
    };

    graph.add_pass(DenoiseTemporal {
        pipeline: pipeline(shader_spirv!("denoise_temporal.comp")),
        trace,
        previous_color,
        previous_moments,
        color,
        moments,
    });
    graph.add_pass(DenoisePrepare {
        pipeline: pipeline(shader_spirv!("denoise_prepare.comp")),
        color,
        moments,
        normal: trace.normal,
        dst: input,
    });
    let filter = pipeline(shader_spirv!("denoise_filter.comp"));
    for (index, name) in FILTER_NAMES.into_iter().enumerate() {
        graph.add_pass(DenoiseFilter {
            name,
            pipeline: filter,
            index: index as u32,
            src: if index == 0 {
                input
            } else {
                work[(index - 1) % 2]
            },
            dst: work[index % 2],
            normal: trace.normal,
            color,
        });
    }
    let images = DenoiseImages {
        denoised,
        color,
        moments,
        input,
        work,
    };
    graph.add_pass(DenoiseResolve {
        pipeline: pipeline(shader_spirv!("denoise_resolve.comp")),
        images,
        normal: trace.normal,
        albedo: trace.albedo,
    });
    images
}

/// The number of filter passes of this frame.
fn filter_passes(ctx: &PassContext) -> u32 {
    let passes = ctx.world.resource::<ExtractedFrame>().denoise_passes;
    passes.min(MAX_DENOISE_PASSES)
}

struct DenoiseTemporal {
    pipeline: ComputePipelineHandle,
    trace: TraceImages,
    previous_color: ResourceId,
    previous_moments: ResourceId,
    color: ResourceId,
    moments: ResourceId,
}

impl Pass for DenoiseTemporal {
    fn name(&self) -> &'static str {
        "denoise_temporal"
    }

    fn accesses(&self) -> Vec<(ResourceId, Access)> {
        let trace = &self.trace;
        let read = [
            trace.radiance,
            trace.albedo,
            trace.normal,
            trace.previous_normal,
            trace.motion,
            self.previous_color,
            self.previous_moments,
        ];
        let write = [self.color, self.moments];
        let read = read.map(|image| (image, Access::StorageRead));
        let write = write.map(|image| (image, Access::StorageWrite));
        read.into_iter().chain(write).collect()
    }

    fn execute(&mut self, ctx: &mut PassContext) {
        let accumulation = ctx.world.resource::<Accumulation>();
        // No new sample in this frame: the mean stays as it is.
        if !accumulation.sampling() {
            return;
        }
        let frame = ctx.world.resource::<ExtractedFrame>();
        let trace = &self.trace;
        let images = [
            trace.radiance,
            trace.albedo,
            trace.normal,
            trace.previous_normal,
            trace.motion,
            self.previous_color,
            self.previous_moments,
            self.color,
            self.moments,
        ];
        let mut push = images.map(|image| ctx.bindless_index(image)).to_vec();
        push.extend([
            (accumulation.frame_samples() as f32).to_bits(),
            (accumulation.history_limit() as f32).to_bits(),
            (frame.max_samples as f32).to_bits(),
        ]);
        ctx.gpu
            .dispatch_compute(
                self.pipeline,
                &push_constants(&push),
                groups(ctx.extent(self.color)),
            )
            .expect("denoise_temporal dispatch");
    }
}

struct DenoisePrepare {
    pipeline: ComputePipelineHandle,
    color: ResourceId,
    moments: ResourceId,
    normal: ResourceId,
    dst: ResourceId,
}

impl Pass for DenoisePrepare {
    fn name(&self) -> &'static str {
        "denoise_prepare"
    }

    fn accesses(&self) -> Vec<(ResourceId, Access)> {
        vec![
            (self.color, Access::StorageRead),
            (self.moments, Access::StorageRead),
            (self.normal, Access::StorageRead),
            (self.dst, Access::StorageWrite),
        ]
    }

    fn execute(&mut self, ctx: &mut PassContext) {
        let accumulation = ctx.world.resource::<Accumulation>();
        // No new sample in this frame: `denoised` of the frame before is still correct.
        if !accumulation.sampling() {
            return;
        }
        let push = push_constants(&[
            ctx.bindless_index(self.color),
            ctx.bindless_index(self.moments),
            ctx.bindless_index(self.normal),
            ctx.bindless_index(self.dst),
            (accumulation.frame_samples() as f32).to_bits(),
        ]);
        ctx.gpu
            .dispatch_compute(self.pipeline, &push, groups(ctx.extent(self.dst)))
            .expect("denoise_prepare dispatch");
    }
}

struct DenoiseFilter {
    name: &'static str,
    pipeline: ComputePipelineHandle,
    /// Number of the pass, from 0. The pattern of pass N has `2^N` pixels between
    /// two of its points.
    index: u32,
    src: ResourceId,
    dst: ResourceId,
    normal: ResourceId,
    /// The mean of the temporal pass: the filter reads the sample count of each pixel.
    color: ResourceId,
}

impl Pass for DenoiseFilter {
    fn name(&self) -> &'static str {
        self.name
    }

    fn accesses(&self) -> Vec<(ResourceId, Access)> {
        vec![
            (self.src, Access::StorageRead),
            (self.normal, Access::StorageRead),
            (self.color, Access::StorageRead),
            (self.dst, Access::StorageWrite),
        ]
    }

    fn execute(&mut self, ctx: &mut PassContext) {
        if !ctx.world.resource::<Accumulation>().sampling() || self.index >= filter_passes(ctx) {
            return;
        }
        // The camera rays: the filter computes the point that each pixel shows.
        let [right, up, forward] = frame_ray_basis(ctx.world);
        let mut push = [
            vec3_bits(right, ctx.bindless_index(self.src)),
            vec3_bits(up, ctx.bindless_index(self.dst)),
            vec3_bits(forward, ctx.bindless_index(self.normal)),
        ]
        .concat();
        push.extend([
            1 << self.index,
            ctx.bindless_index(self.color),
            ctx.world
                .resource::<ExtractedFrame>()
                .denoise_reach
                .to_bits(),
        ]);
        let push = push_constants(&push);
        ctx.gpu
            .dispatch_compute(self.pipeline, &push, groups(ctx.extent(self.dst)))
            .expect("denoise_filter dispatch");
    }
}

struct DenoiseResolve {
    pipeline: ComputePipelineHandle,
    images: DenoiseImages,
    normal: ResourceId,
    albedo: ResourceId,
}

impl Pass for DenoiseResolve {
    fn name(&self) -> &'static str {
        "denoise_resolve"
    }

    fn accesses(&self) -> Vec<(ResourceId, Access)> {
        vec![
            (self.images.input, Access::StorageRead),
            (self.images.work[0], Access::StorageRead),
            (self.images.work[1], Access::StorageRead),
            (self.normal, Access::StorageRead),
            (self.albedo, Access::StorageRead),
            (self.images.denoised, Access::StorageWrite),
        ]
    }

    fn execute(&mut self, ctx: &mut PassContext) {
        if !ctx.world.resource::<Accumulation>().sampling() {
            return;
        }
        let dst = self.images.denoised;
        let push = push_constants(&[
            ctx.bindless_index(self.images.filter_output(ctx)),
            ctx.bindless_index(dst),
            ctx.bindless_index(self.normal),
            ctx.bindless_index(self.albedo),
        ]);
        ctx.gpu
            .dispatch_compute(self.pipeline, &push, groups(ctx.extent(dst)))
            .expect("denoise_resolve dispatch");
    }
}
