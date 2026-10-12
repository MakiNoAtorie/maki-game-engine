//! ReSTIR GI: reuse of bounce samples over time and over pixels.
//!
//! With `restir_gi` of the config, the trace pass does not add the bounce light of a
//! pixel to the `indirect` image. It writes the sample itself (the point that the
//! bounce ray found, and the light from there) to two images. The pass `restir_gi`
//! merges it with the samples of the frame before (reservoirs, three history
//! images) and adds the light of the chosen sample to `indirect`. See
//! `shaders/restir_gi.comp` for the steps. The denoiser gets `indirect` as before.

use maki_gpu::{Access, ComputePipelineHandle, Format, Gpu};

use crate::graph::{FrameGraph, Pass, PassContext, ResourceId, SizeMode, TransientDesc};
use crate::passes::{frame_ray_basis, groups, push_constants, vec3_bits};
use crate::{Accumulation, ExtractedCamera, ExtractedFrame, shader_spirv};

/// A reservoir of the frame before counts as this many samples at most ...
const HISTORY_SAMPLES: f32 = 20.0;
/// ... and as this many while the lighting changes (a light or an object moves).
const CHANGE_HISTORY_SAMPLES: f32 = 4.0;

/// The images of the trace pass and of the denoiser that the pass uses.
pub(crate) struct Inputs {
    pub normal: ResourceId,
    pub previous_normal: ResourceId,
    pub motion: ResourceId,
    pub indirect: ResourceId,
}

/// The two images for the bounce sample of each pixel: the trace pass writes them.
#[derive(Clone, Copy)]
pub(crate) struct SampleImages {
    /// Rgba32Float: the sample point, and a part of its normal.
    pub point: ResourceId,
    /// Rgba16Float: the light from there, and the other part of the normal.
    pub light: ResourceId,
}

/// Declares the images of the bounce samples. Call before the trace pass is added.
pub(crate) fn add_images(graph: &mut FrameGraph) -> SampleImages {
    let internal = |name, format| TransientDesc {
        name,
        format,
        size: SizeMode::Internal,
    };
    SampleImages {
        point: graph.add_transient(internal("gi_point", Format::Rgba32Float)),
        light: graph.add_transient(internal("gi_light", Format::Rgba16Float)),
    }
}

/// Adds the pass at the end of the graph: call after the trace pass is added.
pub(crate) fn add_pass(
    gpu: &mut Gpu,
    graph: &mut FrameGraph,
    samples: SampleImages,
    inputs: Inputs,
) {
    let internal = |name, format| TransientDesc {
        name,
        format,
        size: SizeMode::Internal,
    };
    let (point, previous_point) = graph.add_history(
        internal("gi_reservoir_point", Format::Rgba32Float),
        "gi_reservoir_point_previous",
    );
    let (light, previous_light) = graph.add_history(
        internal("gi_reservoir_light", Format::Rgba16Float),
        "gi_reservoir_light_previous",
    );
    let (weight, previous_weight) = graph.add_history(
        internal("gi_reservoir_weight", Format::Rg16Float),
        "gi_reservoir_weight_previous",
    );
    graph.add_pass(RestirGi {
        pipeline: gpu
            .create_compute_pipeline(shader_spirv!("restir_gi.comp"))
            .expect("restir_gi pipeline"),
        samples,
        inputs,
        previous: [previous_point, previous_light, previous_weight],
        current: [point, light, weight],
    });
}

struct RestirGi {
    pipeline: ComputePipelineHandle,
    samples: SampleImages,
    inputs: Inputs,
    /// The reservoirs of the frame before: point, light, weight.
    previous: [ResourceId; 3],
    /// The same three, of this frame.
    current: [ResourceId; 3],
}

impl Pass for RestirGi {
    fn name(&self) -> &'static str {
        "restir_gi"
    }

    fn accesses(&self) -> Vec<(ResourceId, Access)> {
        let inputs = &self.inputs;
        let read = [
            self.samples.point,
            self.samples.light,
            inputs.normal,
            inputs.previous_normal,
            inputs.motion,
        ];
        let read = read.into_iter().chain(self.previous);
        // The pass reads `indirect` and writes it again: one pixel each, its own.
        let write = [inputs.indirect].into_iter().chain(self.current);
        read.map(|image| (image, Access::StorageRead))
            .chain(write.map(|image| (image, Access::StorageWrite)))
            .collect()
    }

    fn execute(&mut self, ctx: &mut PassContext) {
        let accumulation = ctx.world.resource::<Accumulation>();
        let frame = ctx.world.resource::<ExtractedFrame>();
        if !accumulation.sampling() || !frame.restir_gi {
            return;
        }
        let history = if accumulation.reset_frame() {
            0.0
        } else if accumulation.history_limit() < frame.max_samples {
            // The lighting changes: old samples hold old light.
            CHANGE_HISTORY_SAMPLES
        } else {
            HISTORY_SAMPLES
        };
        let camera = ctx.world.resource::<ExtractedCamera>().current;
        let [right, up, forward] = frame_ray_basis(ctx.world);
        let inputs = &self.inputs;
        let mut push = [
            vec3_bits(camera.position, ctx.bindless_index(self.samples.point)),
            vec3_bits(right, ctx.bindless_index(self.samples.light)),
            vec3_bits(up, ctx.bindless_index(inputs.normal)),
            vec3_bits(forward, ctx.bindless_index(inputs.previous_normal)),
        ]
        .concat();
        push.extend([
            ctx.bindless_index(inputs.motion),
            ctx.bindless_index(inputs.indirect),
        ]);
        push.extend(self.previous.map(|image| ctx.bindless_index(image)));
        push.extend(self.current.map(|image| ctx.bindless_index(image)));
        push.extend([
            // Only the low bits: the number must differ from frame to frame.
            frame.frame as u32,
            (accumulation.frame_samples() as f32).to_bits(),
            history.to_bits(),
            0,
            frame.jitter[0].to_bits(),
            frame.jitter[1].to_bits(),
        ]);
        // The shader has a vec2 at `jitter`: it starts at a multiple of 8 bytes.
        debug_assert_eq!(push.len() - 2, 112 / 4);
        ctx.gpu
            .dispatch_compute(
                self.pipeline,
                &push_constants(&push),
                groups(ctx.extent(inputs.indirect)),
            )
            .expect("restir_gi dispatch");
    }
}
