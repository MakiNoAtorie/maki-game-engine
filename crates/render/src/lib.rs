//! The renderer: owns the GPU and runs one GPU frame per app frame.
//!
//! Data flow of one frame:
//! 1. `Extract` schedule: systems copy what the renderer needs from the game world into
//!    the [`RenderWorld`].
//! 2. `Render` schedule: `render_frame` starts the GPU frame and runs the [`FrameGraph`].
//!    The passes of the graph get the render world and nothing of the game world.
//! 3. `Present` schedule: `end_frame` submits the frame and shows it.
//!
//! A module adds to the renderer in two places: an `Extract` system that writes a
//! resource into the render world, and a pass in the frame graph (from a `Startup`
//! system that runs `.after(setup_renderer)`).
//!
//! Shaders: GLSL files in `shaders/` are compiled to SPIR-V by `build.rs` at build time
//! and embedded with [`shader_spirv!`]. See `build.rs` for the rules.

mod graph;
mod passes;

use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Commands, Res, ResMut};
use bevy_ecs::world::World;
use maki_core::{App, Config, Extract, Module, Present, Render, Startup, Time};
use maki_gpu::{Gpu, GpuDesc};
use maki_platform::{Window, WindowSize};

pub use graph::{BACKBUFFER, FrameGraph, Pass, PassContext, ResourceId, SizeMode, TransientDesc};
pub use maki_gpu::Access;

/// The bytes of a shader that `build.rs` compiled. The argument is the file name in `shaders/`.
macro_rules! shader_spirv {
    ($name:literal) => {
        include_bytes!(concat!(env!("OUT_DIR"), "/", $name, ".spv")).as_slice()
    };
}
pub(crate) use shader_spirv;

/// The GPU as an ECS resource. Exists after `setup_renderer` in `Startup`.
#[derive(Resource)]
pub struct RenderGpu(pub Gpu);

/// The data that the renderer draws from: copies made in the `Extract` schedule.
///
/// A second `bevy_ecs` world and not a struct with fixed fields: a world stores
/// resources by type, so a new module adds its own extracted type and the render crate
/// does not change. Later, extracted objects can be entities of this world.
#[derive(Resource, Default)]
pub struct RenderWorld(pub World);

/// The per-frame values that the built-in passes need. A resource of the render world.
#[derive(Resource, Debug, Clone, Copy)]
pub struct ExtractedFrame {
    /// Size of the images that the path tracer writes, in pixels.
    pub internal_resolution: (u32, u32),
    /// Size of the window in physical pixels.
    pub window_size: (u32, u32),
    /// Game time, for animation in shaders.
    pub seconds: f32,
}

/// Set this environment variable to change the internal resolution every few frames.
/// Validation must stay clean: this proves that the graph makes its images again.
const SELFTEST_ENV: &str = "MAKI_RENDER_SELFTEST";
/// Frames between two resolution changes of the self-test.
const SELFTEST_PERIOD: u64 = 7;

/// Makes the GPU and draws each app frame through the frame graph.
/// Needs the window, so the app must run with `maki_platform::run`.
pub struct RenderModule;

impl Module for RenderModule {
    fn build(&self, app: &mut App) {
        app.init_resource::<RenderWorld>()
            // Extract systems of different modules write different resources of the
            // render world, so their order has no effect.
            .allow_ambiguous_resource::<RenderWorld>()
            .add_systems(Startup, setup_renderer)
            .add_systems(Extract, extract_frame)
            .add_systems(Render, render_frame)
            .add_systems(Present, end_frame.run_if(frame_active));
        if std::env::var_os(SELFTEST_ENV).is_some() {
            tracing::info!(
                "render self-test is on: the internal resolution changes every {SELFTEST_PERIOD} frames"
            );
            app.add_systems(Extract, selftest_resolution.after(extract_frame));
        }
    }
}

/// Makes the GPU and the frame graph with the built-in passes.
/// Startup systems that add passes or make GPU resources run `.after(setup_renderer)`.
pub fn setup_renderer(
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
    let mut gpu =
        Gpu::new(&*window, &desc).unwrap_or_else(|error| panic!("cannot start the GPU: {error:#}"));
    let mut graph = FrameGraph::default();
    passes::build_graph(&mut gpu, &mut graph);
    commands.insert_resource(RenderGpu(gpu));
    commands.insert_resource(graph);
}

/// The only place where the built-in renderer reads the game world.
fn extract_frame(
    config: Res<Config>,
    size: Res<WindowSize>,
    time: Res<Time>,
    mut render_world: ResMut<RenderWorld>,
) {
    render_world.0.insert_resource(ExtractedFrame {
        internal_resolution: (config.render_width, config.render_height),
        window_size: (size.width, size.height),
        seconds: time.elapsed.as_secs_f32(),
    });
}

/// Halves the internal resolution in every second block of `SELFTEST_PERIOD` frames.
fn selftest_resolution(time: Res<Time>, mut render_world: ResMut<RenderWorld>) {
    if (time.frame / SELFTEST_PERIOD).is_multiple_of(2) {
        let mut frame = render_world.0.resource_mut::<ExtractedFrame>();
        let (width, height) = frame.internal_resolution;
        frame.internal_resolution = (width.div_ceil(2), height.div_ceil(2));
    }
}

/// Starts the GPU frame and records all passes. Reads the render world only.
fn render_frame(
    mut gpu: ResMut<RenderGpu>,
    mut graph: ResMut<FrameGraph>,
    render_world: Res<RenderWorld>,
) {
    let world = &render_world.0;
    let frame = *world.resource::<ExtractedFrame>();
    let gpu = &mut gpu.0;
    gpu.resize(frame.window_size.0, frame.window_size.1);
    // False: no frame now (the swapchain is being built again, or the window has no area).
    if gpu.begin_frame().expect("begin_frame") {
        graph.execute(gpu, world, frame.internal_resolution);
    }
}

/// Run condition. False when the frame was skipped.
fn frame_active(gpu: Res<RenderGpu>) -> bool {
    gpu.0.frame_active()
}

fn end_frame(mut gpu: ResMut<RenderGpu>) {
    gpu.0.end_frame().expect("end_frame");
}
