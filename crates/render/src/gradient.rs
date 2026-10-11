//! TEMPORARY: stage 3 proof that the gpu crate works. Not part of the renderer.
//!
//! Pass 1: a compute shader writes a gradient into a small storage image (colors from a buffer).
//! Pass 2: a compute shader reads that image through a sampler and writes the output image.
//! The output image is blitted to the window.
//!
//! Self-test: with the environment variable `MAKI_GPU_SELFTEST` set, all resources are
//! destroyed and made again every few frames, while frames that use them are in flight.
//! Validation must stay clean: this proves deferred destruction and bindless index reuse.
//!
//! To remove: delete this file, `shaders/gradient*.comp`, the `gradient` lines in `lib.rs`,
//! and the `GradientModule` line in `game/src/main.rs`.

use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Commands, Res, ResMut};
use maki_core::{App, Config, Extract, Module, Render, Startup, Time};
use maki_gpu::{
    BufferDesc, BufferHandle, ComputePipelineHandle, Format, Gpu, ImageDesc, ImageHandle,
    SamplerDesc, SamplerHandle,
};

use crate::{RenderGpu, begin_frame, create_gpu, frame_active, shader_spirv};

/// Frames between two resource rebuilds of the self-test.
const SELFTEST_PERIOD: u64 = 7;

pub struct GradientModule;

impl Module for GradientModule {
    fn build(&self, app: &mut App) {
        app.init_resource::<GradientInput>()
            .add_systems(Startup, setup.after(create_gpu))
            .add_systems(Extract, extract)
            .add_systems(Render, draw.after(begin_frame).run_if(frame_active));
    }
}

#[derive(Resource)]
struct Gradient {
    gradient_pipeline: ComputePipelineHandle,
    resample_pipeline: ComputePipelineHandle,
    palette: BufferHandle,
    /// Written by pass 1, sampled by pass 2.
    small: ImageHandle,
    sampler: SamplerHandle,
    output: ImageHandle,
    width: u32,
    height: u32,
    selftest: bool,
}

/// The data that `draw` needs, copied out of the game state in the Extract schedule.
#[derive(Resource, Default)]
struct GradientInput {
    seconds: f32,
    frame: u64,
}

impl Gradient {
    fn new(gpu: &mut Gpu, width: u32, height: u32) -> Self {
        let image = |gpu: &mut Gpu, name, width, height, sampled| {
            gpu.create_image(&ImageDesc {
                name,
                width,
                height,
                format: Format::Rgba8Unorm,
                storage: true,
                sampled,
            })
            .expect("gradient image")
        };
        let small = image(
            gpu,
            "gradient small",
            width.div_ceil(4),
            height.div_ceil(4),
            true,
        );
        let output = image(gpu, "gradient output", width, height, false);

        // Two RGBA colors: dark red and dark blue.
        let colors: [f32; 8] = [0.35, 0.02, 0.04, 1.0, 0.02, 0.05, 0.30, 1.0];
        let bytes: Vec<u8> = colors.iter().flat_map(|c| c.to_ne_bytes()).collect();
        let palette = gpu
            .create_buffer(&BufferDesc {
                name: "gradient palette",
                size: bytes.len() as u64,
                cpu_writable: true,
            })
            .expect("gradient palette");
        gpu.write_buffer(palette, 0, &bytes)
            .expect("gradient palette write");

        Self {
            gradient_pipeline: gpu
                .create_compute_pipeline(shader_spirv!("gradient.comp"))
                .expect("gradient pipeline"),
            resample_pipeline: gpu
                .create_compute_pipeline(shader_spirv!("gradient_resample.comp"))
                .expect("resample pipeline"),
            palette,
            small,
            sampler: gpu
                .create_sampler(&SamplerDesc {
                    linear: true,
                    repeat: false,
                })
                .expect("gradient sampler"),
            output,
            width,
            height,
            selftest: std::env::var_os("MAKI_GPU_SELFTEST").is_some(),
        }
    }

    fn destroy(&self, gpu: &mut Gpu) {
        gpu.destroy_compute_pipeline(self.gradient_pipeline);
        gpu.destroy_compute_pipeline(self.resample_pipeline);
        gpu.destroy_buffer(self.palette);
        gpu.destroy_image(self.small);
        gpu.destroy_sampler(self.sampler);
        gpu.destroy_image(self.output);
    }
}

fn setup(mut commands: Commands, mut gpu: ResMut<RenderGpu>, config: Res<Config>) {
    let gradient = Gradient::new(&mut gpu.0, config.render_width, config.render_height);
    if gradient.selftest {
        tracing::info!(
            "GPU self-test is on: resources are made again every {SELFTEST_PERIOD} frames"
        );
    }
    commands.insert_resource(gradient);
}

fn extract(time: Res<Time>, mut input: ResMut<GradientInput>) {
    input.seconds = time.elapsed.as_secs_f32();
    input.frame = time.frame;
}

/// A push constant block of 32-bit fields, as bytes.
fn push_constants(fields: &[u32]) -> Vec<u8> {
    fields
        .iter()
        .flat_map(|field| field.to_ne_bytes())
        .collect()
}

fn draw(mut gpu: ResMut<RenderGpu>, mut gradient: ResMut<Gradient>, input: Res<GradientInput>) {
    let gpu = &mut gpu.0;
    if gradient.selftest && input.frame.is_multiple_of(SELFTEST_PERIOD) {
        // The frames in flight still use the old resources at this point.
        gradient.destroy(gpu);
        *gradient = Gradient::new(gpu, gradient.width, gradient.height);
    }
    let g = &*gradient;
    let palette = gpu.buffer_index(g.palette).expect("palette is alive");
    let small = gpu.image_index(g.small).expect("small image is alive");
    let sampler = gpu.sampler_index(g.sampler).expect("sampler is alive");
    let output = gpu.image_index(g.output).expect("output image is alive");

    // The shaders have 8x8 work groups: round up so the groups cover the image.
    let groups = |width: u32, height: u32| [width.div_ceil(8), height.div_ceil(8), 1];
    let (small_width, small_height) = (g.width.div_ceil(4), g.height.div_ceil(4));

    // Field order as in the `Push` block of each shader.
    let push = push_constants(&[input.seconds.to_bits(), small, palette]);
    gpu.dispatch_compute(
        g.gradient_pipeline,
        &push,
        groups(small_width, small_height),
    )
    .expect("gradient dispatch");
    let push = push_constants(&[small, sampler, output]);
    gpu.dispatch_compute(g.resample_pipeline, &push, groups(g.width, g.height))
        .expect("resample dispatch");
    gpu.blit_to_swapchain(g.output).expect("gradient blit");
}
