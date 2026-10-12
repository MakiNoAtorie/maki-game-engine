//! The debug panel of the renderer: what the window shows, and the settings of the
//! window and the renderer that have an effect during a run.

use maki_core::{App, Module};
use maki_platform::WindowSettings;
use maki_render::{DebugView, MAX_DENOISE_PASSES, RenderSettings, ToneCurve};

use crate::add_panel;

/// The picture sizes of the selector, in pixels.
const RESOLUTIONS: [(u32, u32); 6] = [
    (640, 360),
    (960, 540),
    (1280, 720),
    (1600, 900),
    (1920, 1080),
    (2560, 1440),
];

/// Adds the "Debug" panel. Needs `UiModule` to show it.
pub struct RenderPanelModule;

impl Module for RenderPanelModule {
    fn build(&self, app: &mut App) {
        add_panel(app, "Debug", true, |ui, world| {
            let mut view = *world.resource::<DebugView>();
            ui.label("View");
            for (value, name) in DebugView::ALL {
                ui.radio_value(&mut view, value, name);
            }
            // Only a different value is a write: a write is a change for the renderer.
            if view != *world.resource::<DebugView>() {
                *world.resource_mut::<DebugView>() = view;
            }

            // The systems read the settings in each frame, so a new value shows in the
            // next frame. A change starts the picture again. Nothing is written to the
            // config file.
            // The slider ranges are in the limits of `RenderSettings::check`.
            ui.separator();
            let mut window = world.resource::<WindowSettings>().clone();
            let mut config = world.resource::<RenderSettings>().clone();

            // The size of the picture. The window gets the size of the picture and
            // the panel column.
            let picture = (window.width, window.height);
            egui::ComboBox::from_label("resolution")
                .selected_text(format!("{} x {}", picture.0, picture.1))
                .show_ui(ui, |ui| {
                    for size in RESOLUTIONS {
                        let name = format!("{} x {}", size.0, size.1);
                        if ui.selectable_label(size == picture, name).clicked() {
                            (window.width, window.height) = size;
                        }
                    }
                });
            ui.collapsing("Display", |ui| {
                ui.checkbox(&mut window.vsync, "vsync");
                if config.width > 0 && config.height > 0 {
                    ui.label(format!(
                        "The config fixes the render size: {} x {}",
                        config.width, config.height
                    ));
                    if ui.button("Follow the resolution").clicked() {
                        (config.width, config.height) = (0, 0);
                    }
                } else {
                    ui.add(egui::Slider::new(&mut config.scale, 0.1..=1.0).text("render scale"));
                }
                ui.checkbox(&mut config.taa, "anti-aliasing");
                ui.add(
                    egui::Slider::new(&mut config.taa_blend, 0.0..=1.0)
                        .text("anti-aliasing: part of a new frame (0: reference mean)"),
                );
                ui.add(
                    egui::Slider::new(&mut config.texture_bias, -4.0..=4.0)
                        .text("texture mip bias (below 0: sharper)"),
                );
                ui.add(
                    egui::Slider::new(&mut config.exposure, 0.1..=8.0)
                        .logarithmic(true)
                        .text("exposure"),
                );
            });
            ui.collapsing("Look", |ui| {
                ui.horizontal(|ui| {
                    ui.label("tone curve");
                    ui.radio_value(&mut config.tone_curve, ToneCurve::Agx, "AgX");
                    ui.radio_value(&mut config.tone_curve, ToneCurve::Neutral, "PBR Neutral");
                });
                ui.add(egui::Slider::new(&mut config.bloom, 0.0..=0.5).text("bloom"));
                ui.add(egui::Slider::new(&mut config.vignette, 0.0..=1.0).text("vignette"));
                // R, G, B of each grade control.
                for (name, values, range) in [
                    ("lift", &mut config.grade_lift, -0.2..=0.2),
                    ("gamma", &mut config.grade_gamma, 0.5..=2.0),
                    ("gain", &mut config.grade_gain, 0.5..=2.0),
                ] {
                    ui.horizontal(|ui| {
                        for value in values {
                            ui.add(
                                egui::DragValue::new(value)
                                    .speed(0.005)
                                    .range(range.clone()),
                            );
                        }
                        ui.label(name);
                    });
                }
            });
            ui.collapsing("Path tracer", |ui| {
                ui.add(
                    egui::Slider::new(&mut config.samples_per_frame, 0..=64)
                        .text("samples per frame (0: automatic)"),
                );
                ui.add(
                    egui::Slider::new(&mut config.trace_budget_ms, 0.5..=33.0)
                        .text("trace budget, ms (automatic only)"),
                );
                ui.add(
                    egui::Slider::new(&mut config.max_path_rays, 2..=32).text("rays in one path"),
                );
                ui.checkbox(&mut config.blue_noise, "blue noise");
                ui.add(
                    egui::Slider::new(&mut config.max_bounces, 1..=16)
                        .text("max bounces (diffuse)"),
                );
                ui.checkbox(&mut config.bounce_light, "bounce light of the frame before");
            });
            ui.collapsing("Denoiser", |ui| {
                ui.add(
                    egui::Slider::new(&mut config.denoise_passes, 0..=MAX_DENOISE_PASSES)
                        .text("filter passes, indirect light"),
                );
                ui.add(
                    egui::Slider::new(&mut config.denoise_direct_passes, 0..=MAX_DENOISE_PASSES)
                        .text("filter passes, direct light"),
                );
                ui.add(
                    egui::Slider::new(&mut config.denoise_specular_passes, 0..=MAX_DENOISE_PASSES)
                        .text("filter passes, rough reflections"),
                );
                ui.add(
                    egui::Slider::new(&mut config.denoise_reach, 0.0..=4096.0)
                        .logarithmic(true)
                        .text("filter reach (0: all passes at each pixel)"),
                );
                ui.add(
                    egui::Slider::new(&mut config.indirect_clamp, 0.0..=64.0)
                        .text("bounce light limit (0: none)"),
                );
                ui.add(
                    egui::Slider::new(&mut config.history_clamp, 0.0..=8.0)
                        .text("direct light history test, deviations (0: off)"),
                );
                ui.add(
                    egui::Slider::new(&mut config.light_history_samples, 0..=1024)
                        .logarithmic(true)
                        .text("indirect samples kept on a light change"),
                );
                ui.add(
                    egui::Slider::new(&mut config.max_samples, 1..=16384)
                        .logarithmic(true)
                        .text("samples kept at rest"),
                );
            });
            // Only a different value is a write.
            if window != *world.resource::<WindowSettings>() {
                *world.resource_mut::<WindowSettings>() = window;
            }
            if config != *world.resource::<RenderSettings>() {
                *world.resource_mut::<RenderSettings>() = config;
            }
        });
    }
}
