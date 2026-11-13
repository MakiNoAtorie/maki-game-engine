//! The help panel: the controls, made from the binding table. No shortcut text is
//! written here, so a changed binding changes the panel. It shows the bindings of
//! the current input mode.

use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Res, ResMut};
use bevy_ecs::world::World;
use maki_core::{App, Update};
use maki_platform::{Action, ActionState, Bindings, InputMode, Key, add_bindings};

use crate::{Panels, ToolCommand, ToolCommands, add_panel, run_ui};

pub(crate) const HELP: &str = "Help";

/// Shows or hides the help panel.
pub const TOGGLE_HELP: Action = Action("toggle_help");

/// Adds the help panel and its toggle. Part of `UiModule`.
pub(crate) fn build(app: &mut App) {
    add_bindings(app, &[(Key::F1, TOGGLE_HELP, "Toggle help panel")]);
    add_panel(app, HELP, true, help_panel);
    // Open at the start: F1 shows the controls, not a folded header.
    app.resource_mut::<Panels>().unfold(HELP);
    // Before the egui frame: the panel opens or closes in the frame of the key press.
    app.add_systems(Update, toggle_help.before(run_ui));
}

fn toggle_help(actions: Res<ActionState>, mut panels: ResMut<Panels>) {
    if actions.just_pressed(TOGGLE_HELP) {
        panels.toggle(HELP);
    }
}

/// The text of the panel: key and description of each binding of a mode, in table
/// order. `command`: the name of the command of a tool that an action starts now;
/// it is the text in place of the description.
pub(crate) fn rows(
    bindings: &Bindings,
    mode: InputMode,
    command: impl Fn(Action) -> Option<&'static str>,
) -> Vec<(String, &'static str)> {
    let row = |binding: &maki_platform::Binding| {
        let text = command(binding.action).unwrap_or(binding.description);
        (binding.chord.label(), text)
    };
    let of_mode = bindings.0.iter().filter(|binding| binding.mode == mode);
    of_mode.map(row).collect()
}

fn help_panel(ui: &mut egui::Ui, world: &mut World) {
    egui::Grid::new("bindings")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            let mode = *world.resource::<InputMode>();
            let commands = world.get_resource::<ToolCommands>().cloned();
            let commands = commands.unwrap_or_default();
            // Commands of other panels can share a key: the one that can run now names it.
            let command = |action| {
                let of_key: Vec<&ToolCommand> = (commands.iter())
                    .filter(|command| command.action == Some(action))
                    .collect();
                let can_run = of_key.iter().find(|command| (command.can_run)(world));
                can_run.or(of_key.first()).map(|command| command.name)
            };
            for (key, text) in rows(world.resource::<Bindings>(), mode, command) {
                ui.monospace(key);
                ui.label(text);
                ui.end_row();
            }
        });
}
