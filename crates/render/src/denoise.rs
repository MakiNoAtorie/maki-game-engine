//! The denoiser: from the noisy samples of each frame to the picture.
//!
//! One frame of the path tracer has few samples for each pixel: it is noisy. The
//! denoiser takes the mean of many samples, over time and over the pixels around. The
//! method is that of SVGF (Schied et al., "Spatiotemporal Variance-Guided Filtering",
//! 2017).
//!
//! The trace pass gives three kinds of light for each pixel (see `shaders/trace.glsl`),
//! and the picture is `albedo * (direct + indirect) + specular + emission`:
//! - `direct`: light straight from the lights. Its noise is small (only where a light
//!   is partly hidden), and its shadow edges are sharp: few filter passes.
//! - `indirect`: bounce light. Much noise, and soft by nature: all filter passes.
//! - `specular`: the reflection on glass and on a rough surface (metal, plastic). For
//!   a smooth surface a filter would need the surface in the reflection, which the
//!   pixel does not have: the mean over time only. The reflection of a rough surface
//!   is soft by nature and noisy: a narrow filter (`denoise_specular_passes`).
//! - The light of a lamp that the pixel shows (the emission) has no noise and does not
//!   go through the denoiser: `denoise_resolve` adds it.
//!
//! With one kind of light for all three, the noise of the bounce light opens the
//! filter, and the filter then blurs the shadow edges of the direct light.
//!
//! The passes, between `trace` and `tonemap`:
//!
//! 1. `denoise_temporal`: the mean over time of each kind of light. The `motion`
//!    image tells where each surface point was one frame before, so the mean follows
//!    a moving object and a moving camera. Where the frame before showed another
//!    surface, the pixel starts again. While nothing changes, the mean gets all
//!    samples up to `max_samples`: the picture converges. Where the lighting changes,
//!    a second, short mean (the fast history) shows it, and the long mean follows
//!    (`history_clamp` of the config): see the shader.
//!
//! For the direct light, then for the indirect light (`denoise_direct_*`, `denoise_*`):
//!
//! 2. `prepare`: the noise that is left in the mean of each pixel (a variance).
//! 3. `filter_1` to `_N` (`denoise_direct_passes` and `denoise_passes` of the config):
//!    each one blurs with a wider pattern. The `normal` image (normal and distance)
//!    and the variance tell which neighbors count. With no noise left, only equal
//!    neighbors count: the filter turns itself off. A pixel whose mean has many
//!    samples also gets fewer passes (`denoise_reach` of the config): the filter is
//!    as wide as the noise needs.
//!
//! 4. `denoise_resolve`: puts the three together and writes `denoised`.
//!
//! The means of step 1 and the surface image are history images of the frame graph:
//! the temporal pass reads those of the frame before. The CPU side (`Accumulation`)
//! tells the temporal pass how much history it can keep in this frame, and stops all
//! these passes when the picture is complete.
//!
//! All passes are at the internal resolution. An upscaler comes after them.
//! The shaders are `shaders/denoise_*.comp`.
// ponytail: the history is the mean before the filter passes. SVGF feeds the result
// of the first filter pass back into the history, which is calmer in motion and
// blurs more. Do that if new pixels behind a moving object flicker too much.
// ponytail: the two kinds of light run the filter shader one after the other. One
// shader with two targets would read the surface images one time; do that if the
// time of the filter passes shows a need.

use maki_gpu::{Access, ComputePipelineHandle, Format, Gpu};

use crate::graph::{FrameGraph, Pass, PassContext, ResourceId, SizeMode, TransientDesc};
use crate::passes::{frame_ray_basis, groups, push_constants, vec3_bits};
use crate::{Accumulation, ExtractedFrame, shader_spirv};

/// The largest number of filter passes for one kind of light (`denoise_passes` and
/// `denoise_direct_passes` of the config).
pub const MAX_DENOISE_PASSES: u32 = 6;

/// The kinds of light that the filter passes run on. The numbers are the `signal`
/// values of `shaders/denoise_prepare.comp`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Signal {
    Direct = 0,
    Indirect = 1,
    Specular = 2,
}

