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
//! The game spawns entities with the components of the scene crate: `MeshInstance`,
//! `Light`, and one `Camera`. The passes (`passes` module) trace one light
//! path per pixel and frame, keep the mean over frames (`accumulation` and `denoise`
//! modules), and smooth the edges and scale to the window (`taa` module).
//!
//! A module adds to the renderer in two places: an `Extract` system that writes a
//! resource into the render world, and a pass in the frame graph (from a `Startup`
//! system that runs `.after(setup_renderer)`). If the data of the module changes what
//! the picture shows, its `Extract` system also tells the renderer when that data
//! changes: [`RenderWorld::soften_accumulation`] for a change of the lighting,
//! [`RenderWorld::wake_accumulation`] for a change of the view,
//! [`RenderWorld::reset_accumulation`] if no old sample is of use (see the
//! `accumulation` module).
//!
//! Shaders: GLSL files in `shaders/` are compiled to SPIR-V by `build.rs` at build time
//! and embedded with `shader_spirv!`. See `build.rs` for the rules.

mod accumulation;
mod bloom;
mod camera;
mod denoise;
mod graph;
mod light;
mod meshes;
mod motion;
mod noise;
mod passes;
mod taa;

use bevy_ecs::change_detection::DetectChanges;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Commands, Res, ResMut};
use bevy_ecs::world::World;
use maki_core::{App, Config, Extract, Module, Present, Render, Startup, Time, ToneCurve};
use maki_gpu::{Gpu, GpuDesc};
use maki_platform::{Window, WindowSize};
use maki_scene::{CurrentRoom, ScreenFade};

pub use accumulation::Accumulation;
pub use camera::{ExtractedCamera, ray_basis};
pub use denoise::MAX_DENOISE_PASSES;
pub use graph::{
    BACKBUFFER, FrameGraph, Pass, PassContext, PassTimings, ResourceId, SizeMode, TransientDesc,
};
pub use light::{ExtractedLight, ExtractedLights, MAX_LIGHTS};
pub use maki_gpu::{Access, Format};
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
    /// Drops the history of each pixel. Call from an `Extract` system when the old
    /// samples are of no use: the picture starts again, with all its noise.
    pub fn reset_accumulation(&mut self) {
        self.0.resource_mut::<Accumulation>().request_reset();
    }

    /// Keeps a short history of the indirect light for each pixel
    /// (`light_history_samples` of the config). Call from an `Extract` system when
    /// the lighting has changed: a light, or an object that moved (its shadow moved
    /// too). The old samples are almost correct. The direct light needs no such
    /// call: the denoiser finds the pixels where it changed (`history_clamp`).
    pub fn soften_accumulation(&mut self) {
        self.0.resource_mut::<Accumulation>().request_soft_reset();
    }

    /// Makes new samples and keeps the history. Call from an `Extract` system when
    /// the view has changed: the old samples are correct, and the denoiser finds them
    /// at their new place in the picture.
    pub fn wake_accumulation(&mut self) {
        self.0.resource_mut::<Accumulation>().request_wake();
    }
}

