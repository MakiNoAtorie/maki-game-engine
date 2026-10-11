//! The renderer: owns the GPU and runs one GPU frame per app frame.
//!
//! Frame flow: `begin_frame` (Render) -> draw systems of other modules (Render,
//! `.after(begin_frame).run_if(frame_active)`) -> `end_frame` (Present).
//!
//! Shaders: GLSL files in `shaders/` are compiled to SPIR-V by `build.rs` at build time
//! and embedded with [`shader_spirv!`]. See `build.rs` for the rules.

mod gradient;

use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Commands, Res, ResMut};
use maki_core::{App, Config, Module, Present, Render, Startup};
use maki_gpu::{Gpu, GpuDesc};
use maki_platform::{Window, WindowSize};

pub use gradient::GradientModule;

/// The bytes of a shader that `build.rs` compiled. The argument is the file name in `shaders/`.
macro_rules! shader_spirv {
    ($name:literal) => {
        include_bytes!(concat!(env!("OUT_DIR"), "/", $name, ".spv")).as_slice()
    };
}
pub(crate) use shader_spirv;

/// The GPU as an ECS resource. Exists after `create_gpu` in `Startup`.
#[derive(Resource)]
pub struct RenderGpu(pub Gpu);

/// Makes the GPU and frames each app frame with `begin_frame` / `end_frame`.
/// Needs the window, so the app must run with `maki_platform::run`.
pub struct RenderModule;

impl Module for RenderModule {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, create_gpu)
            .add_systems(Render, begin_frame)
            .add_systems(Present, end_frame.run_if(frame_active));
    }
}

/// Startup systems that make GPU resources run `.after(create_gpu)`.
pub fn create_gpu(
    mut commands: Commands,
    window: Res<Window>,
    size: Res<WindowSize>,
    config: Res<Config>,
) {
    let desc = GpuDesc {
        width: size.width,
        height: size.height,
        vsync: config.vsync,
    };
    // No GPU, no game: stop with the reason (for example the missing Vulkan features).
    let gpu =
        Gpu::new(&*window, &desc).unwrap_or_else(|error| panic!("cannot start the GPU: {error:#}"));
    commands.insert_resource(RenderGpu(gpu));
}

/// Draw systems run after this one.
// `WindowSize` is platform data, not game state, so it needs no Extract step.
pub fn begin_frame(mut gpu: ResMut<RenderGpu>, size: Res<WindowSize>) {
    gpu.0.resize(size.width, size.height);
    gpu.0.begin_frame().expect("begin_frame");
}

/// Run condition. False when the frame was skipped (the swapchain is being built again).
pub fn frame_active(gpu: Res<RenderGpu>) -> bool {
    gpu.0.frame_active()
}

fn end_frame(mut gpu: ResMut<RenderGpu>) {
    gpu.0.end_frame().expect("end_frame");
}
