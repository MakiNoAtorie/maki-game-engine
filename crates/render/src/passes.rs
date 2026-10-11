//! The passes of the frame graph, in frame order:
//! build_tlas -> trace -> the denoiser passes -> tonemap -> present.
//! Other crates add passes between them: the ui crate puts `ui_overlay` before `present`.
//!
//! build_tlas and trace make the samples of the frame, and the denoiser passes (the
//! `denoise` module) make the picture from them. All of them do no GPU work when the
//! picture has all its samples ([`Accumulation::sampling`]).
//!
//! To add a pass: write a struct with the [`Pass`] trait (here or in a new file), put its
//! shader in `shaders/`, and add it to the graph (`build_graph` below, or
//! `FrameGraph::add_pass_before` from another module).

use std::collections::HashMap;

use glam::Vec3;
use maki_gpu::{
    Access, ComputePipelineHandle, Format, Gpu, SamplerDesc, SamplerHandle, TlasHandle,
    TlasInstance,
};

use crate::graph::{
    BACKBUFFER, FrameGraph, Pass, PassContext, ResourceId, SizeMode, TransientDesc,
};
use crate::light::{LightBuffers, light_bytes};
use crate::motion::{MotionBuffers, Projection, instance_bytes, matrix_rows};
use crate::{
    Accumulation, ExtractedCamera, ExtractedFrame, ExtractedInstances, ExtractedLights, GpuMeshes,
    MAX_LIGHTS, denoise, ray_basis, shader_spirv,
};

/// The TLAS has room for this many instances. More are not drawn; an error is logged.
pub const MAX_INSTANCES: u32 = 4096;

/// Adds the built-in images and passes. The order of the `add_pass` calls is the frame order.
pub(crate) fn build_graph(gpu: &mut Gpu, graph: &mut FrameGraph) {
    let internal = |name, format| TransientDesc {
        name,
        format,
        size: SizeMode::Internal,
    };
    // The light that each pixel gets: the mean of the samples of this frame.
    // Linear color with values far above 1.0 (the light itself), at the internal resolution.
    // 16-bit floats: one frame needs no more, and the smaller image is faster to read.
    let radiance = graph.add_transient(internal("radiance", Format::Rgba16Float));
    // The frame as the display shows it: sRGB-encoded, 8 bits, at window size.
    let output = graph.add_transient(TransientDesc {
        name: "output",
        format: Format::Rgba8Unorm,
        size: SizeMode::Output,
    });

    // The surface that each pixel shows, for the denoiser: see `shaders/trace.comp`.
    // World normal in xyz, distance from the camera in w. With history: the denoiser
    // compares it with the surface of the frame before.
    let (normal, previous_normal) =
        graph.add_history(internal("normal", Format::Rgba16Float), "normal_previous");
    // The color of that surface.
    let albedo = graph.add_transient(internal("albedo", Format::Rgba8Unorm));
    // How far the surface of each pixel moved on the screen since the frame before, in
    // pixels: see the `motion` module.
    let motion = graph.add_transient(internal("motion", Format::Rg16Float));

    graph.add_pass(BuildTlas {
        tlas: gpu.create_tlas(MAX_INSTANCES).expect("TLAS"),
        full_logged: false,
        partial_logged: false,
        built: None,
    });
    graph.add_pass(Trace {
        pipeline: gpu
            .create_compute_pipeline(shader_spirv!("trace.comp"))
            .expect("trace pipeline"),
        radiance,
        normal,
        albedo,
        motion,
        lights_logged: false,
    });
    let denoise = denoise::add_passes(
        gpu,
        graph,
        denoise::TraceImages {
            radiance,
            albedo,
            normal,
            previous_normal,
            motion,
        },
    );
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
        hdr: denoise.denoised,
        views: [normal, motion, denoise.color, denoise.moments, radiance],
        output,
    });
    graph.add_pass(Present { output });
}

/// A push constant block of 32-bit fields, as bytes.
pub(crate) fn push_constants(fields: &[u32]) -> Vec<u8> {
    fields
        .iter()
        .flat_map(|field| field.to_ne_bytes())
        .collect()
}

/// Work group counts for a shader with 8x8 groups: rounded up, so the groups cover the image.
pub(crate) fn groups((width, height): (u32, u32)) -> [u32; 3] {
    [width.div_ceil(8), height.div_ceil(8), 1]
}

