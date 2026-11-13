//! The debug panel of the gameplay modules: their settings that have an effect during
//! a run. Nothing is written to the config file.

use maki_core::{App, Module};
use maki_gameplay::{CameraSettings, PlayerDef};
use maki_ui::{add_panel, egui};

/// Adds the "Gameplay" panel. Needs `UiModule` to show it. A section shows only if
/// the game added the module of its settings.
pub struct GameplayPanelModule;

impl Module for GameplayPanelModule {
    fn build(&self, app: &mut App) {
        add_panel(app, "Gameplay", true, |ui, world| {
            ui.weak("Changes last until the game stops. They are not saved.");
            let mut shown = false;
            if let Some(old) = world.get_resource::<CameraSettings>() {
                shown = true;
                let mut camera = old.clone();
                section(ui, "Camera", |ui| {
                    ui.add(
                        egui::Slider::new(&mut camera.fov_degrees, 10.0..=120.0)
                            .text("Field of view, deg"),
                    );
                    ui.add(
                        egui::Slider::new(&mut camera.follow_time, 0.0..=2.0)
                            .text("Follow time, s"),
                    );
                    ui.add(
                        egui::Slider::new(&mut camera.margin, 0.0..=10.0).text("Wall margin, m"),
                    );
                    // The two points give the view direction and the distance.
                    for (name, point) in [
                        ("Position", &mut camera.position),
                        ("Target", &mut camera.target),
                    ] {
                        ui.horizontal(|ui| {
                            ui.label(name);
                            for (axis, value) in ["x ", "y ", "z "].into_iter().zip(point) {
                                let drag = egui::DragValue::new(value).speed(0.05);
                                ui.add(drag.prefix(axis).suffix(" m"));
                            }
                        });
                    }
                });
                // Only a different value is a write.
                if camera != *old {
                    *world.resource_mut::<CameraSettings>() = camera;
                }
            }
            // The values of the player entity (from its prefab file), not of a file.
            let mut players = world.query::<&mut PlayerDef>();
            if let Some(old) = players.iter(world).next().cloned() {
                shown = true;
                let mut player = old.clone();
                section(ui, "Player", |ui| {
                    ui.add(
                        egui::Slider::new(&mut player.walk_speed, 0.0..=10.0)
                            .text("Walk speed, m/s"),
                    );
                    ui.add(
                        egui::Slider::new(&mut player.run_speed, 0.0..=10.0).text("Run speed, m/s"),
                    );
                });
                if player != old {
                    for mut def in players.iter_mut(world) {
                        *def = player.clone();
                    }
                }
            }
            if !shown {
                ui.weak("No gameplay settings. Play the game, or add the gameplay modules.");
            }
        });
    }
}

/// A section that starts open.
fn section(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(title)
        .default_open(true)
        .show(ui, body);
}
