//! What the developer sees of the events: the debug panel of the game state. The
//! text box of the game is in `game_ui`.

use maki_core::{App, Module};
use maki_events::{EventRunner, GameState};

use crate::add_panel;

/// Adds the "Game state" panel. Needs `UiModule` to show it.
pub struct EventsUiModule;

impl Module for EventsUiModule {
    fn build(&self, app: &mut App) {
        // State of the panel: the text of the name field.
        let mut new_name = String::new();
        add_panel(app, "Game state", true, move |ui, world| {
            game_state_panel(ui, world, &mut new_name);
        });
    }
}

/// The content of the "Game state" panel: each flag and variable, to see and to set,
/// and the events that run. `new_name`: the text of its name field, kept by the
/// caller. A tool shows it at a place of its own.
pub fn game_state_panel(
    ui: &mut egui::Ui,
    world: &mut bevy_ecs::world::World,
    new_name: &mut String,
) {
    {
        {
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
                ui.add(egui::TextEdit::singleline(new_name).desired_width(120.0));
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
                ui.label(format!("{} ({:?}): {command}", event.name, event.start));
            }
            if runner.running().is_empty() {
                ui.weak("none");
            }
        }
    }
}
