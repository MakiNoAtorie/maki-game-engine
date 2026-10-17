//! The UI of a game: what the player sees over the picture. The text box (a message,
//! a question with its answers), and the open menus.
//!
//! The module draws what resources of `maki_events` hold (`MessageBox`, `ChoiceBox`,
//! `Menus`):
//! the events and the gameplay know nothing of egui (rule 1 of `CLAUDE.md`).
//!
//! The text of the game has a font family of its own, [`GAME_FONT`]: the font file of
//! `[game_ui] font` first (for the glyphs of a language, such as Japanese), then the
//! default fonts. The editor keeps its own fonts.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bevy_ecs::message::MessageReader;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Res, ResMut};
use maki_assets::AssetChanged;
use maki_core::{App, Module, Settings, Startup, Update};
use maki_events::{
    CHOICE_DOWN, CHOICE_UP, ChoiceBox, Choices, INTERACT, MenuPlace, Menus, MessageBox, ShownMenu,
    shown_menus,
};
use maki_platform::{Action, Bindings};
use serde::{Deserialize, Serialize};

use crate::{Panels, UiContext, add_overlay};

const MESSAGE: &str = "Message";
const MENUS: &str = "Menus";
const TEXT: egui::Color32 = egui::Color32::from_gray(235);
const MARK: egui::Color32 = egui::Color32::from_rgb(255, 225, 120);

/// The name of the font family of the game.
pub const GAME_FONT: &str = "game";

/// The `[game_ui]` table of the config file.
#[derive(Resource, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GameUiSettings {
    /// A TTF or OTF file, from the working directory: the first font of the text of
    /// the game. None: the default fonts.
    pub font: Option<PathBuf>,
    /// The size of the text of the game, as a part of the default size.
    pub scale: f32,
}

impl Default for GameUiSettings {
    fn default() -> Self {
        Self {
            font: None,
            scale: 1.0,
        }
    }
}

impl Settings for GameUiSettings {
    const TABLE: &'static str = "game_ui";

    fn check(&self) -> Result<(), String> {
        match self.scale > 0.0 {
            true => Ok(()),
            false => Err("scale must be above 0".to_owned()),
        }
    }
}

/// Adds the UI of the game: the text box and the menus. Needs `UiModule` to show it.
pub struct GameUiModule;

impl Module for GameUiModule {
    fn build(&self, app: &mut App) {
        app.add_settings::<GameUiSettings>();
        let scale = app.world().resource::<GameUiSettings>().scale;
        add_overlay(app, MENUS, false, move |ui, world| {
            for (index, menu) in shown_menus(world).iter().enumerate() {
                menu_box(ui, index, menu, scale);
            }
        });
        // The text of a `Message` or a `Choice` command, on the picture.
        add_overlay(app, MESSAGE, false, move |ui, world| {
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
            text_box(ui, &text, choices.as_ref(), &hint, scale);
        });
        app.add_message::<AssetChanged>()
            .add_systems(Startup, install_fonts)
            .add_systems(Update, (show_message, show_menus, font_changed));
    }
}

/// The fonts of egui with the family of the game: the font file first, then the
/// default fonts. `Err`: the file does not read; the family has the default fonts.
pub fn game_fonts(font: Option<&Path>) -> (egui::FontDefinitions, Result<(), String>) {
    let mut fonts = egui::FontDefinitions::default();
    let mut family = fonts.families[&egui::FontFamily::Proportional].clone();
    let mut result = Ok(());
    if let Some(path) = font {
        match std::fs::read(path) {
            Ok(bytes) => {
                let data = egui::FontData::from_owned(bytes);
                fonts.font_data.insert(GAME_FONT.to_owned(), Arc::new(data));
                family.insert(0, GAME_FONT.to_owned());
            }
            Err(error) => {
                result = Err(format!(
                    "the font {} does not read: {error}",
                    path.display()
                ))
            }
        }
    }
    let name = egui::FontFamily::Name(GAME_FONT.into());
    fonts.families.insert(name, family);
    (fonts, result)
}

/// Runs in `Startup`, and again when the font file changes: gives egui the fonts.
fn install_fonts(ui: Option<Res<UiContext>>, settings: Res<GameUiSettings>) {
    let Some(ui) = ui else { return };
    let (fonts, result) = game_fonts(settings.font.as_deref());
    if let Err(error) = result {
        tracing::error!("game UI: {error}; the default fonts show");
    }
    ui.ctx.set_fonts(fonts);
}