/// Builds the TLAS from the extracted instances, in each frame where the list differs
/// from the list of the last build.
///
/// A full build, so an object that moves, appears, or goes away needs no other code
/// path: it is only a different instance list. The BLAS of each mesh group, the
/// expensive part, is built one time.
///
/// A BLAS holds all meshes of a model (see the `meshes` module), so the mesh instances
/// of one model with one transform are one TLAS instance. A light is not in the TLAS, so a light that moves costs no build.
/// No build when the accumulation is complete: a changed instance list resets it.
// It uses no image, so the graph plans no barrier for it: `Gpu::build_tlas` orders
// itself against the ray queries.
struct BuildTlas {
    tlas: TlasHandle,
    /// True after the "too many instances" error, to log it one time.
    full_logged: bool,
    /// True after the "a part of a model" warning, to log it one time.
    partial_logged: bool,
    /// The instances that the TLAS holds. `None` before the first build.
    built: Option<Vec<TlasInstance>>,
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
        // The transform of the frame before for each TLAS instance, in the same order.
        let mut previous = Vec::with_capacity(extracted.len());
        // For each TLAS instance of a group of more than one mesh: how many mesh
        // instances asked for it.
        let mut group_counts: HashMap<(maki_gpu::BlasHandle, [u32; 12]), u32> = HashMap::new();
        for instance in extracted {
            // A mesh that is not on the GPU (still loading, or the upload failed) is not drawn.
            let Some(mesh) = meshes.get(instance.mesh) else {
                continue;
            };
            let transform = matrix_rows(instance.transform);
            if mesh.group_len > 1 {
                let count = group_counts
                    .entry((mesh.blas, transform.map(f32::to_bits)))
                    .or_default();
                *count += 1;
                // Another mesh of the group, with this transform, made the instance.
                if *count > 1 {
                    continue;
                }
            }
            instances.push(TlasInstance {
                blas: mesh.blas,
                transform,
                // On a hit, the shader gets this number and finds the mesh data with it.
                custom_index: mesh.group_first,
            });
            previous.push(matrix_rows(instance.previous_transform));
        }
        // ponytail: the BLAS of a model draws all its meshes. If the game draws only a
        // part of a model, or moves one of its meshes alone, the rest shows too. Give
        // such a mesh its own BLAS (a flag of the model asset) when objects can move.
        let partial = |((blas, _), count): (&(maki_gpu::BlasHandle, _), &u32)| {
            meshes.get_group_len(*blas) > *count
        };
        if !self.partial_logged && group_counts.iter().any(partial) {
            self.partial_logged = true;
            tracing::warn!("an instance of a part of a model draws the full model");
        }
        if instances.len() > MAX_INSTANCES as usize && !self.full_logged {
            self.full_logged = true;
            tracing::error!(
                instances = instances.len(),
                "more than MAX_INSTANCES ({MAX_INSTANCES}) instances: the rest is not drawn"
            );
        }
        instances.truncate(MAX_INSTANCES as usize);
        previous.truncate(MAX_INSTANCES as usize);
        // In each frame, also with no build: an instance that stopped has the same
        // transform as in the last build, and its previous transform is now that one.
        if !previous.is_empty() {
            let buffer = ctx.world.resource::<MotionBuffers>().instances[ctx.gpu.frame_slot()];
            ctx.gpu
                .write_buffer(buffer, 0, &instance_bytes(&previous))
                .expect("instance buffer write");
        }
        // A new BLAS has a new handle, so a mesh change also makes the lists differ.
        if self.built.as_ref() == Some(&instances) {
            return;
        }
        ctx.gpu
            .build_tlas(self.tlas, &instances)
            .expect("build_tlas");
        self.built = Some(instances);
    }
}

/// The path tracer: follows random light paths from the camera for each pixel and
/// writes the mean of the light that they bring (`radiance`). See `shaders/trace.comp` for the steps.
struct Trace {
    pipeline: ComputePipelineHandle,
    radiance: ResourceId,
    normal: ResourceId,
    albedo: ResourceId,
    motion: ResourceId,
    /// True after the "too many lights" error, to log it one time.
    lights_logged: bool,
}

