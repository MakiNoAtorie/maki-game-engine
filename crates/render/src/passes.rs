//! The passes of the frame graph, in frame order:
//! build_tlas -> trace -> accumulate -> tonemap -> present.
//! Other crates add passes between them: the ui crate puts `ui_overlay` before `present`.
//!
//! build_tlas, trace, and accumulate make one sample of the picture. They do no GPU
//! work when the accumulation has all its samples ([`Accumulation::sampling`]).
//!
//! To add a pass: write a struct with the [`Pass`] trait (here or in a new file), put its
//! shader in `shaders/`, and add it to the graph (`build_graph` below, or
//! `FrameGraph::add_pass_before` from another module).

use glam::Vec3;
use maki_gpu::{
    Access, ComputePipelineHandle, Format, Gpu, SamplerDesc, SamplerHandle, TlasHandle,
    TlasInstance,
};

use crate::graph::{
    BACKBUFFER, FrameGraph, Pass, PassContext, ResourceId, SizeMode, TransientDesc,
};
use crate::{
    Accumulation, ExtractedCamera, ExtractedFrame, ExtractedInstances, ExtractedLights, GpuMeshes,
    Light, shader_spirv,
};

/// The TLAS has room for this many instances. More are not drawn; an error is logged.
pub const MAX_INSTANCES: u32 = 4096;

