//! The debug panel of the gameplay modules: their settings that have an effect during
//! a run. Nothing is written to the config file.

use maki_core::{App, Module};
use maki_gameplay::{CameraSettings, PlayerSettings};
use maki_ui::{add_panel, egui};

/// Adds the "Gameplay" panel. Needs `UiModule` to show it. A section shows only if
/// the game added the module of its settings.
pub struct GameplayPanelModule;

impl Module for GameplayPanelModule {
    fn build(&self, app: &mut App) {
        add_panel(app, "Gameplay", true, |ui, world| {
            if let Some(old) = world.get_resource::<CameraSettings>() {
                let mut camera = old.clone();
                ui.collapsing("Camera", |ui| {
                    ui.add(
                        egui::Slider::new(&mut camera.fov_degrees, 10.0..=120.0)
                            .text("field of view"),
                    );
                    ui.add(
                        egui::Slider::new(&mut camera.follow_time, 0.0..=2.0)
                            .text("follow time, s"),
                    );
                    ui.add(
                        egui::Slider::new(&mut camera.margin, 0.0..=10.0).text("wall margin, m"),
                    );
                    // The two points give the view direction and the distance.
                    for (name, point) in [
                        ("position", &mut camera.position),
                        ("target", &mut camera.target),
                    ] {
                        ui.horizontal(|ui| {
                            for value in point {
                                ui.add(egui::DragValue::new(value).speed(0.05));
                            }
                            ui.label(name);
                        });
                    }
                });
                // Only a different value is a write.
                if camera != *old {
                    *world.resource_mut::<CameraSettings>() = camera;
                }
            }
            if let Some(old) = world.get_resource::<PlayerSettings>() {
                let mut player = old.clone();
                ui.collapsing("Player", |ui| {
                    ui.add(
                        egui::Slider::new(&mut player.walk_speed, 0.0..=10.0)
                            .text("walk speed, m/s"),
                    );
                    ui.add(
                        egui::Slider::new(&mut player.run_speed, 0.0..=10.0).text("run speed, m/s"),
                    );
                });
                if player != *old {
                    *world.resource_mut::<PlayerSettings>() = player;
                }
            }
        });
    }
}