impl Signal {
    fn prepare_name(self) -> &'static str {
        match self {
            Signal::Direct => "denoise_direct_prepare",
            Signal::Indirect => "denoise_prepare",
            Signal::Specular => "denoise_specular_prepare",
        }
    }

    fn filter_names(self) -> [&'static str; MAX_DENOISE_PASSES as usize] {
        match self {
            Signal::Direct => [
                "denoise_direct_1",
                "denoise_direct_2",
                "denoise_direct_3",
                "denoise_direct_4",
                "denoise_direct_5",
                "denoise_direct_6",
            ],
            Signal::Indirect => [
                "denoise_filter_1",
                "denoise_filter_2",
                "denoise_filter_3",
                "denoise_filter_4",
                "denoise_filter_5",
                "denoise_filter_6",
            ],
            Signal::Specular => [
                "denoise_specular_1",
                "denoise_specular_2",
                "denoise_specular_3",
                "denoise_specular_4",
                "denoise_specular_5",
                "denoise_specular_6",
            ],
        }
    }

    /// The number of filter passes of this frame.
    fn passes(self, ctx: &PassContext) -> u32 {
        let frame = ctx.world.resource::<ExtractedFrame>();
        let passes = match self {
            Signal::Direct => frame.denoise_direct_passes,
            Signal::Indirect => frame.denoise_passes,
            Signal::Specular => frame.denoise_specular_passes,
        };
        passes.min(MAX_DENOISE_PASSES)
    }
}

/// The images of the trace pass that the denoiser reads.
#[derive(Clone, Copy)]
pub(crate) struct TraceImages {
    pub direct: ResourceId,
    pub indirect: ResourceId,
    pub specular: ResourceId,
    pub albedo: ResourceId,
    pub normal: ResourceId,
    /// `normal` of the frame before.
    pub previous_normal: ResourceId,
    pub motion: ResourceId,
    /// The mean indirect light of the denoiser and that of the frame before
    /// (`indirect_history`). The trace pass reads the second one.
    pub mean_indirect: ResourceId,
    pub previous_mean_indirect: ResourceId,
}

/// Declares the history images of the mean indirect light: `(current, previous)`.
/// Not in `add_passes`: the trace pass reads `previous`, and it is added first.
pub(crate) fn indirect_history(graph: &mut FrameGraph) -> (ResourceId, ResourceId) {
    graph.add_history(
        TransientDesc {
            name: "denoise_indirect",
            // 32-bit floats: see `add_passes`.
            format: Format::Rgba32Float,
            size: SizeMode::Internal,
        },
        "denoise_indirect_previous",
    )
}

/// The images of one kind of light on its way through the filter.
#[derive(Clone, Copy)]
pub(crate) struct SignalImages {
    signal: Signal,
    /// The mean over time, with its sample count in the fourth channel.
    pub mean: ResourceId,
    /// What the filter passes get: the mean, with its variance in the fourth channel.
    pub input: ResourceId,
    /// The filter passes write these two, in turns.
    work: [ResourceId; 2],
}

impl SignalImages {
    /// The image that the last filter pass of this frame wrote. With no filter pass,
    /// that of the prepare pass.
    pub fn filtered(&self, ctx: &PassContext) -> ResourceId {
        match self.signal.passes(ctx) {
            0 => self.input,
            passes => self.work[(passes as usize - 1) % 2],
        }
    }

    /// Each image that `filtered` can give.
    pub fn filtered_candidates(&self) -> [ResourceId; 3] {
        [self.input, self.work[0], self.work[1]]
    }
}

/// The images that the denoiser makes.
#[derive(Clone, Copy)]
pub(crate) struct DenoiseImages {
    /// The picture.
    pub denoised: ResourceId,
    pub direct: SignalImages,
    pub indirect: SignalImages,
    pub specular: SignalImages,
    /// For the direct light in the first two channels, for the indirect light in the
    /// other two: the mean brightness of one frame and the mean of its square.
    pub moments: ResourceId,
}

