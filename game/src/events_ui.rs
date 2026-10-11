//! What the player and the developer see of the events: the message window, and the
//! debug panel of the game state.

use bevy_ecs::system::{Res, ResMut};
use maki_core::{App, Module, Update};
use maki_events::{EventRunner, GameState, MessageBox};
use maki_ui::{Panels, add_panel, egui};

const MESSAGE: &str = "Message";

/// Adds the "Message" window and the "Game state" panel. Needs `UiModule` to show them.
pub struct EventsUiModule;

impl Module for EventsUiModule {
    fn build(&self, app: &mut App) {
        // The text of the `Message` command. A plain egui window: `MessageBox` is
        // the interface, so a styled text box can replace this with no change to the
        // events.
        add_panel(app, MESSAGE, false, |ui, world| {
            if let Some(text) = &world.resource::<MessageBox>().0 {
                ui.label(egui::RichText::new(text).size(18.0));
                ui.weak("Z or Enter");
            }
        });
        app.add_systems(Update, show_message);

        // State of the panel: the text of the name field.
        let mut new_name = String::new();
        add_panel(app, "Game state", true, move |ui, world| {
            let old = world.resource::<GameState>();
            let mut state = old.clone();
            ui.label("Flags");
            for (name, value) in &mut state.flags {
                ui.checkbox(value, name.as_str());
            }
            ui.label("Variables");
            for (name, value) in &mut state.variables {
                ui.add(egui::DragValue::new(value).prefix(format!("{name}: ")));
            }
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut new_name).desired_width(120.0));
                let name = new_name.trim();
                if ui.button("Add flag").clicked() && !name.is_empty() {
                    state.set_flag(name, false);
                }
                if ui.button("Add variable").clicked() && !name.is_empty() {
                    state.set_variable(name, 0);
                }
            });
            // Only a different state is a write.
            if state != *old {
                *world.resource_mut::<GameState>() = state;
            }

            ui.separator();
            ui.label("Running events");
            let runner = world.resource::<EventRunner>();
            for event in runner.running() {
                let command = event.current().map_or(String::new(), |c| format!("{c:?}"));
                ui.label(format!("{} ({:?}): {command}", event.name, event.trigger));
            }
            if runner.running().is_empty() {
                ui.weak("none");
            }
        });
    }
}

/// The message window is open while there is a message.
fn show_message(message: Res<MessageBox>, mut panels: ResMut<Panels>) {
    let open = message.0.is_some();
    if panels.is_open(MESSAGE) != open {
        panels.set_open(MESSAGE, open);
    }
}
