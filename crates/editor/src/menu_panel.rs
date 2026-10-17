//! A menu file in the event panel: its fields and its rows at the left, the
//! conditions and the commands of the selected row in the middle, the arguments of the
//! selected command at the right. A change is written to the file at once, with no
//! undo, as a routine is.

use std::path::{Path, PathBuf};

use bevy_ecs::world::World;
use maki_assets::DATA_FOLDER;
use maki_core::fields::Fields;
use maki_events::{Command, CommandRegistry, Menu, MenuRow, TextKey, menu_text, read_menu};
use maki_scene::RoomDef;
use maki_ui::egui::{self, Ui};

use crate::Editor;
use crate::event_panel::{
    ArgumentEditors, EventUi, Lists, cached_live_lists, command_arguments, command_rows,
    conditions, lists_and_problems, pick_key, with_picks,
};

/// The folder of the menu files, in the data folder of the room directory.
const FOLDER: &str = "menus";
/// The end of the name of the menu files.
const EXTENSION: &str = "menu.ron";

/// A menu file that the event panel edits in place of an event.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuEdit {
    pub path: PathBuf,
    pub menu: Menu,
    /// The row whose conditions and commands show.
    pub row: usize,
}

impl MenuEdit {
    pub fn open(path: &Path, registry: &CommandRegistry) -> Result<Self, String> {
        Ok(Self {
            path: path.to_owned(),
            menu: read_menu(path, registry)?,
            row: 0,
        })
    }

    pub fn save(&self) -> Result<(), String> {
        std::fs::write(&self.path, menu_text(&self.menu))
            .map_err(|error| format!("{} not written: {error}", self.path.display()))
    }
}

/// Writes an empty menu file `data/menus/<name>.menu.ron` in `directory`: its path.
/// `Err`: why no file was written.
pub fn new_menu(directory: &Path, name: &str) -> Result<PathBuf, String> {
    let name = name.trim();
    let letters = |c: char| c.is_alphanumeric() || c == '_' || c == '-' || c == ' ';
    if name.is_empty() || !name.chars().all(letters) {
        return Err("A name of a menu has letters, numbers, `_`, and `-`.".to_owned());
    }
    let folder = directory.join(DATA_FOLDER).join(FOLDER);
    let path = folder.join(format!("{name}.{EXTENSION}"));
    if path.exists() {
        return Err(format!("{} is there already.", path.display()));
    }
    let written = std::fs::create_dir_all(&folder)
        .and_then(|()| std::fs::write(&path, menu_text(&Menu::default())));
    written.map_err(|error| format!("{} not written: {error}", path.display()))?;
    Ok(path)
}