/// The per-frame values that the built-in passes need. A resource of the render world.
#[derive(Resource, Debug, Clone, Copy)]
pub struct ExtractedFrame {
    /// Size of the images that the path tracer writes, in pixels.
    pub internal_resolution: (u32, u32),
    /// Size of the picture in pixels: `window_width` and `window_height` of the
    /// config. The window shows it in its top left corner, not scaled.
    pub picture_size: (u32, u32),
    /// Size of the window in physical pixels: the picture and the room for the panels.
    pub window_size: (u32, u32),
    /// True: a frame waits for the display refresh.
    pub vsync: bool,
    /// Number of the app frame. Part of the seed of the random numbers, so each
    /// frame makes a different sample. Also names the frame in the pass timings.
    pub frame: u64,
    /// The accumulation stops at this number of samples.
    pub max_samples: u32,
    /// Samples that the trace pass makes for each pixel in one frame, 1 or more.
    /// `extract_frame` writes the config value; if that is 0 (automatic), the
    /// accumulation module writes its choice after it.
    pub samples_per_frame: u32,
    /// The largest number of rays in one light path.
    pub max_path_rays: u32,
    /// A path ends after this many diffuse bounces, 1 or more.
    pub max_bounces: u32,
    /// `bounce_light` of the config: the last surface of a path reads the bounce
    /// light of the frame before.
    pub bounce_light: bool,
    /// Samples that the indirect light of a pixel keeps on a soft reset.
    pub light_history_samples: u32,
    /// Filter passes of the denoiser for the indirect light in this frame, 0 to
    /// `MAX_DENOISE_PASSES`.
    pub denoise_passes: u32,
    /// The same for the direct light.
    pub denoise_direct_passes: u32,
    /// The same for the specular light of a rough surface.
    pub denoise_specular_passes: u32,
    /// `denoise_reach` of the config: a filter pass runs for a pixel while
    /// `step * step * samples` is at most this. 0: for each pixel.
    pub denoise_reach: f32,
    /// `indirect_clamp` of the config: the limit for the light of one bounce ray.
    pub indirect_clamp: f32,
    /// `history_clamp` of the config: the history test of the direct light, in
    /// standard deviations of the noise. 0: no such test.
    pub history_clamp: f32,
    /// `blue_noise` of the config: the trace pass uses the blue-noise pattern.
    pub blue_noise: bool,
    /// The offset of all camera rays of this frame from the pixel centers, in pixels:
    /// see the `taa` module. Zero with anti-aliasing off.
    pub jitter: [f32; 2],
    /// `taa` of the config: anti-aliasing is on.
    pub taa: bool,
    /// `taa_blend` of the config: the part of a new frame in the picture.
    pub taa_blend: f32,
    /// The tonemap pass multiplies the picture by this.
    pub exposure: f32,
    pub look: Look,
    /// How dark the fade makes the picture: 0.0 is the picture, 1.0 is black.
    pub fade: f32,
    pub debug_view: DebugView,
}

/// The look of the picture: the values of the config for the passes after the
/// anti-aliasing (`bloom` and `tonemap`). See `Config` for each one.
#[derive(Debug, Clone, Copy)]
pub struct Look {
    pub tone_curve: ToneCurve,
    pub bloom: f32,
    pub vignette: f32,
    pub lift: [f32; 3],
    pub gamma: [f32; 3],
    pub gain: [f32; 3],
}

/// What the window shows. A resource of the game world: a debug panel writes it.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DebugView {
    /// The picture.
    #[default]
    Final,
    /// Screen motion of each pixel since the frame before: red for X, green for Y.
    Motion,
    /// Distance from the camera: near is bright.
    Depth,
    /// World normal: X, Y, Z from -1 to 1 as red, green, blue from 0 to 1.
    Normals,
    /// Samples in the mean of each pixel: black for none, white for `max_samples`.
    HistoryLength,
    /// How noisy one frame is at each pixel: the standard deviation of its brightness
    /// over time, as a part of the brightness.
    Variance,
    /// The samples of this frame alone, with no denoiser.
    Raw,
    /// What the filter passes of the indirect light get: its mean over time, without
    /// the surface color.
    FilterInput,
    /// The noise that the filter takes that mean to have, as a part of its brightness.
    /// The filter blurs where this is bright.
    FilterVariance,
    /// The indirect light after its last filter pass, without the surface color.
    FilterOutput,
    /// The direct light after its last filter pass, without the surface color.
    Direct,
    /// The mean of the specular light: the reflection on glass, and the light itself.
    Specular,
    /// The surface color that the light is multiplied by.
    Albedo,
}