/// Adds the denoiser passes at the end of the graph: call after the pass that writes
/// the trace images.
///
/// The graph has all `MAX_DENOISE_PASSES` filter passes for each kind of light. Each
/// frame runs the first ones of them, so the counts can change while the game runs.
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
    let (mean_direct, previous_direct) = graph.add_history(
        internal("denoise_direct", Format::Rgba32Float),
        "denoise_direct_previous",
    );
    let (mean_indirect, previous_indirect) = (trace.mean_indirect, trace.previous_mean_indirect);
    let (moments, previous_moments) = graph.add_history(
        internal("denoise_moments", Format::Rgba32Float),
        "denoise_moments_previous",
    );
    let (mean_specular, previous_specular) = graph.add_history(
        internal("denoise_specular", Format::Rgba32Float),
        "denoise_specular_previous",
    );
    // The fast mean of the direct light: few frames, so 16 bits are sufficient.
    let (fast_direct, previous_fast_direct) = graph.add_history(
        internal("denoise_fast_direct", Format::Rgba16Float),
        "denoise_fast_direct_previous",
    );
    // 16 bits are sufficient: these images are made again in each frame.
    // The first filter pass reads `input`. Then the passes read one work image and
    // write the other, in turns. `input` stays as it is, for its debug views.
    let direct = SignalImages {
        signal: Signal::Direct,
        mean: mean_direct,
        input: graph.add_transient(internal("denoise_direct_input", Format::Rgba16Float)),
        work: [
            graph.add_transient(internal("denoise_direct_work_a", Format::Rgba16Float)),
            graph.add_transient(internal("denoise_direct_work_b", Format::Rgba16Float)),
        ],
    };
    let indirect = SignalImages {
        signal: Signal::Indirect,
        mean: mean_indirect,
        input: graph.add_transient(internal("denoise_input", Format::Rgba16Float)),
        work: [
            graph.add_transient(internal("denoise_work_a", Format::Rgba16Float)),
            graph.add_transient(internal("denoise_work_b", Format::Rgba16Float)),
        ],
    };
    let specular = SignalImages {
        signal: Signal::Specular,
        mean: mean_specular,
        input: graph.add_transient(internal("denoise_specular_input", Format::Rgba16Float)),
        work: [
            graph.add_transient(internal("denoise_specular_work_a", Format::Rgba16Float)),
            graph.add_transient(internal("denoise_specular_work_b", Format::Rgba16Float)),
        ],
    };
    let images = DenoiseImages {
        denoised: graph.add_transient(internal("denoised", Format::Rgba16Float)),
        direct,
        indirect,
        specular,
        moments,
    };
    let mut pipeline = |spirv| {
        gpu.create_compute_pipeline(spirv)
            .expect("denoise pipeline")
    };

    graph.add_pass(DenoiseTemporal {
        pipeline: pipeline(shader_spirv!("denoise_temporal.comp")),
        trace,
        previous: [
            previous_direct,
            previous_indirect,
            previous_moments,
            previous_specular,
            previous_fast_direct,
        ],
        current: [
            mean_direct,
            mean_indirect,
            moments,
            mean_specular,
            fast_direct,
        ],
    });
    let prepare = pipeline(shader_spirv!("denoise_prepare.comp"));
    let filter = pipeline(shader_spirv!("denoise_filter.comp"));
    for images in [direct, indirect, specular] {
        graph.add_pass(DenoisePrepare {
            pipeline: prepare,
            images,
            moments,
            normal: trace.normal,
            albedo: trace.albedo,
        });
        for (index, name) in images.signal.filter_names().into_iter().enumerate() {
            graph.add_pass(DenoiseFilter {
                name,
                pipeline: filter,
                signal: images.signal,
                index: index as u32,
                src: if index == 0 {
                    images.input
                } else {
                    images.work[(index - 1) % 2]
                },
                dst: images.work[index % 2],
                normal: trace.normal,
                samples: images.mean,
            });
        }
    }
    graph.add_pass(DenoiseResolve {
        pipeline: pipeline(shader_spirv!("denoise_resolve.comp")),
        images,
        trace,
    });
    images
}

struct DenoiseTemporal {
    pipeline: ComputePipelineHandle,
    trace: TraceImages,
    /// The means of the frame before: direct, indirect, moments, specular, and the
    /// fast mean of the direct light.
    previous: [ResourceId; 5],
    /// The same five, of this frame.
    current: [ResourceId; 5],
}

impl DenoiseTemporal {
    /// The images in the order of the `Push` block of the shader. The pass reads the
    /// first `READ_IMAGES` and writes the rest.
    fn images(&self) -> Vec<ResourceId> {
        let trace = &self.trace;
        let read = [
            trace.direct,
            trace.indirect,
            trace.specular,
            trace.normal,
            trace.previous_normal,
            trace.motion,
        ];
        let all = read.into_iter().chain(self.previous).chain(self.current);
        all.collect()
    }
}

/// See `DenoiseTemporal::images`.
const READ_IMAGES: usize = 11;

impl Pass for DenoiseTemporal {
    fn name(&self) -> &'static str {
        "denoise_temporal"
    }

    fn accesses(&self) -> Vec<(ResourceId, Access)> {
        let access = |index| {
            if index < READ_IMAGES {
                Access::StorageRead
            } else {
                Access::StorageWrite
            }
        };
        let images = self.images().into_iter().enumerate();
        images
            .map(|(index, image)| (image, access(index)))
            .collect()
    }

    fn execute(&mut self, ctx: &mut PassContext) {
        let accumulation = ctx.world.resource::<Accumulation>();
        // No new sample in this frame: the mean stays as it is.
        if !accumulation.sampling() {
            return;
        }
        let frame = ctx.world.resource::<ExtractedFrame>();
        let images = self.images().into_iter();
        let mut push: Vec<u32> = images.map(|image| ctx.bindless_index(image)).collect();
        // After a change of the lighting, the indirect and the specular light keep a
        // short history. The direct light keeps all of it where the fast history
        // agrees with it: the shader tests each pixel.
        let limit = accumulation.history_limit() as f32;
        let direct_limit = if limit > 0.0 && frame.history_clamp > 0.0 {
            frame.max_samples as f32
        } else {
            limit
        };
        push.extend([
            (accumulation.frame_samples() as f32).to_bits(),
            direct_limit.to_bits(),
            limit.to_bits(),
            (frame.max_samples as f32).to_bits(),
            frame.history_clamp.to_bits(),
        ]);
        ctx.gpu
            .dispatch_compute(
                self.pipeline,
                &push_constants(&push),
                groups(ctx.extent(self.current[0])),
            )
            .expect("denoise_temporal dispatch");
    }
}

