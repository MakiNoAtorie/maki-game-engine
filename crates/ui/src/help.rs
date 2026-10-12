//! The help panel: the controls, made from the binding table. No shortcut text is
//! written here, so a changed binding changes the panel.

use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Res, ResMut};
use bevy_ecs::world::World;
use maki_core::{App, Update};
use maki_platform::{Action, ActionState, Bindings, Key, add_bindings};

use crate::{Panels, add_panel, run_ui};

pub(crate) const HELP: &str = "Help";

/// Shows or hides the help panel.
pub const TOGGLE_HELP: Action = Action("toggle_help");

/// Adds the help panel and its toggle. Part of `UiModule`.
pub(crate) fn build(app: &mut App) {
    add_bindings(app, &[(Key::F1, TOGGLE_HELP, "Toggle help panel")]);
    add_panel(app, HELP, true, help_panel);
    // Before the egui frame: the panel opens or closes in the frame of the key press.
    app.add_systems(Update, toggle_help.before(run_ui));
}

fn toggle_help(actions: Res<ActionState>, mut panels: ResMut<Panels>) {
    if actions.just_pressed(TOGGLE_HELP) {
        panels.toggle(HELP);
    }
}

/// The text of the panel: key and description of each binding, in table order.
pub(crate) fn rows(bindings: &Bindings) -> Vec<(&'static str, &'static str)> {
    let row = |binding: &maki_platform::Binding| (binding.key.label(), binding.description);
    bindings.0.iter().map(row).collect()
}

fn help_panel(ui: &mut egui::Ui, world: &mut World) {
    egui::Grid::new("bindings")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            for (key, description) in rows(world.resource::<Bindings>()) {
                ui.label(key);
                ui.label(description);
                ui.end_row();
            }
        });
}
