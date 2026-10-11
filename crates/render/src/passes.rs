//! The passes of the frame graph, in frame order:
//! build_tlas -> trace -> tonemap -> present.
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
use crate::{ExtractedCamera, ExtractedInstances, GpuMeshes, shader_spirv};

/// The TLAS has room for this many instances. More are not drawn; an error is logged.
pub const MAX_INSTANCES: u32 = 4096;

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
        hdr,
        depth,
        normal,
        motion,
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

/// Builds the TLAS from the extracted instances, in each frame.
///
/// A full build each frame costs GPU time also when nothing moves. In return, an object
/// that moves, appears, or goes away needs no other code path: it is only a different
/// instance list. The BLAS of each mesh, the expensive part, is built one time.
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

/// Traces one ray per pixel from the camera. Shows the world normal of the hit as color.
/// The path tracer replaces the color later; the other three images stay.
struct Trace {
    pipeline: ComputePipelineHandle,
    hdr: ResourceId,
    depth: ResourceId,
    normal: ResourceId,
    motion: ResourceId,
}

impl Pass for Trace {
    fn name(&self) -> &'static str {
        "trace"
    }

    fn accesses(&self) -> Vec<(ResourceId, Access)> {
        [self.hdr, self.depth, self.normal, self.motion]
            .map(|image| (image, Access::StorageWrite))
            .to_vec()
    }

    fn execute(&mut self, ctx: &mut PassContext) {
        let camera = ctx.world.resource::<ExtractedCamera>().0;
        // The picture is scaled to the window, so the rays use the shape of the window
        // and not that of the internal image. A resize thus does not stretch the scene.
        let (width, height) = ctx.gpu.swapchain_extent();
        let [right, up, forward] = camera.ray_basis(width as f32 / height as f32);
        let mesh_table = ctx
            .gpu
            .buffer_index(ctx.world.resource::<GpuMeshes>().table())
            .expect("the mesh table is alive");
        // Field order as in the `Push` block of the shader: a vec3, then a uint.
        let vec3_uint = |v: Vec3, u: u32| [v.x.to_bits(), v.y.to_bits(), v.z.to_bits(), u];
        let mut fields = [
            vec3_uint(camera.position, mesh_table),
            vec3_uint(right, ctx.bindless_index(self.hdr)),
            vec3_uint(up, ctx.bindless_index(self.depth)),
            vec3_uint(forward, ctx.bindless_index(self.normal)),
        ]
        .concat();
        fields.push(ctx.bindless_index(self.motion));
        ctx.gpu
            .dispatch_compute(
                self.pipeline,
                &push_constants(&fields),
                groups(ctx.extent(self.hdr)),
            )
            .expect("trace dispatch");
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
