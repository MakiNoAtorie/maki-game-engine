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
            let view_name = |view: DebugView| {
                let name = DebugView::ALL.iter().find(|(value, _)| *value == view);
                name.map_or("?", |(_, name)| *name)
            };
            egui::ComboBox::from_label("View")
                .selected_text(view_name(view))
                .show_ui(ui, |ui| {
                    for (value, name) in DebugView::ALL {
                        ui.selectable_value(&mut view, value, name);
                    }
                });
            // Only a different value is a write: a write is a change for the renderer.
            if view != *world.resource::<DebugView>() {
                *world.resource_mut::<DebugView>() = view;
            }

            // The systems read the settings in each frame, so a new value shows in the
            // next frame. A change starts the picture again. Nothing is written to the
            // config file.
            // The slider ranges are in the limits of `RenderSettings::check`.
            ui.weak("Changes last until you quit; the config file is not changed.");
            ui.separator();
            let mut window = world.resource::<WindowSettings>().clone();
            let mut config = world.resource::<RenderSettings>().clone();

            egui::CollapsingHeader::new("Display")
                .default_open(true)
                .show(ui, |ui| {
                    // The window gets the size of the picture and the panel column.
                    let picture = (window.width, window.height);
                    egui::ComboBox::from_label("Resolution")
                        .selected_text(format!("{} x {}", picture.0, picture.1))
                        .show_ui(ui, |ui| {
                            for size in RESOLUTIONS {
                                let name = format!("{} x {}", size.0, size.1);
                                if ui.selectable_label(size == picture, name).clicked() {
                                    (window.width, window.height) = size;
                                }
                            }
                        });
                    ui.checkbox(&mut window.vsync, "VSync");
                    let fixed = config.width > 0 && config.height > 0;
                    ui.add_enabled(
                        !fixed,
                        egui::Slider::new(&mut config.scale, 0.1..=1.0).text("Render scale"),
                    );
                    if fixed {
                        ui.weak(format!(
                            "The config fixes the render size: {} x {}",
                            config.width, config.height
                        ));
                        if ui.button("Use the render scale (this run only)").clicked() {
                            (config.width, config.height) = (0, 0);
                        }
                    }
                    ui.checkbox(&mut config.taa, "Anti-aliasing");
                    ui.add(egui::Slider::new(&mut config.taa_blend, 0.0..=1.0).text("AA blend"))
                        .on_hover_text("Part of a new frame in the picture (0: reference mean)");
                    ui.add(
                        egui::Slider::new(&mut config.texture_bias, -4.0..=4.0).text("Mip bias"),
                    )
                    .on_hover_text("Texture mip bias: below 0 is sharper");
                });
            egui::CollapsingHeader::new("Look")
                .default_open(true)
                .show(ui, |ui| {
                    ui.add(
                        egui::Slider::new(&mut config.exposure, 0.1..=8.0)
                            .logarithmic(true)
                            .text("Exposure"),
                    );
                    ui.horizontal(|ui| {
                        ui.label("Tone curve");
                        ui.radio_value(&mut config.tone_curve, ToneCurve::Agx, "AgX");
                        ui.radio_value(&mut config.tone_curve, ToneCurve::Neutral, "PBR Neutral");
                    });
                    ui.add(egui::Slider::new(&mut config.bloom, 0.0..=0.5).text("Bloom"));
                    ui.add(egui::Slider::new(&mut config.vignette, 0.0..=1.0).text("Vignette"));
                    // R, G, B of each grade control.
                    for (name, values, range) in [
                        ("Lift", &mut config.grade_lift, -0.2..=0.2),
                        ("Gamma", &mut config.grade_gamma, 0.5..=2.0),
                        ("Gain", &mut config.grade_gain, 0.5..=2.0),
                    ] {
                        ui.horizontal(|ui| {
                            ui.add_sized([48.0, 18.0], egui::Label::new(name));
                            for (value, channel) in values.iter_mut().zip(["R ", "G ", "B "]) {
                                ui.add(
                                    egui::DragValue::new(value)
                                        .prefix(channel)
                                        .speed(0.005)
                                        .range(range.clone()),
                                );
                            }
                        });
                    }
                });
            ui.collapsing("Path tracer", |ui| {
                ui.add(
                    egui::Slider::new(&mut config.samples_per_frame, 0..=64)
                        .text("Samples per frame"),
                )
                .on_hover_text("0: automatic, in the trace budget");
                ui.add(
                    egui::Slider::new(&mut config.trace_budget_ms, 0.5..=33.0)
                        .text("Trace budget (ms)"),
                )
                .on_hover_text("Used only when the samples per frame are automatic");
                ui.add(egui::Slider::new(&mut config.max_path_rays, 2..=32).text("Rays per path"));
                ui.checkbox(&mut config.blue_noise, "Blue noise");
                ui.add(egui::Slider::new(&mut config.max_bounces, 1..=16).text("Max bounces"))
                    .on_hover_text("Diffuse bounces of a path");
                ui.checkbox(&mut config.bounce_light, "Bounce light")
                    .on_hover_text("Uses the bounce light of the frame before");
            });
            ui.collapsing("Denoiser", |ui| {
                ui.add(
                    egui::Slider::new(&mut config.denoise_passes, 0..=MAX_DENOISE_PASSES)
                        .text("Passes: indirect"),
                )
                .on_hover_text("Filter passes of the indirect light");
                ui.add(
                    egui::Slider::new(&mut config.denoise_direct_passes, 0..=MAX_DENOISE_PASSES)
                        .text("Passes: direct"),
                )
                .on_hover_text("Filter passes of the direct light");
                ui.add(
                    egui::Slider::new(&mut config.denoise_specular_passes, 0..=MAX_DENOISE_PASSES)
                        .text("Passes: reflections"),
                )
                .on_hover_text("Filter passes of the rough reflections");
                ui.add(
                    egui::Slider::new(&mut config.denoise_reach, 0.0..=4096.0)
                        .logarithmic(true)
                        .text("Reach"),
                )
                .on_hover_text("Filter reach. 0: all passes at each pixel");
                ui.add(
                    egui::Slider::new(&mut config.indirect_clamp, 0.0..=64.0)
                        .text("Bounce light limit"),
                )
                .on_hover_text("0: no limit");
                ui.add(
                    egui::Slider::new(&mut config.history_clamp, 0.0..=8.0).text("History test"),
                )
                .on_hover_text("Direct light history test, in deviations. 0: off");
                ui.add(
                    egui::Slider::new(&mut config.light_history_samples, 0..=1024)
                        .logarithmic(true)
                        .text("Kept on light change"),
                )
                .on_hover_text("Indirect samples kept when a light changes");
                ui.add(
                    egui::Slider::new(&mut config.max_samples, 1..=16384)
                        .logarithmic(true)
                        .text("Kept at rest"),
                )
                .on_hover_text("Samples kept while the view does not move");
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
