//! The stats panel: frame time, sample count, light, resolution.
//! It uses only the public panel API, as a panel of another crate does.

use maki_core::{App, Module, Time};
use maki_render::{Accumulation, ExtractedFrame, Light, RenderWorld};

use crate::add_panel;

/// Adds the "Stats" panel. Needs `UiModule` to show it.
pub struct StatsPanelModule;

impl Module for StatsPanelModule {
    fn build(&self, app: &mut App) {
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
                rows.push(("Internal resolution", format!("{width} x {height}")));
            }
            for light in world.query::<&Light>().iter(world) {
                let p = light.position;
                rows.push((
                    "Light position",
                    format!("{:.2}, {:.2}, {:.2}", p.x, p.y, p.z),
                ));
                rows.push(("Light intensity", format!("{:.2}", light.intensity)));
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