/// Runs in `Update`: a changed font file is read again.
fn font_changed(
    mut changed: MessageReader<AssetChanged>,
    ui: Option<Res<UiContext>>,
    settings: Res<GameUiSettings>,
) {
    let font = settings.font.as_deref();
    if changed
        .read()
        .any(|AssetChanged(path)| Some(path.as_path()) == font)
    {
        install_fonts(ui, settings);
    }
}

/// The text box of the game at the bottom of the picture: the text, the answers of a
/// question with a mark at the selected one, and the keys to go on. `scale`: of the
/// text sizes.
fn text_box(ui: &mut egui::Ui, text: &str, choices: Option<&Choices>, hint: &str, scale: f32) {
    const MARGIN: f32 = 24.0;
    let font = |size: f32| game_font(size, scale);
    // The picture: the UI gives it to an overlay as its clip rectangle.
    let picture = ui.clip_rect();
    let width = (picture.width() - 2.0 * MARGIN).clamp(120.0, 760.0);
    let frame = box_frame();
    let shown = egui::Area::new(egui::Id::new("text box of the game"))
        .pivot(egui::Align2::CENTER_BOTTOM)
        .fixed_pos(picture.center_bottom() - egui::vec2(0.0, MARGIN))
        .interactable(false)
        .show(ui.ctx(), |ui| {
            frame.show(ui, |ui| {
                // Less than the full width: the frame has its margin.
                ui.set_width(width - 32.0);
                ui.label(egui::RichText::new(text).font(font(20.0)).color(TEXT));
                if let Some(choices) = choices {
                    ui.add_space(8.0);
                    for (index, answer) in choices.answers.iter().enumerate() {
                        let selected = index == choices.selected;
                        let (mark, color) = if selected {
                            ("> ", MARK)
                        } else {
                            ("   ", TEXT)
                        };
                        let line = egui::RichText::new(format!("{mark}{answer}")).font(font(20.0));
                        ui.label(line.color(color));
                    }
                }
                ui.add_space(6.0);
                ui.label(egui::RichText::new(hint).font(font(13.0)).weak());
            });
        });
    // Over a menu that opened after it.
    ui.ctx().move_to_top(shown.response.layer_id);
}

/// The font of the text of the game, of a size of the default scale.
fn game_font(size: f32, scale: f32) -> egui::FontId {
    egui::FontId::new(size * scale, egui::FontFamily::Name(GAME_FONT.into()))
}

/// The frame of a box of the game: the text box, a menu.
fn box_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(egui::Color32::from_rgba_unmultiplied(10, 10, 14, 235))
        .stroke(egui::Stroke::new(1.5, egui::Color32::from_gray(170)))
        .corner_radius(6)
        .inner_margin(16)
}

