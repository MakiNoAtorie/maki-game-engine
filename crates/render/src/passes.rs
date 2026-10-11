//! The passes of the frame graph, in frame order: test_pattern -> tonemap -> present.
//!
//! To add a pass: write a struct with the [`Pass`] trait (here or in a new file), put its
//! shader in `shaders/`, and add it to the graph (`build_graph` below, or
//! `FrameGraph::add_pass_before` from another module).

use maki_gpu::{Access, ComputePipelineHandle, Format, Gpu, SamplerDesc, SamplerHandle};

use crate::graph::{
    BACKBUFFER, FrameGraph, Pass, PassContext, ResourceId, SizeMode, TransientDesc,
};
use crate::{ExtractedFrame, shader_spirv};

/// Adds the built-in images and passes. The order of the `add_pass` calls is the frame order.
pub(crate) fn build_graph(gpu: &mut Gpu, graph: &mut FrameGraph) {
    // Linear color with values above 1.0, at the internal resolution.
    let hdr = graph.add_transient(TransientDesc {
        name: "hdr",
        format: Format::Rgba16Float,
        size: SizeMode::Internal,
    });
    // The frame as the display shows it: sRGB-encoded, 8 bits, at window size.
    let output = graph.add_transient(TransientDesc {
        name: "output",
        format: Format::Rgba8Unorm,
        size: SizeMode::Output,
    });

    graph.add_pass(TestPattern {
        pipeline: gpu
            .create_compute_pipeline(shader_spirv!("test_pattern.comp"))
            .expect("test_pattern pipeline"),
        hdr,
    });
    graph.add_pass(Tonemap {
        pipeline: gpu
            .create_compute_pipeline(shader_spirv!("tonemap.comp"))
            .expect("tonemap pipeline"),
        sampler: gpu
            .create_sampler(&SamplerDesc {
                linear: true,
                repeat: false,
            })
            .expect("tonemap sampler"),
        hdr,
        output,
    });
    graph.add_pass(Present { output });
}

/// A push constant block of 32-bit fields, as bytes.
fn push_constants(fields: &[u32]) -> Vec<u8> {
    fields
        .iter()
        .flat_map(|field| field.to_ne_bytes())
        .collect()
}

/// Work group counts for a shader with 8x8 groups: rounded up, so the groups cover the image.
fn groups((width, height): (u32, u32)) -> [u32; 3] {
    [width.div_ceil(8), height.div_ceil(8), 1]
}

/// Writes a moving HDR gradient. Stands in for the path tracer.
struct TestPattern {
    pipeline: ComputePipelineHandle,
    hdr: ResourceId,
}

impl Pass for TestPattern {
    fn name(&self) -> &'static str {
        "test_pattern"
    }

    fn accesses(&self) -> Vec<(ResourceId, Access)> {
        vec![(self.hdr, Access::StorageWrite)]
    }

    fn execute(&mut self, ctx: &mut PassContext) {
        let seconds = ctx.world.resource::<ExtractedFrame>().seconds;
        // Field order as in the `Push` block of the shader.
        let push = push_constants(&[seconds.to_bits(), ctx.bindless_index(self.hdr)]);
        ctx.gpu
            .dispatch_compute(self.pipeline, &push, groups(ctx.extent(self.hdr)))
            .expect("test_pattern dispatch");
    }
}

/// HDR to display color, scaled from the internal resolution to the window size.
struct Tonemap {
    pipeline: ComputePipelineHandle,
    sampler: SamplerHandle,
    hdr: ResourceId,
    output: ResourceId,
}

impl Pass for Tonemap {
    fn name(&self) -> &'static str {
        "tonemap"
    }

    fn accesses(&self) -> Vec<(ResourceId, Access)> {
        vec![
            (self.hdr, Access::Sampled),
            (self.output, Access::StorageWrite),
        ]
    }

    fn execute(&mut self, ctx: &mut PassContext) {
        let sampler = ctx
            .gpu
            .sampler_index(self.sampler)
            .expect("tonemap sampler is alive");
        let push = push_constants(&[
            ctx.bindless_index(self.hdr),
            sampler,
            ctx.bindless_index(self.output),
        ]);
        ctx.gpu
            .dispatch_compute(self.pipeline, &push, groups(ctx.extent(self.output)))
            .expect("tonemap dispatch");
    }
}

/// Copies the output image into the swapchain image. `Gpu::end_frame` shows it.
struct Present {
    output: ResourceId,
}

impl Pass for Present {
    fn name(&self) -> &'static str {
        "present"
    }

    fn accesses(&self) -> Vec<(ResourceId, Access)> {
        vec![
            (self.output, Access::TransferSrc),
            (BACKBUFFER, Access::TransferDst),
        ]
    }

    fn execute(&mut self, ctx: &mut PassContext) {
        ctx.gpu
            .blit_to_swapchain(ctx.image(self.output))
            .expect("present blit");
    }
}
