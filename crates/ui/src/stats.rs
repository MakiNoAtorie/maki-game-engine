//! The stats panel: frame time, sample count, light, resolution, GPU time of each pass.
//! And the frame counter in the corner of the picture.
//! It uses only the public panel API, as a panel of another crate does.

use std::time::{Duration, Instant};

use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Res, ResMut};
use maki_core::{App, Module, Time, Transform, Update};
use maki_render::{Accumulation, ExtractedFrame, FrameGraph, RenderGpu, RenderWorld};
use maki_scene::{Light, Player};

use crate::{add_corner, add_panel};

/// Adds the "Stats" panel. Needs `UiModule` to show it.
pub struct StatsPanelModule;

/// The frame time in milliseconds that the game would have with no limit: without the
/// time that the CPU waited for the display (vsync) or for the GPU. The GPU time of
/// the passes is the lower limit, because the wait for the GPU is not counted.
fn work_ms(interval: Duration, wait: Duration, gpu_ms: f32) -> f32 {
    (interval.saturating_sub(wait).as_secs_f32() * 1000.0).max(gpu_ms)
}

/// The mean of the frame time with no limit ([`work_ms`]), in milliseconds: for the
/// frame counter, and for the status row of a tool.
#[derive(Resource)]
pub struct FrameTime {
    pub mean_ms: f32,
    /// The start of the frame before.
    last: Instant,
}

fn measure_frame_time(
    mut time: ResMut<FrameTime>,
    gpu: Option<Res<RenderGpu>>,
    graph: Option<Res<FrameGraph>>,
) {
    // The real time, not `Time::delta`: a benchmark run sets that to a fixed value.
    let interval = std::mem::replace(&mut time.last, Instant::now()).elapsed();
    let wait = gpu.map_or(Duration::ZERO, |gpu| gpu.0.frame_wait());
    let timings = graph.as_ref().and_then(|graph| graph.timings());
    let gpu_ms = timings.map_or(0.0, |t| t.passes.iter().map(|(_, ms)| ms).sum());
    let ms = work_ms(interval, wait, gpu_ms);
    // A moving mean: the time of one frame changes too fast to read.
    time.mean_ms = if time.mean_ms == 0.0 {
        ms
    } else {
        time.mean_ms + (ms - time.mean_ms) * 0.05
    };
}

impl Module for StatsPanelModule {
    fn build(&self, app: &mut App) {
        app.insert_resource(FrameTime {
            mean_ms: 0.0,
            last: Instant::now(),
        })
        // It reads what the renderer measured: in any order with it.
        .add_systems(Update, measure_frame_time.ambiguous_with_all());
        add_corner(app, "Frame counter", true, move |ui, world| {
            let mean_ms = world.resource::<FrameTime>().mean_ms;
            let green = |text: String| {
                egui::RichText::new(text)
                    .monospace()
                    .size(16.0)
                    .color(egui::Color32::GREEN)
            };
            ui.label(green(format!(
                "{:.0} fps",
                1000.0 / mean_ms.max(f32::EPSILON)
            )));
            ui.label(green(format!("{mean_ms:.2} ms")));
        });

        // State of the panel: the mean frame time in milliseconds.
        let mut frame_ms = 0.0_f32;
        add_panel(app, "Stats", true, move |ui, world| {
            // `Time::delta` has an upper limit (`max_fixed_steps` fixed steps), so a
            // very slow frame shows as that limit.
            let delta_ms = world.resource::<Time>().delta.as_secs_f32() * 1000.0;
            // A moving mean: the time of one frame changes too fast to read.
            frame_ms = if frame_ms == 0.0 {
                delta_ms
            } else {
                frame_ms + (delta_ms - frame_ms) * 0.05
            };

            let mut rows = vec![
                // As the display shows them: with vsync, the rate of the display.
                ("Frame time", format!("{frame_ms:.2} ms")),
                ("FPS", format!("{:.0}", 1000.0 / frame_ms.max(f32::EPSILON))),
            ];
            // The values that the renderer uses for this picture. None without a renderer.
            if let Some(render_world) = world.get_resource::<RenderWorld>()
                && let Some(frame) = render_world.0.get_resource::<ExtractedFrame>()
            {
                let samples = render_world.0.resource::<Accumulation>().samples();
                let (width, height) = frame.internal_resolution;
                rows.push(("Samples", format!("{samples} / {}", frame.max_samples)));
                rows.push(("Samples per frame", frame.samples_per_frame.to_string()));
                rows.push(("Internal resolution", format!("{width} x {height}")));
            }
            let mut players = world.query_filtered::<&Transform, bevy_ecs::query::With<Player>>();
            for transform in players.iter(world) {
                let p = transform.translation;
                rows.push((
                    "Player position",
                    format!("{:.2}, {:.2}, {:.2}", p.x, p.y, p.z),
                ));
            }
            for (transform, light) in world.query::<(&Transform, &Light)>().iter(world) {
                let p = transform.translation;
                rows.push((
                    "Light position",
                    format!("{:.2}, {:.2}, {:.2}", p.x, p.y, p.z),
                ));
                rows.push(("Light intensity", format!("{:.2}", light.intensity)));
            }

            if let Some(gpu) = world.get_resource::<RenderGpu>() {
                let megabytes = gpu.0.allocated_bytes() as f64 / (1024.0 * 1024.0);
                rows.push(("GPU memory", format!("{megabytes:.1} MB")));
            }
            // Not a mean: the values of the newest frame that the GPU has finished.
            if let Some(timings) = world.get_resource::<FrameGraph>().and_then(|g| g.timings()) {
                for (pass, ms) in &timings.passes {
                    rows.push((pass, format!("{ms:.3} ms GPU")));
                }
            }

            egui::Grid::new("stats").num_columns(2).show(ui, |ui| {
                for (name, value) in rows {
                    ui.label(name);
                    ui.label(value);
                    ui.end_row();
                }
            });
        });
    }
}
