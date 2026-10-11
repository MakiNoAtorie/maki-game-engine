//! The denoiser: a spatial filter between the passes `accumulate` and `tonemap`.
//!
//! A picture of few samples is noisy. The filter gives each pixel the mean of the
//! pixels around it that show the same surface in the same lighting. It is the spatial
//! part of SVGF (Schied et al., "Spatiotemporal Variance-Guided Filtering", 2017):
//!
//! 1. `denoise_prepare` divides the accumulated picture by the surface color (the
//!    `albedo` image of the trace pass) and measures the noise around each pixel.
//! 2. `denoise_filter_1` to `_N` blur, each with a wider pattern. The `normal` image
//!    (normal and distance) and the measured noise tell which neighbors count.
//!    The last one multiplies by the surface color again and writes `denoised`.
//!
//! The filter turns itself off: the noise in the accumulated picture falls with each
//! sample, and with no noise only equal neighbors count.
//!
//! All passes are at the internal resolution. An upscaler comes after them.
//! The shaders are `shaders/denoise_prepare.comp` and `shaders/denoise_filter.comp`.
// ponytail: one signal. Direct light and bounce light are filtered together, so a
// sharp shadow edge in a noisy area gets soft. Filter the two apart (two images from
// the trace pass) when that shows.
// ponytail: mirror and glass pixels are filtered with the first diffuse surface that
// the path of the first sample found behind them. Glass picks reflection or
// refraction at random, so that surface can change from frame to frame.

use maki_gpu::{Access, ComputePipelineHandle, Format, Gpu};

use crate::graph::{FrameGraph, Pass, PassContext, ResourceId, SizeMode, TransientDesc};
use crate::passes::{groups, push_constants};
use crate::{Accumulation, shader_spirv};

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

/// Adds the denoiser passes at the end of the graph: call after the pass that writes
/// `accumulated`. Returns the image with the result. `filter_passes` is 1 or more.
pub(crate) fn add_passes(
    gpu: &mut Gpu,
    graph: &mut FrameGraph,
    accumulated: ResourceId,
    normal: ResourceId,
    albedo: ResourceId,
    filter_passes: u32,
) -> ResourceId {
    let mut image = |name| {
        graph.add_transient(TransientDesc {
            name,
            // 16 bits are sufficient: these images are made again in each frame.
            format: Format::Rgba16Float,
            size: SizeMode::Internal,
        })
    };
    // The filter passes read one work image and write the other, in turns.
    let work = [image("denoise_work_a"), image("denoise_work_b")];
    let denoised = image("denoised");

    graph.add_pass(DenoisePrepare {
        pipeline: gpu
            .create_compute_pipeline(shader_spirv!("denoise_prepare.comp"))
            .expect("denoise_prepare pipeline"),
        accumulated,
        normal,
        albedo,
        dst: work[0],
    });
    let pipeline = gpu
        .create_compute_pipeline(shader_spirv!("denoise_filter.comp"))
        .expect("denoise_filter pipeline");
    for index in 0..filter_passes.min(MAX_DENOISE_PASSES) as usize {
        let last = index + 1 == filter_passes as usize;
        graph.add_pass(DenoiseFilter {
            name: FILTER_NAMES[index],
            pipeline,
            src: work[index % 2],
            dst: if last {
                denoised
            } else {
                work[(index + 1) % 2]
            },
            normal,
            albedo,
            step: 1 << index,
            last,
        });
    }
    denoised
}

struct DenoisePrepare {
    pipeline: ComputePipelineHandle,
    accumulated: ResourceId,
    normal: ResourceId,
    albedo: ResourceId,
    dst: ResourceId,
}

impl Pass for DenoisePrepare {
    fn name(&self) -> &'static str {
        "denoise_prepare"
    }

    fn accesses(&self) -> Vec<(ResourceId, Access)> {
        vec![
            (self.accumulated, Access::StorageRead),
            (self.normal, Access::StorageRead),
            (self.albedo, Access::StorageRead),
            (self.dst, Access::StorageWrite),
        ]
    }

    fn execute(&mut self, ctx: &mut PassContext) {
        // No new sample in this frame: `denoised` of the frame before is still correct.
        if !ctx.world.resource::<Accumulation>().sampling() {
            return;
        }
        let push = push_constants(&[
            ctx.bindless_index(self.accumulated),
            ctx.bindless_index(self.normal),
            ctx.bindless_index(self.albedo),
            ctx.bindless_index(self.dst),
        ]);
        ctx.gpu
            .dispatch_compute(self.pipeline, &push, groups(ctx.extent(self.dst)))
            .expect("denoise_prepare dispatch");
    }
}

struct DenoiseFilter {
    name: &'static str,
    pipeline: ComputePipelineHandle,
    src: ResourceId,
    dst: ResourceId,
    normal: ResourceId,
    albedo: ResourceId,
    /// Pixels between two points of the filter pattern.
    step: u32,
    /// True: this pass writes the final picture.
    last: bool,
}

impl Pass for DenoiseFilter {
    fn name(&self) -> &'static str {
        self.name
    }

    fn accesses(&self) -> Vec<(ResourceId, Access)> {
        vec![
            (self.src, Access::StorageRead),
            (self.normal, Access::StorageRead),
            (self.albedo, Access::StorageRead),
            (self.dst, Access::StorageWrite),
        ]
    }

    fn execute(&mut self, ctx: &mut PassContext) {
        if !ctx.world.resource::<Accumulation>().sampling() {
            return;
        }
        let push = push_constants(&[
            ctx.bindless_index(self.src),
            ctx.bindless_index(self.dst),
            ctx.bindless_index(self.normal),
            ctx.bindless_index(self.albedo),
            self.step,
            u32::from(self.last),
        ]);
        ctx.gpu
            .dispatch_compute(self.pipeline, &push, groups(ctx.extent(self.dst)))
            .expect("denoise_filter dispatch");
    }
}