impl Pass for Trace {
    fn name(&self) -> &'static str {
        "trace"
    }

    fn accesses(&self) -> Vec<(ResourceId, Access)> {
        [self.radiance, self.normal, self.albedo, self.motion]
            .map(|image| (image, Access::StorageWrite))
            .to_vec()
    }

    fn execute(&mut self, ctx: &mut PassContext) {
        // The mean has all its samples: one more would cost GPU time and change nothing.
        let frame_samples = ctx.world.resource::<Accumulation>().frame_samples();
        if frame_samples == 0 {
            return;
        }
        let cameras = *ctx.world.resource::<ExtractedCamera>();
        let camera = cameras.current;
        let frame = ctx.world.resource::<ExtractedFrame>();
        // The picture is scaled to the window, so the rays use the shape of the window
        // and not that of the internal image. A resize thus does not stretch the scene.
        // The extracted size and not the swapchain size: the swapchain can change one
        // frame before the window size message, and that frame has no accumulation reset.
        let (width, height) = frame.window_size;
        let aspect = width as f32 / height as f32;
        let [right, up, forward] = ray_basis(&camera, aspect);
        // The camera of the frame before, for the motion of each pixel.
        let slot = ctx.gpu.frame_slot();
        let buffers = ctx.world.resource::<MotionBuffers>();
        let (camera_buffer, instance_buffer) = (buffers.camera[slot], buffers.instances[slot]);
        let previous = Projection::new(
            cameras.previous.position,
            ray_basis(&cameras.previous, aspect),
        );
        ctx.gpu
            .write_buffer(camera_buffer, 0, &previous.bytes())
            .expect("camera buffer write");
        let buffer_index =
            |gpu: &Gpu, buffer| gpu.buffer_index(buffer).expect("the buffer is alive");
        let mesh_table = ctx
            .gpu
            .buffer_index(ctx.world.resource::<GpuMeshes>().table())
            .expect("the mesh table is alive");
        // The lights go into the light buffer of this frame slot.
        let lights = &ctx.world.resource::<ExtractedLights>().0;
        if lights.len() > MAX_LIGHTS && !self.lights_logged {
            self.lights_logged = true;
            tracing::error!(
                lights = lights.len(),
                "more than MAX_LIGHTS ({MAX_LIGHTS}) lights: the rest gives no light"
            );
        }
        let light_buffer = ctx.world.resource::<LightBuffers>().0[ctx.gpu.frame_slot()];
        let light_data = light_bytes(lights);
        if !light_data.is_empty() {
            ctx.gpu
                .write_buffer(light_buffer, 0, &light_data)
                .expect("light buffer write");
        }
        let light_index = ctx
            .gpu
            .buffer_index(light_buffer)
            .expect("the light buffer is alive");
        // Field order as in the `Push` block of the shader: a vec3, then one 32-bit value.
        let vec3_bits = |v: Vec3, last: u32| [v.x.to_bits(), v.y.to_bits(), v.z.to_bits(), last];
        let mut fields = [
            vec3_bits(camera.position, mesh_table),
            vec3_bits(right, ctx.bindless_index(self.radiance)),
            vec3_bits(up, ctx.bindless_index(self.albedo)),
            vec3_bits(forward, ctx.bindless_index(self.normal)),
        ]
        .concat();
        fields.extend([
            // Only the low bits: the number must differ from frame to frame.
            frame.frame as u32,
            frame_samples,
            frame.max_path_rays,
            light_index,
            lights.len().min(MAX_LIGHTS) as u32,
            ctx.bindless_index(self.motion),
            buffer_index(ctx.gpu, camera_buffer),
            buffer_index(ctx.gpu, instance_buffer),
        ]);
        ctx.gpu
            .dispatch_compute(
                self.pipeline,
                &push_constants(&fields),
                groups(ctx.extent(self.radiance)),
            )
            .expect("trace dispatch");
    }
}

/// The picture (the result of the denoiser) to display color, scaled from the internal
/// resolution to the window size. With a debug view
/// ([`DebugView`](crate::DebugView)) it shows another image of the graph in its place. Runs in each frame, also when the accumulation is complete: the window
/// needs a picture in each frame, and this pass is cheap.
struct Tonemap {
    pipeline: ComputePipelineHandle,
    sampler: SamplerHandle,
    hdr: ResourceId,
    /// The images of the debug views: normal, motion, the mean of the denoiser, its
    /// moments, and the radiance of this frame.
    views: [ResourceId; 5],
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
        .into_iter()
        .chain(self.views.map(|image| (image, Access::Sampled)))
        .collect()
    }

    fn execute(&mut self, ctx: &mut PassContext) {
        let sampler = ctx
            .gpu
            .sampler_index(self.sampler)
            .expect("tonemap sampler is alive");
        let frame = ctx.world.resource::<ExtractedFrame>();
        let mut push = vec![
            ctx.bindless_index(self.hdr),
            sampler,
            ctx.bindless_index(self.output),
            // A fade is a lower exposure: at zero the tone curve gives black.
            (frame.exposure * (1.0 - frame.fade)).to_bits(),
            // The `VIEW_*` numbers of the shader.
            frame.debug_view as u32,
        ];
        push.extend(self.views.map(|image| ctx.bindless_index(image)));
        let push = push_constants(&push);
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
