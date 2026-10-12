//! The debug panel: what the window shows, and the config values that have an effect
//! during a run.

use maki_core::{App, Config, Module, ToneCurve};
use maki_render::{DebugView, MAX_DENOISE_PASSES};
use maki_ui::{add_panel, egui};

/// Adds the "Debug" panel. Needs `UiModule` to show it.
pub struct DebugPanelModule;

impl Module for DebugPanelModule {
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

            // The systems read the config in each frame, so a new value shows in the
            // next frame. A config change starts the picture again.
            // The slider ranges are in the limits of `App::new` for the config file.
            ui.separator();
            let mut config = world.resource::<Config>().clone();

            ui.collapsing("Display", |ui| {
                ui.checkbox(&mut config.vsync, "vsync");
                if config.render_width > 0 && config.render_height > 0 {
                    ui.label(format!(
                        "The config fixes the render size: {} x {}",
                        config.render_width, config.render_height
                    ));
                    if ui.button("Follow the window").clicked() {
                        (config.render_width, config.render_height) = (0, 0);
                    }
                } else {
                    ui.add(
                        egui::Slider::new(&mut config.render_scale, 0.1..=1.0).text("render scale"),
                    );
                }
                ui.checkbox(&mut config.taa, "anti-aliasing");
                ui.add(
                    egui::Slider::new(&mut config.taa_blend, 0.0..=1.0)
                        .text("anti-aliasing: part of a new frame (0: reference mean)"),
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
            ui.collapsing("Camera", |ui| {
                ui.add(
                    egui::Slider::new(&mut config.camera_fov_degrees, 10.0..=120.0)
                        .text("field of view"),
                );
                ui.add(
                    egui::Slider::new(&mut config.camera_follow_time, 0.0..=2.0)
                        .text("follow time, s"),
                );
                ui.add(
                    egui::Slider::new(&mut config.camera_margin, 0.0..=10.0).text("wall margin, m"),
                );
                // The two points give the view direction and the distance.
                for (name, point) in [
                    ("position", &mut config.camera_position),
                    ("target", &mut config.camera_target),
                ] {
                    ui.horizontal(|ui| {
                        for value in point {
                            ui.add(egui::DragValue::new(value).speed(0.05));
                        }
                        ui.label(name);
                    });
                }
            });
            ui.collapsing("Player", |ui| {
                ui.add(
                    egui::Slider::new(&mut config.player_walk_speed, 0.0..=10.0)
                        .text("walk speed, m/s"),
                );
                ui.add(
                    egui::Slider::new(&mut config.player_run_speed, 0.0..=10.0)
                        .text("run speed, m/s"),
                );
            });

            // Only a different value is a write.
            if config != *world.resource::<Config>() {
                *world.resource_mut::<Config>() = config;
            }
        });
    }
}
