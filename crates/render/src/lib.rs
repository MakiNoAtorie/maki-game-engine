//! The renderer: owns the GPU and runs one GPU frame per app frame.
//!
//! Data flow of one frame:
//! 1. `Extract` schedule: systems copy what the renderer needs from the game world into
//!    the [`RenderWorld`].
//! 2. `Render` schedule: `render_frame` starts the GPU frame and runs the [`FrameGraph`].
//!    The passes of the graph get the render world and nothing of the game world.
//! 3. `Present` schedule: `end_frame` submits the frame and shows it.
//!
//! Meshes: see the `meshes` module. It uploads the mesh assets and keeps the mesh table.
//! The game writes [`ExtractedInstances`] in `Extract`, and spawns one [`Camera`].
//! The game also spawns [`Light`] entities. The passes (`passes` module) trace one light
//! path per pixel and frame, and keep the mean over frames (`accumulation` module).
//!
//! A module adds to the renderer in two places: an `Extract` system that writes a
//! resource into the render world, and a pass in the frame graph (from a `Startup`
//! system that runs `.after(setup_renderer)`). If the data of the module changes what
//! the picture shows, its `Extract` system also calls
//! [`RenderWorld::reset_accumulation`] when that data changes.
//!
//! Shaders: GLSL files in `shaders/` are compiled to SPIR-V by `build.rs` at build time
//! and embedded with [`shader_spirv!`]. See `build.rs` for the rules.

mod accumulation;
mod camera;
mod graph;
mod light;
mod meshes;
mod passes;

use bevy_ecs::change_detection::DetectChanges;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Commands, Res, ResMut};
use bevy_ecs::world::World;
use maki_core::{App, Config, Extract, Module, Present, Render, Startup, Time};
use maki_gpu::{Gpu, GpuDesc};
use maki_platform::{Window, WindowSize};

pub use accumulation::Accumulation;
pub use camera::{Camera, ExtractedCamera};
pub use graph::{BACKBUFFER, FrameGraph, Pass, PassContext, ResourceId, SizeMode, TransientDesc};
pub use light::{ExtractedLights, Light};
pub use maki_gpu::Access;
pub use meshes::{ExtractedInstance, ExtractedInstances, GpuMesh, GpuMeshes, MAX_MESHES};
pub use passes::MAX_INSTANCES;

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
#[derive(Resource)]
pub struct RenderWorld(pub World);

impl Default for RenderWorld {
    fn default() -> Self {
        let mut world = World::new();
        // Here and not in a `Startup` system: `Extract` systems of all modules use it.
        world.init_resource::<Accumulation>();
        Self(world)
    }
}

impl RenderWorld {
    /// Starts the accumulated image again. Call from an `Extract` system when
    /// something that the picture shows has changed: the old samples are then wrong.
    pub fn reset_accumulation(&mut self) {
        self.0.resource_mut::<Accumulation>().request_reset();
    }
}

/// The per-frame values that the built-in passes need. A resource of the render world.
#[derive(Resource, Debug, Clone, Copy)]
pub struct ExtractedFrame {
    /// Size of the images that the path tracer writes, in pixels.
    pub internal_resolution: (u32, u32),
    /// Size of the window in physical pixels.
    pub window_size: (u32, u32),
    /// Number of the app frame. Part of the seed of the random numbers, so each
    /// frame makes a different sample.
    pub frame: u32,
    /// The accumulation stops at this number of samples.
    pub max_samples: u32,
    /// The tonemap pass multiplies the picture by this.
    pub exposure: f32,
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
            // render world, so their order has no effect. The one shared write, the
            // accumulation reset, only sets a flag.
            .allow_ambiguous_resource::<RenderWorld>()
            // Systems of different modules make their own GPU resources (buffers,
            // pipelines), so their order has no effect. Without this, a module would
            // have to name the private systems of another module to declare an order.
            .allow_ambiguous_resource::<RenderGpu>()
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
        accumulation::build(app);
        meshes::build(app);
        camera::build(app);
        light::build(app);
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
    // The config holds the internal resolution: a new size gives new, empty images.
    // The window size sets the shape of the camera rays (see `Trace` in `passes`).
    if config.is_changed() || size.is_changed() {
        render_world.reset_accumulation();
    }
    render_world.0.insert_resource(ExtractedFrame {
        internal_resolution: (config.render_width, config.render_height),
        window_size: (size.width, size.height),
        // Only the low bits are necessary: the number must differ from frame to frame.
        frame: time.frame as u32,
        max_samples: config.max_samples,
        exposure: config.exposure,
    });
}

/// Halves the internal resolution in every second block of `SELFTEST_PERIOD` frames.
fn selftest_resolution(time: Res<Time>, mut render_world: ResMut<RenderWorld>) {
    // The first frame of a block has a new resolution.
    if time.frame.is_multiple_of(SELFTEST_PERIOD) {
        render_world.reset_accumulation();
    }
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
    mut render_world: ResMut<RenderWorld>,
) {
    let world = &mut render_world.0;
    let frame = *world.resource::<ExtractedFrame>();
    let gpu = &mut gpu.0;
    gpu.resize(frame.window_size.0, frame.window_size.1);
    // False: no frame now (the swapchain is being built again, or the window has no area).
    if gpu.begin_frame().expect("begin_frame") {
        // Only for a frame that is drawn: a reset request waits over a skipped frame.
        world
            .resource_mut::<Accumulation>()
            .start_frame(frame.max_samples);
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