impl DebugView {
    /// Each view with its name, for a panel.
    pub const ALL: [(Self, &str); 13] = [
        (Self::Final, "Final"),
        (Self::Raw, "Raw (one frame)"),
        (Self::Motion, "Motion vectors"),
        (Self::Depth, "Depth"),
        (Self::Normals, "Normals"),
        (Self::HistoryLength, "History length"),
        (Self::Variance, "Variance (one frame)"),
        (Self::Albedo, "Surface color"),
        (Self::Direct, "Direct light"),
        (Self::FilterInput, "Indirect light: filter input"),
        (Self::FilterVariance, "Indirect light: filter variance"),
        (Self::FilterOutput, "Indirect light"),
        (Self::Specular, "Specular light"),
    ];
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
            .init_resource::<DebugView>()
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
        motion::build(app);
        noise::build(app);
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
pub(crate) fn extract_frame(
    config: Res<Config>,
    size: Res<WindowSize>,
    time: Res<Time>,
    debug_view: Res<DebugView>,
    // The two come from `maki_scene::SceneModule`. A game without it has no rooms.
    room: Option<Res<CurrentRoom>>,
    fade: Option<Res<ScreenFade>>,
    mut render_world: ResMut<RenderWorld>,
) {
    // A new internal resolution gives new, empty images. The picture size is in the
    // config, so a new window size changes nothing in the picture.
    // ponytail: each config change is a reset, also one that needs none (the exposure).
    // Compare the old and the new values that the picture depends on if that shows.
    // Another room: the old samples show the old room, also where a wall of the new
    // room is at the same place.
    let new_room = room.is_some_and(|room| room.is_changed());
    if config.is_changed() || new_room {
        render_world.reset_accumulation();
    }
    // Never zero: the passes divide by it.
    let picture = (config.window_width.max(1), config.window_height.max(1));
    render_world.0.insert_resource(ExtractedFrame {
        internal_resolution: internal_resolution(&config, picture),
        picture_size: picture,
        window_size: (size.width, size.height),
        vsync: config.vsync,
        frame: time.frame,
        max_samples: config.max_samples,
        // Never 0: the passes divide by it.
        samples_per_frame: config.samples_per_frame.max(1),
        max_path_rays: config.max_path_rays,
        max_bounces: config.max_bounces.clamp(1, 16),
        bounce_light: config.bounce_light,
        light_history_samples: config.light_history_samples,
        denoise_passes: config.denoise_passes,
        denoise_direct_passes: config.denoise_direct_passes,
        denoise_specular_passes: config.denoise_specular_passes,
        denoise_reach: config.denoise_reach,
        indirect_clamp: config.indirect_clamp,
        history_clamp: config.history_clamp,
        blue_noise: config.blue_noise,
        jitter: if config.taa {
            taa::jitter(time.frame)
        } else {
            [0.0; 2]
        },
        taa: config.taa,
        taa_blend: config.taa_blend,
        exposure: config.exposure,
        look: Look {
            tone_curve: config.tone_curve,
            bloom: config.bloom,
            vignette: config.vignette,
            lift: config.grade_lift,
            gamma: config.grade_gamma,
            gain: config.grade_gain,
        },
        fade: fade.map_or(0.0, |fade| fade.0.clamp(0.0, 1.0)),
        debug_view: *debug_view,
    });
}

/// The size of the path tracer images: the fixed size of the config, or the picture
/// size multiplied by `render_scale`. Never zero.
fn internal_resolution(config: &Config, picture: (u32, u32)) -> (u32, u32) {
    if config.render_width > 0 && config.render_height > 0 {
        return (config.render_width, config.render_height);
    }
    let scaled = |size: u32| ((size as f32 * config.render_scale).round() as u32).max(1);
    (scaled(picture.0), scaled(picture.1))
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
    gpu.set_vsync(frame.vsync);
    // False: no frame now (the swapchain is being built again, or the window has no area).
    if gpu.begin_frame().expect("begin_frame") {
        // Only for a frame that is drawn: a reset request waits over a skipped frame.
        world.resource_mut::<Accumulation>().start_frame(
            frame.max_samples,
            frame.samples_per_frame,
            frame.light_history_samples,
        );
        // The history images change places only when the passes write them.
        let sampling = world.resource::<Accumulation>().sampling();
        graph.execute(
            gpu,
            world,
            frame.internal_resolution,
            frame.picture_size,
            frame.frame,
            sampling,
        );
    }
}

/// Run condition. False when the frame was skipped.
fn frame_active(gpu: Res<RenderGpu>) -> bool {
    gpu.0.frame_active()
}

fn end_frame(mut gpu: ResMut<RenderGpu>) {
    gpu.0.end_frame().expect("end_frame");
}
