//! What the player and the developer see of the events: the text box of the game
//! (a message, a question with its answers), and the debug panel of the game state.

use bevy_ecs::system::{Res, ResMut};
use maki_core::{App, Module, Update};
use maki_events::{
    CHOICE_DOWN, CHOICE_UP, ChoiceBox, Choices, EventRunner, GameState, INTERACT, MessageBox,
};
use maki_platform::{Action, Bindings};

use crate::{Panels, add_overlay, add_panel};

const MESSAGE: &str = "Message";

/// Adds the text box of the game and the "Game state" panel. Needs `UiModule` to
/// show them.
pub struct EventsUiModule;

impl Module for EventsUiModule {
    fn build(&self, app: &mut App) {
        // The text of a `Message` or a `Choice` command, on the picture. `MessageBox`
        // and `ChoiceBox` are the interface: the events know nothing of this box.
        add_overlay(app, MESSAGE, false, |ui, world| {
            let Some(text) = world.resource::<MessageBox>().0.clone() else {
                return;
            };
            let choices = world.resource::<ChoiceBox>().0.clone();
            let bindings = world.resource::<Bindings>();
            let go_on = keys(bindings, INTERACT);
            // The keys that the box tells the player.
            let hint = match &choices {
                Some(_) => {
                    let (up, down) = (keys(bindings, CHOICE_UP), keys(bindings, CHOICE_DOWN));
                    format!("{up} / {down}: another answer.  {go_on}: take it.")
                }
                None => go_on,
            };
            text_box(ui, &text, choices.as_ref(), &hint);
        });
        app.add_systems(Update, show_message);

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

/// The box of the game for a text: at the bottom of the picture, as wide as the
/// picture up to a limit. `choices`: the answers of a question, with a mark at the
/// selected one. `hint`: the keys, in small text.
fn text_box(ui: &mut egui::Ui, text: &str, choices: Option<&Choices>, hint: &str) {
    const MARGIN: f32 = 24.0;
    const TEXT: egui::Color32 = egui::Color32::from_gray(235);
    const MARK: egui::Color32 = egui::Color32::from_rgb(255, 225, 120);
    // The picture: the UI gives it to an overlay as its clip rectangle.
    let picture = ui.clip_rect();
    let width = (picture.width() - 2.0 * MARGIN).clamp(120.0, 760.0);
    let frame = egui::Frame::new()
        .fill(egui::Color32::from_rgba_unmultiplied(10, 10, 14, 235))
        .stroke(egui::Stroke::new(1.5, egui::Color32::from_gray(170)))
        .corner_radius(6)
        .inner_margin(16);
    egui::Area::new(egui::Id::new("text box of the game"))
        .pivot(egui::Align2::CENTER_BOTTOM)
        .fixed_pos(picture.center_bottom() - egui::vec2(0.0, MARGIN))
        .interactable(false)
        .show(ui.ctx(), |ui| {
            frame.show(ui, |ui| {
                // Less than the full width: the frame has its margin.
                ui.set_width(width - 32.0);
                ui.label(egui::RichText::new(text).size(20.0).color(TEXT));
                if let Some(choices) = choices {
                    ui.add_space(8.0);
                    for (index, answer) in choices.answers.iter().enumerate() {
                        let selected = index == choices.selected;
                        let (mark, color) = if selected {
                            ("> ", MARK)
                        } else {
                            ("   ", TEXT)
                        };
                        let line = egui::RichText::new(format!("{mark}{answer}")).size(20.0);
                        ui.label(line.color(color));
                    }
                }
                ui.add_space(6.0);
                ui.label(egui::RichText::new(hint).size(13.0).weak());
            });
        });
}

/// The keys of an action, as the binding table has them: "Z or Enter".
fn keys(bindings: &Bindings, action: Action) -> String {
    let keys = bindings.0.iter().filter(|binding| binding.action == action);
    let labels: Vec<_> = keys.map(|binding| binding.chord.label()).collect();
    labels.join(" or ")
}

/// The text box is open while there is a message.
fn show_message(message: Res<MessageBox>, mut panels: ResMut<Panels>) {
    let open = message.0.is_some();
    if panels.is_open(MESSAGE) != open {
        panels.set_open(MESSAGE, open);
    }
}