struct DenoisePrepare {
    pipeline: ComputePipelineHandle,
    images: SignalImages,
    moments: ResourceId,
    normal: ResourceId,
    /// The roughness of the surface is in its fourth channel.
    albedo: ResourceId,
}

impl Pass for DenoisePrepare {
    fn name(&self) -> &'static str {
        self.images.signal.prepare_name()
    }

    fn accesses(&self) -> Vec<(ResourceId, Access)> {
        vec![
            (self.images.mean, Access::StorageRead),
            (self.moments, Access::StorageRead),
            (self.normal, Access::StorageRead),
            (self.albedo, Access::StorageRead),
            (self.images.input, Access::StorageWrite),
        ]
    }

    fn execute(&mut self, ctx: &mut PassContext) {
        let accumulation = ctx.world.resource::<Accumulation>();
        // No new sample in this frame: `denoised` of the frame before is still correct.
        if !accumulation.sampling() {
            return;
        }
        let dst = self.images.input;
        let push = push_constants(&[
            ctx.bindless_index(self.images.mean),
            ctx.bindless_index(self.moments),
            ctx.bindless_index(self.normal),
            ctx.bindless_index(dst),
            (accumulation.frame_samples() as f32).to_bits(),
            self.images.signal as u32,
            ctx.bindless_index(self.albedo),
        ]);
        ctx.gpu
            .dispatch_compute(self.pipeline, &push, groups(ctx.extent(dst)))
            .expect("denoise_prepare dispatch");
    }
}

struct DenoiseFilter {
    name: &'static str,
    pipeline: ComputePipelineHandle,
    signal: Signal,
    /// Number of the pass, from 0. The pattern of pass N has `2^N` pixels between
    /// two of its points.
    index: u32,
    src: ResourceId,
    dst: ResourceId,
    normal: ResourceId,
    /// The mean of this light: the filter reads the sample count of each pixel.
    samples: ResourceId,
}

impl Pass for DenoiseFilter {
    fn name(&self) -> &'static str {
        self.name
    }

    fn accesses(&self) -> Vec<(ResourceId, Access)> {
        vec![
            (self.src, Access::StorageRead),
            (self.normal, Access::StorageRead),
            (self.samples, Access::StorageRead),
            (self.dst, Access::StorageWrite),
        ]
    }

    fn execute(&mut self, ctx: &mut PassContext) {
        if !ctx.world.resource::<Accumulation>().sampling() || self.index >= self.signal.passes(ctx)
        {
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
            ctx.bindless_index(self.samples),
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
    /// The surface color, and the emission in the light images of this frame.
    trace: TraceImages,
}

impl Pass for DenoiseResolve {
    fn name(&self) -> &'static str {
        "denoise_resolve"
    }

    fn accesses(&self) -> Vec<(ResourceId, Access)> {
        let images = &self.images;
        let filtered = [images.direct, images.indirect, images.specular]
            .into_iter()
            .flat_map(|signal| signal.filtered_candidates());
        let trace = &self.trace;
        filtered
            .chain([trace.albedo])
            .chain([trace.direct, trace.indirect, trace.specular])
            .map(|image| (image, Access::StorageRead))
            .chain([(images.denoised, Access::StorageWrite)])
            .collect()
    }

    fn execute(&mut self, ctx: &mut PassContext) {
        if !ctx.world.resource::<Accumulation>().sampling() {
            return;
        }
        let images = &self.images;
        let push = push_constants(&[
            ctx.bindless_index(images.direct.filtered(ctx)),
            ctx.bindless_index(images.indirect.filtered(ctx)),
            ctx.bindless_index(images.specular.filtered(ctx)),
            ctx.bindless_index(self.trace.albedo),
            ctx.bindless_index(images.denoised),
            ctx.bindless_index(self.trace.direct),
            ctx.bindless_index(self.trace.indirect),
            ctx.bindless_index(self.trace.specular),
        ]);
        ctx.gpu
            .dispatch_compute(self.pipeline, &push, groups(ctx.extent(images.denoised)))
            .expect("denoise_resolve dispatch");
    }
}