/// Adds the built-in images and passes. The order of the `add_pass` calls is the frame order.
pub(crate) fn build_graph(gpu: &mut Gpu, graph: &mut FrameGraph) {
    // The light that each pixel gets, from one random path: the sample of this frame.
    // Linear color with values far above 1.0 (the light itself), at the internal resolution.
    let radiance = graph.add_transient(TransientDesc {
        name: "radiance",
        format: Format::Rgba32Float,
        size: SizeMode::Internal,
    });
    // The mean of the `radiance` images of all frames since the last reset.
    let accumulated = graph.add_transient(TransientDesc {
        name: "accumulated",
        format: Format::Rgba32Float,
        size: SizeMode::Internal,
    });
    // The frame as the display shows it: sRGB-encoded, 8 bits, at window size.
    let output = graph.add_transient(TransientDesc {
        name: "output",
        format: Format::Rgba8Unorm,
        size: SizeMode::Output,
    });

    // The next three have no reader yet. Later passes (denoiser, upscaler) find them
    // by name with `FrameGraph::resource`.
    // Distance from the camera to the surface along the ray, in meters.
    let depth = graph.add_transient(TransientDesc {
        name: "depth",
        format: Format::R32Float,
        size: SizeMode::Internal,
    });
    // World normal of the surface.
    let normal = graph.add_transient(TransientDesc {
        name: "normal",
        format: Format::Rgba16Float,
        size: SizeMode::Internal,
    });
    // Screen motion of the surface since the last frame.
    let motion = graph.add_transient(TransientDesc {
        name: "motion",
        format: Format::Rg16Float,
        size: SizeMode::Internal,
    });

    graph.add_pass(BuildTlas {
        tlas: gpu.create_tlas(MAX_INSTANCES).expect("TLAS"),
        full_logged: false,
    });
    graph.add_pass(Trace {
        pipeline: gpu
            .create_compute_pipeline(shader_spirv!("trace.comp"))
            .expect("trace pipeline"),
        radiance,
        depth,
        normal,
        motion,
    });
    graph.add_pass(Accumulate {
        pipeline: gpu
            .create_compute_pipeline(shader_spirv!("accumulate.comp"))
            .expect("accumulate pipeline"),
        radiance,
        accumulated,
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
        accumulated,
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

/// Builds the TLAS from the extracted instances, in each frame.
///
/// A full build each frame costs GPU time also when nothing moves. In return, an object
/// that moves, appears, or goes away needs no other code path: it is only a different
/// instance list. The BLAS of each mesh, the expensive part, is built one time.
/// No build when the accumulation is complete: a changed instance list resets it.
// It uses no image, so the graph plans no barrier for it: `Gpu::build_tlas` orders
// itself against the ray queries.
struct BuildTlas {
    tlas: TlasHandle,
    /// True after the "too many instances" error, to log it one time.
    full_logged: bool,
}

impl Pass for BuildTlas {
    fn name(&self) -> &'static str {
        "build_tlas"
    }

    fn accesses(&self) -> Vec<(ResourceId, Access)> {
        Vec::new()
    }

    fn execute(&mut self, ctx: &mut PassContext) {
        if !ctx.world.resource::<Accumulation>().sampling() {
            return;
        }
        let meshes = ctx.world.resource::<GpuMeshes>();
        let extracted = &ctx.world.resource::<ExtractedInstances>().0;
        let mut instances = Vec::with_capacity(extracted.len());
        for instance in extracted {
            // A mesh that is not on the GPU (still loading, or the upload failed) is not drawn.
            let Some(mesh) = meshes.get(instance.mesh) else {
                continue;
            };
            // The first three rows of the matrix, row by row.
            let rows = instance.transform.transpose().to_cols_array();
            instances.push(TlasInstance {
                blas: mesh.blas,
                transform: rows[..12].try_into().expect("12 of 16 values"),
                // On a hit, the shader gets this number and finds the mesh data with it.
                custom_index: mesh.index,
            });
        }
        if instances.len() > MAX_INSTANCES as usize && !self.full_logged {
            self.full_logged = true;
            tracing::error!(
                instances = instances.len(),
                "more than MAX_INSTANCES ({MAX_INSTANCES}) instances: the rest is not drawn"
            );
        }
        instances.truncate(MAX_INSTANCES as usize);
        ctx.gpu
            .build_tlas(self.tlas, &instances)
            .expect("build_tlas");
    }
}

/// The path tracer: follows one random light path per pixel from the camera and writes
/// the light that it brings (`radiance`). See `shaders/trace.comp` for the steps.
struct Trace {
    pipeline: ComputePipelineHandle,
    radiance: ResourceId,
    depth: ResourceId,
    normal: ResourceId,
    motion: ResourceId,
}

impl Pass for Trace {
    fn name(&self) -> &'static str {
        "trace"
    }

    fn accesses(&self) -> Vec<(ResourceId, Access)> {
        [self.radiance, self.depth, self.normal, self.motion]
            .map(|image| (image, Access::StorageWrite))
            .to_vec()
    }

    fn execute(&mut self, ctx: &mut PassContext) {
        // The mean has all its samples: one more would cost GPU time and change nothing.
        if !ctx.world.resource::<Accumulation>().sampling() {
            return;
        }
        let camera = ctx.world.resource::<ExtractedCamera>().0;
        let frame = ctx.world.resource::<ExtractedFrame>();
        // The picture is scaled to the window, so the rays use the shape of the window
        // and not that of the internal image. A resize thus does not stretch the scene.
        // The extracted size and not the swapchain size: the swapchain can change one
        // frame before the window size message, and that frame has no accumulation reset.
        let (width, height) = frame.window_size;
        let [right, up, forward] = camera.ray_basis(width as f32 / height as f32);
        let mesh_table = ctx
            .gpu
            .buffer_index(ctx.world.resource::<GpuMeshes>().table())
            .expect("the mesh table is alive");
        // ponytail: the shader gets one light, in the push constants. For more lights,
        // put them in a buffer (one copy for each frame in flight, because the CPU
        // writes it each frame) and let the shader pick one at random for each shadow ray.
        // No light: a radius of zero makes the light sphere impossible to hit.
        let (light_position, light_radius, light_radiance) =
            ctx.world.resource::<ExtractedLights>().0.first().map_or(
                (Vec3::ZERO, 0.0, Vec3::ZERO),
                |light| {
                    (
                        light.position,
                        light.radius.max(Light::MIN_RADIUS),
                        light.radiance(),
                    )
                },
            );
        // Field order as in the `Push` block of the shader: a vec3, then one 32-bit value.
        let vec3_bits = |v: Vec3, last: u32| [v.x.to_bits(), v.y.to_bits(), v.z.to_bits(), last];
        let mut fields = [
            vec3_bits(camera.position, mesh_table),
            vec3_bits(right, ctx.bindless_index(self.radiance)),
            vec3_bits(up, ctx.bindless_index(self.depth)),
            vec3_bits(forward, ctx.bindless_index(self.normal)),
            vec3_bits(light_position, light_radius.to_bits()),
            vec3_bits(light_radiance, ctx.bindless_index(self.motion)),
        ]
        .concat();
        fields.push(frame.frame);
        ctx.gpu
            .dispatch_compute(
                self.pipeline,
                &push_constants(&fields),
                groups(ctx.extent(self.radiance)),
            )
            .expect("trace dispatch");
    }
}

/// Blends the sample of this frame into the mean of all samples since the last reset.
struct Accumulate {
    pipeline: ComputePipelineHandle,
    radiance: ResourceId,
    accumulated: ResourceId,
}

impl Pass for Accumulate {
    fn name(&self) -> &'static str {
        "accumulate"
    }

    fn accesses(&self) -> Vec<(ResourceId, Access)> {
        vec![
            (self.radiance, Access::StorageRead),
            (self.accumulated, Access::StorageReadWrite),
        ]
    }

    fn execute(&mut self, ctx: &mut PassContext) {
        let accumulation = ctx.world.resource::<Accumulation>();
        // No new sample in this frame: the mean stays as it is.
        if !accumulation.sampling() {
            return;
        }
        let push = push_constants(&[
            ctx.bindless_index(self.radiance),
            ctx.bindless_index(self.accumulated),
            accumulation.samples(),
        ]);
        ctx.gpu
            .dispatch_compute(self.pipeline, &push, groups(ctx.extent(self.accumulated)))
            .expect("accumulate dispatch");
    }
}

/// The accumulated image to display color, scaled from the internal resolution to the
/// window size. Runs in each frame, also when the accumulation is complete: the window
/// needs a picture in each frame, and this pass is cheap.
struct Tonemap {
    pipeline: ComputePipelineHandle,
    sampler: SamplerHandle,
    accumulated: ResourceId,
    output: ResourceId,
}

impl Pass for Tonemap {
    fn name(&self) -> &'static str {
        "tonemap"
    }

    fn accesses(&self) -> Vec<(ResourceId, Access)> {
        vec![
            (self.accumulated, Access::Sampled),
            (self.output, Access::StorageWrite),
        ]
    }

    fn execute(&mut self, ctx: &mut PassContext) {
        let sampler = ctx
            .gpu
            .sampler_index(self.sampler)
            .expect("tonemap sampler is alive");
        let push = push_constants(&[
            ctx.bindless_index(self.accumulated),
            sampler,
            ctx.bindless_index(self.output),
            ctx.world.resource::<ExtractedFrame>().exposure.to_bits(),
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