/// The menu files under `directory`, sorted.
fn menu_files(directory: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut open = vec![directory.to_owned()];
    while let Some(folder) = open.pop() {
        for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if path.is_dir() {
                open.push(path);
            } else if name.ends_with(&format!(".{EXTENSION}")) {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

impl Editor {
    /// Opens a menu file in the event panel.
    pub fn open_menu(&mut self, path: &Path) {
        match MenuEdit::open(path, &self.registry) {
            Ok(edit) => {
                self.menu = Some(edit);
                self.routine = None;
                self.event_ui.row = None;
            }
            Err(error) => self.error = Some(error),
        }
    }
}

/// Under the commands of the "Assets" menu: the menu files to open, and "New menu"
/// with its name field.
pub(crate) fn menu_menu(ui: &mut Ui, editor: &mut Editor, name: &mut String) {
    ui.add_enabled_ui(!editor.playing(), |ui| {
        let directory = editor.document.directory();
        ui.menu_button("Open menu", |ui| {
            let files = menu_files(&directory);
            if files.is_empty() {
                ui.weak("No menu file at the room file. \"New menu\" writes one.");
            }
            for path in files {
                let shown = path.strip_prefix(&directory).unwrap_or(&path);
                if ui.button(shown.to_string_lossy()).clicked() {
                    editor.open_menu(&path);
                    ui.close();
                }
            }
        });
        ui.horizontal(|ui| {
            let field = egui::TextEdit::singleline(name)
                .desired_width(120.0)
                .hint_text("name of the menu");
            ui.add(field);
            let button = ui
                .add_enabled(!name.trim().is_empty(), egui::Button::new("New menu"))
                .on_hover_text(
                    "Writes data/menus/<name>.menu.ron and opens it in the event panel. \
                     A command OpenMenu shows it.",
                );
            if button.clicked() {
                match new_menu(&directory, name) {
                    Ok(path) => {
                        editor.open_menu(&path);
                        editor.status = format!("{} written.", path.display());
                        name.clear();
                    }
                    Err(error) => editor.error = Some(error),
                }
                ui.close();
            }
        });
    });
}

/// The event panel with a menu file open. True: the panel showed it.
pub(crate) fn menu_panel(ui: &mut Ui, world: &mut World, editor: &mut Editor) -> bool {
    if editor.menu.is_none() {
        return false;
    }
    let registry = editor.registry.clone();
    // No object: the lists of the room, and no problem of an event.
    let (mut lists, _) = lists_and_problems(editor, None);
    let directory = editor.document.directory();
    let (room, cache) = (&editor.document.room, &mut editor.event_ui.live);
    cached_live_lists(world, room, &directory, cache, &mut lists);
    let editors = world
        .get_resource::<ArgumentEditors>()
        .cloned()
        .unwrap_or_default();
    crate::widgets::set_room_directory(ui, &directory, &editor.converters);
    let Editor {
        menu,
        event_ui,
        document,
        error,
        ..
    } = editor;
    let edit = menu.as_mut().expect("tested above");
    let room = &document.room;
    let (mut changed, mut close) = (false, false);
    let frame = egui::Frame::NONE.inner_margin(6);
    let scroll = |salt: &'static str| egui::ScrollArea::both().id_salt(salt).auto_shrink(false);
    let before = edit.row;
    let mut select = before;
    egui::Panel::left("menu fields")
        .resizable(true)
        .frame(frame)
        .show(ui, |ui| {
            scroll("menu fields").show(ui, |ui| {
                ui.horizontal(|ui| {
                    let file = edit.path.strip_prefix(&directory).unwrap_or(&edit.path);
                    ui.strong(file.to_string_lossy());
                    close = ui.button("Close").clicked();
                });
                changed |= menu_fields(ui, &mut edit.menu, &lists);
                ui.separator();
                ui.strong("Rows");
                changed |= row_list(ui, &mut edit.menu.rows, &mut select);
            });
        });
    if select != before {
        edit.row = select;
        event_ui.row = None;
    }
    egui::Panel::left("menu row")
        .resizable(true)
        .frame(frame)
        .show(ui, |ui| {
            scroll("menu row").show(ui, |ui| {
                let row = edit.menu.rows.get_mut(edit.row);
                changed |= row_editor(ui, row, event_ui, (&lists, &registry, room));
            });
        });
    egui::Frame::NONE.inner_margin(6).show(ui, |ui| {
        scroll("menu row arguments").show(ui, |ui| {
            if let Some(list) = edit.menu.rows.get_mut(edit.row).and_then(commands_of) {
                let places = (&lists, &registry, &editors, room);
                changed |= command_arguments(ui, list, event_ui, places);
            }
        });
    });
    if changed && let Err(text) = edit.save() {
        *error = Some(text);
    }
    if close {
        *menu = None;
    }
    true
}

/// Title, place, focus, and back, from the field table of the menu. The rows are
/// not fields: they stay as they are.
fn menu_fields(ui: &mut Ui, menu: &mut Menu, lists: &Lists) -> bool {
    let mut tree = menu.tree();
    let shown = with_picks(lists, |picks| {
        crate::fields::show(ui, &Menu::kind(), &mut tree, picks)
    });
    if !shown {
        return false;
    }
    match maki_core::fields::read::<Menu>(tree) {
        Ok(new) => {
            *menu = Menu {
                rows: std::mem::take(&mut menu.rows),
                ..new
            };
            true
        }
        Err(_) => false,
    }
}

// `_` arms: the kinds of row that #72 adds have no editor here yet.
#[allow(unreachable_patterns)]
fn commands_of(row: &mut MenuRow) -> Option<&mut Vec<Command>> {
    match row {
        MenuRow::Commands { commands, .. } => Some(commands),
        _ => None,
    }
}

/// The list of the rows: pick one, add, remove, move. True: the list changed.
fn row_list(ui: &mut Ui, rows: &mut Vec<MenuRow>, select: &mut usize) -> bool {
    let mut changed = false;
    for (index, row) in rows.iter().enumerate() {
        if ui
            .selectable_label(*select == index, row_label(row))
            .clicked()
        {
            *select = index;
        }
    }
    ui.horizontal(|ui| {
        if ui.button("Add").clicked() {
            rows.push(MenuRow::Commands {
                text: TextKey::default(),
                conditions: Vec::new(),
                commands: Vec::new(),
            });
            *select = rows.len() - 1;
            changed = true;
        }
        let at = *select;
        if ui
            .add_enabled(at < rows.len(), egui::Button::new("Remove"))
            .clicked()
        {
            rows.remove(at);
            *select = at.saturating_sub(1);
            changed = true;
        }
        if ui
            .add_enabled(0 < at && at < rows.len(), egui::Button::new("Up"))
            .clicked()
        {
            rows.swap(at, at - 1);
            *select = at - 1;
            changed = true;
        }
        if ui
            .add_enabled(at + 1 < rows.len(), egui::Button::new("Down"))
            .clicked()
        {
            rows.swap(at, at + 1);
            *select = at + 1;
            changed = true;
        }
    });
    changed
}

#[allow(unreachable_patterns)]
fn row_label(row: &MenuRow) -> String {
    match row {
        MenuRow::Commands { text, .. } if text.0.is_empty() => "(no text key)".to_owned(),
        MenuRow::Commands { text, .. } => text.0.clone(),
        _ => "(row with no editor)".to_owned(),
    }
}

/// The text key, the conditions, and the command rows of a row. True: a change.
#[allow(unreachable_patterns)]
fn row_editor(
    ui: &mut Ui,
    row: Option<&mut MenuRow>,
    event_ui: &mut EventUi,
    (lists, registry, room): (&Lists, &CommandRegistry, &RoomDef),
) -> bool {
    let mut changed = false;
    match row {
        Some(MenuRow::Commands {
            text,
            conditions: held,
            commands,
        }) => {
            ui.label("Text");
            changed |= pick_key(ui, "menu row text", text, lists);
            ui.label("The row shows while all conditions hold:");
            changed |= conditions(ui, held, lists);
            ui.separator();
            ui.label("Commands of a pick");
            changed |= command_rows(ui, commands, event_ui, (lists, registry, room), &[], 0);
        }
        Some(_) => {
            ui.weak("This kind of row has no editor yet.");
        }
        None => {
            ui.weak("No row. \"Add\" makes one.");
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use bevy_ecs::schedule::IntoScheduleConfigs;

    use super::*;

    /// The texts that the event panel showed, with a menu open.
    #[derive(bevy_ecs::resource::Resource, Default)]
    struct Shown(Vec<String>);

    /// Draws the panel in a headless egui frame when a menu is open.
    fn draw(world: &mut World) {
        if world.resource::<Editor>().menu.is_none() {
            return;
        }
        let mut output = egui::Context::default().run_ui(egui::RawInput::default(), |ui| {
            world.resource_scope(|world, mut editor: bevy_ecs::world::Mut<Editor>| {
                assert!(menu_panel(ui, world, &mut editor));
            });
        });
        output.textures_delta.clear();
        let texts = output.shapes.iter().filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        });
        world.resource_mut::<Shown>().0 = texts.collect();
    }

    #[test]
    fn a_new_menu_is_a_file_that_opens_and_shows_its_rows() {
        let (mut app, directory) = crate::tests::editor_app("menu-panel");
        app.init_resource::<Shown>()
            .add_systems(maki_core::Update, draw.ambiguous_with_all());
        app.run(|mut app| {
            crate::tests::frames(&mut app, 3);
            let path = new_menu(&directory, "pause").unwrap();
            assert_eq!(path, directory.join("data/menus/pause.menu.ron"));
            assert!(new_menu(&directory, "pause").is_err());
            assert!(new_menu(&directory, "../out").is_err());
            assert_eq!(menu_files(&directory), std::slice::from_ref(&path));
            // Empty menu: the file is what `menu_text` writes.
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                menu_text(&Menu::default())
            );

            let text = r#"(title: Some("menu.pause"), rows: [
                Commands(text: "menu.back", commands: [CloseMenu]),
                Commands(text: "menu.key", conditions: [Flag("has_key")], commands: [Message("key")]),
            ])"#;
            std::fs::write(&path, text).unwrap();
            app.resource_mut::<Editor>().open_menu(&path);
            crate::tests::frames(&mut app, 1);
            let texts = &app.world().resource::<Shown>().0;
            for wanted in ["menu.back", "menu.key"] {
                assert!(texts.iter().any(|text| text == wanted), "{texts:?}");
            }
            // The first row is selected: its commands show as rows.
            assert!(texts.iter().any(|t| t.contains("Close")), "{texts:?}");
            // Nothing changed: the file is as it was.
            assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        });
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_change_of_a_field_or_a_row_writes_the_file() {
        let (app, directory) = crate::tests::editor_app("menu-panel-save");
        let registry = app.world().resource::<CommandRegistry>().clone();
        let path = new_menu(&directory, "m").unwrap();
        let mut edit = MenuEdit::open(&path, &registry).unwrap();
        assert_eq!(edit.menu, Menu::default());

        // The edits that the widgets make, then the save that the panel does.
        edit.menu.rows.push(MenuRow::Commands {
            text: "menu.a".into(),
            conditions: vec![maki_events::Condition::Flag("f".to_owned())],
            commands: vec![registry.parse_command("CloseMenu").unwrap()],
        });
        edit.menu.focus = false;
        edit.menu.title = Some("menu.t".into());
        edit.save().unwrap();
        assert_eq!(MenuEdit::open(&path, &registry).unwrap().menu, edit.menu);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