/// A menu at its place on the picture: the title, and the rows with a mark at the
/// row of the focus. `index`: in the stack, for the id of its area.
fn menu_box(ui: &mut egui::Ui, index: usize, menu: &ShownMenu, scale: f32) {
    const MARGIN: f32 = 24.0;
    let align = match menu.place {
        MenuPlace::Center => egui::Align2::CENTER_CENTER,
        MenuPlace::Top => egui::Align2::CENTER_TOP,
        MenuPlace::Bottom => egui::Align2::CENTER_BOTTOM,
        MenuPlace::TopLeft => egui::Align2::LEFT_TOP,
        MenuPlace::TopRight => egui::Align2::RIGHT_TOP,
        MenuPlace::BottomLeft => egui::Align2::LEFT_BOTTOM,
        MenuPlace::BottomRight => egui::Align2::RIGHT_BOTTOM,
    };
    let picture = ui.clip_rect().shrink(MARGIN);
    egui::Area::new(egui::Id::new(("menu of the game", index)))
        .pivot(align)
        .fixed_pos(align.pos_in_rect(&picture))
        .interactable(false)
        .show(ui.ctx(), |ui| {
            box_frame().show(ui, |ui| {
                if let Some(title) = &menu.title {
                    ui.label(
                        egui::RichText::new(title)
                            .font(game_font(22.0, scale))
                            .color(TEXT),
                    );
                    ui.add_space(8.0);
                }
                for row in &menu.rows {
                    let (mark, color) = if row.focus {
                        ("> ", MARK)
                    } else {
                        ("   ", TEXT)
                    };
                    let count = row
                        .count
                        .map_or(String::new(), |count| format!("  x{count}"));
                    let line = egui::RichText::new(format!("{mark}{}{count}", row.text));
                    ui.label(line.font(game_font(20.0, scale)).color(color));
                }
                if let Some(description) = &menu.description {
                    ui.add_space(8.0);
                    let line = egui::RichText::new(description);
                    ui.label(line.font(game_font(18.0, scale)).color(TEXT));
                }
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

/// The menus show while one is open.
fn show_menus(menus: Res<Menus>, mut panels: ResMut<Panels>) {
    let open = !menus.0.is_empty();
    if panels.is_open(MENUS) != open {
        panels.set_open(MENUS, open);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The texts of the text box in egui frames. The second frame: egui measures a
    /// new area in its first one and does not show it.
    fn shown(text: &str, choices: Option<&Choices>) -> Vec<String> {
        drawn(|ui| text_box(ui, text, choices, "Z: go on", 1.0))
    }

    /// The texts that a drawing shows, in the second of two egui frames.
    fn drawn(mut draw: impl FnMut(&mut egui::Ui)) -> Vec<String> {
        let ctx = egui::Context::default();
        let (fonts, _) = game_fonts(None);
        ctx.set_fonts(fonts);
        let mut frame = || {
            let mut output = ctx.run_ui(egui::RawInput::default(), &mut draw);
            // No renderer takes the font image: egui panics at a delta that nobody read.
            output.textures_delta.clear();
            output
        };
        frame();
        let output = frame();
        let mut texts = Vec::new();
        for shape in output.shapes {
            if let egui::epaint::Shape::Text(text) = shape.shape {
                texts.push(text.galley.text().to_owned());
            }
        }
        texts
    }

    #[test]
    fn the_text_box_shows_a_message_and_a_question() {
        assert_eq!(
            shown("The door is locked.", None),
            ["The door is locked.", "Z: go on"]
        );
        let choices = Choices {
            answers: vec!["Yes".into(), "No".into()],
            selected: 1,
        };
        let texts = shown("Open it?", Some(&choices));
        assert_eq!(texts, ["Open it?", "   Yes", "> No", "Z: go on"]);
    }

    #[test]
    fn a_menu_shows_its_title_and_its_rows_with_a_mark_at_the_focus() {
        let row = |text: &str, focus| maki_events::ShownRow {
            text: text.to_owned(),
            focus,
            count: None,
        };
        let menu = ShownMenu {
            place: MenuPlace::TopRight,
            title: Some("Pause".to_owned()),
            rows: vec![row("Back", false), row("Items", true)],
            description: None,
        };
        let texts = drawn(|ui| menu_box(ui, 0, &menu, 1.0));
        assert_eq!(texts, ["Pause", "   Back", "> Items"]);
    }

    #[test]
    fn a_menu_shows_the_names_and_counts_of_items_and_the_description_of_the_focus() {
        let item = |text: &str, focus, count| maki_events::ShownRow {
            text: text.to_owned(),
            focus,
            count: Some(count),
        };
        let menu = ShownMenu {
            place: MenuPlace::Center,
            title: None,
            rows: vec![item("Key", true, 2), item("Coin", false, 15)],
            description: Some("Opens the door.".to_owned()),
        };
        let texts = drawn(|ui| menu_box(ui, 0, &menu, 1.0));
        assert_eq!(texts, ["> Key  x2", "   Coin  x15", "Opens the door."]);
    }

    #[test]
    fn a_font_file_is_the_first_font_of_the_game_and_the_editor_keeps_its_fonts() {
        // A font file of the test: the bytes of a default font of egui.
        let defaults = egui::FontDefinitions::default();
        let first = &defaults.families[&egui::FontFamily::Proportional][0];
        let path = std::env::temp_dir().join(format!("maki-font-{}.ttf", std::process::id()));
        std::fs::write(&path, &*defaults.font_data[first].font).unwrap();
        let (fonts, result) = game_fonts(Some(&path));
        assert!(result.is_ok());
        let game = &fonts.families[&egui::FontFamily::Name(GAME_FONT.into())];
        assert_eq!(game[0], GAME_FONT);
        assert_eq!(
            &game[1..],
            defaults.families[&egui::FontFamily::Proportional]
        );
        // The families of the editor are as before.
        let proportional = egui::FontFamily::Proportional;
        assert_eq!(
            fonts.families[&proportional],
            defaults.families[&proportional]
        );
        std::fs::remove_file(path).unwrap();

        // No file: an error, and the family of the game has the default fonts.
        let (fonts, result) = game_fonts(Some(Path::new("no/such/font.ttf")));
        assert!(result.unwrap_err().contains("does not read"));
        let game = &fonts.families[&egui::FontFamily::Name(GAME_FONT.into())];
        assert_eq!(game, &defaults.families[&egui::FontFamily::Proportional]);
    }
}
