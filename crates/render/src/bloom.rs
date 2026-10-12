//! Bloom: the glow around the bright parts of the picture (the lamp, its reflections).
//!
//! The passes `bloom_down_1` to `_N` make the picture smaller, each one to half the
//! size of the one before. The passes `bloom_up_(N-1)` to `_1` make it larger again
//! and add the image of each size. The result has half the output size and is the sum
//! of `LEVELS` blurs, from narrow to wide. The pass `tonemap` blends
//! `result / LEVELS` into the picture, by `bloom` of the config. See
//! `shaders/bloom.comp`.

use maki_gpu::{Access, ComputePipelineHandle, Format, Gpu, SamplerDesc, SamplerHandle};

use crate::graph::{FrameGraph, Pass, PassContext, ResourceId, SizeMode, TransientDesc};
use crate::passes::{groups, push_constants};
use crate::{Accumulation, ExtractedFrame, shader_spirv};

/// The number of sizes. The smallest image has `1 / 2^LEVELS` of the output size: with
/// 5, the glow of a point is about 60 pixels wide.
pub(crate) const LEVELS: usize = 5;

const DOWN_NAMES: [&str; LEVELS] = [
    "bloom_down_1",
    "bloom_down_2",
    "bloom_down_3",
    "bloom_down_4",
    "bloom_down_5",
];
const DOWN_IMAGES: [&str; LEVELS] = [
    "bloom_half",
    "bloom_4th",
    "bloom_8th",
    "bloom_16th",
    "bloom_32nd",
];
const UP_NAMES: [&str; LEVELS - 1] = ["bloom_up_1", "bloom_up_2", "bloom_up_3", "bloom_up_4"];
const UP_IMAGES: [&str; LEVELS - 1] = [
    "bloom_sum_half",
    "bloom_sum_4th",
    "bloom_sum_8th",
    "bloom_sum_16th",
];

/// Adds the bloom passes at the end of the graph. `picture` is at the output
/// resolution. Returns the result: divide it by `LEVELS`.
pub(crate) fn add_passes(gpu: &mut Gpu, graph: &mut FrameGraph, picture: ResourceId) -> ResourceId {
    let pipeline = gpu
        .create_compute_pipeline(shader_spirv!("bloom.comp"))
        .expect("bloom pipeline");
    let sampler = gpu
        .create_sampler(&SamplerDesc {
            linear: true,
            repeat: false,
        })
        .expect("bloom sampler");
    let image = |graph: &mut FrameGraph, name, level: usize| {
        graph.add_transient(TransientDesc {
            name,
            format: Format::Rgba16Float,
            size: SizeMode::OutputDiv(2 << level),
        })
    };
    // Smaller: the picture, then each image to the next one.
    let mut down = Vec::with_capacity(LEVELS);
    for level in 0..LEVELS {
        let dst = image(graph, DOWN_IMAGES[level], level);
        graph.add_pass(BloomPass {
            name: DOWN_NAMES[level],
            pipeline,
            sampler,
            mode: if level == 0 { 0 } else { 1 },
            src: down.last().copied().unwrap_or(picture),
            add: None,
            dst,
        });
        down.push(dst);
    }
    // Larger: from the smallest image up, each step plus the image of its size.
    let mut sum = down[LEVELS - 1];
    for level in (0..LEVELS - 1).rev() {
        let dst = image(graph, UP_IMAGES[level], level);
        graph.add_pass(BloomPass {
            name: UP_NAMES[level],
            pipeline,
            sampler,
            mode: 2,
            src: sum,
            add: Some(down[level]),
            dst,
        });
        sum = dst;
    }
    sum
}

/// One step of the chain.
struct BloomPass {
    name: &'static str,
    pipeline: ComputePipelineHandle,
    sampler: SamplerHandle,
    /// A `MODE_*` value of the shader.
    mode: u32,
    src: ResourceId,
    /// For a step to a larger size: the image of that size.
    add: Option<ResourceId>,
    dst: ResourceId,
}

impl Pass for BloomPass {
    fn name(&self) -> &'static str {
        self.name
    }

    fn accesses(&self) -> Vec<(ResourceId, Access)> {
        let sampled = [Some(self.src), self.add].into_iter().flatten();
        sampled
            .map(|image| (image, Access::Sampled))
            .chain([(self.dst, Access::StorageWrite)])
            .collect()
    }

    fn execute(&mut self, ctx: &mut PassContext) {
        // No new picture in this frame, or no bloom: nothing to do.
        if !ctx.world.resource::<Accumulation>().sampling()
            || ctx.world.resource::<ExtractedFrame>().look.bloom <= 0.0
        {
            return;
        }
        let sampler = ctx
            .gpu
            .sampler_index(self.sampler)
            .expect("bloom sampler is alive");
        let push = push_constants(&[
            ctx.bindless_index(self.src),
            sampler,
            ctx.bindless_index(self.dst),
            self.mode,
            ctx.bindless_index(self.add.unwrap_or(self.src)),
        ]);
        ctx.gpu
            .dispatch_compute(self.pipeline, &push, groups(ctx.extent(self.dst)))
            .expect("bloom dispatch");
    }
}
