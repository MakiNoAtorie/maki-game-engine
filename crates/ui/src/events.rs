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
            ui.separator();
            running_events(ui, world);
        });
    }
}

/// The content of the "Game state" panel: each flag and variable, to see and to set.
/// `new_name`: the text of its name field, kept by the caller. A tool shows it at a
/// place of its own, and the running events in another panel.
pub fn game_state_panel(
    ui: &mut egui::Ui,
    world: &mut bevy_ecs::world::World,
    new_name: &mut String,
) {
    let old = world.resource::<GameState>();
    let mut state = old.clone();
    ui.strong("Flags");
    for (name, value) in &mut state.flags {
        ui.checkbox(value, name.as_str());
    }
    if state.flags.is_empty() {
        ui.weak("No flags");
    }
    ui.strong("Variables");
    for (name, value) in &mut state.variables {
        ui.add(egui::DragValue::new(value).prefix(format!("{name}: ")));
    }
    if state.variables.is_empty() {
        ui.weak("No variables");
    }
    ui.horizontal(|ui| {
        let field = egui::TextEdit::singleline(new_name)
            .hint_text("Name")
            .desired_width(120.0);
        let field = ui.add(field);
        let name = new_name.trim().to_owned();
        let filled = !name.is_empty();
        // Enter adds a flag.
        let enter = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
        if (ui
            .add_enabled(filled, egui::Button::new("Add flag"))
            .clicked()
            || enter)
            && filled
        {
            state.set_flag(&name, false);
            new_name.clear();
        }
        if ui
            .add_enabled(filled, egui::Button::new("Add variable"))
            .clicked()
        {
            state.set_variable(&name, 0);
            new_name.clear();
        }
    });
    // Only a different state is a write.
    if state != *old {
        *world.resource_mut::<GameState>() = state;
    }
}

/// The events that run, with the command at which each one is.
fn running_events(ui: &mut egui::Ui, world: &bevy_ecs::world::World) {
    ui.strong("Running events");
    let runner = world.resource::<EventRunner>();
    for event in runner.running() {
        let command = event.current().map_or(String::new(), |c| format!("{c:?}"));
        ui.label(format!("{} ({:?}): {command}", event.name, event.start));
    }
    if runner.running().is_empty() {
        ui.weak("None");
    }
}
