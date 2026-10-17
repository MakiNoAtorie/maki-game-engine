//! The panel "Texts": a row for each key of the text tables, a column for each
//! language. A change of a cell is written to the table file at once, with no undo, as
//! a data file in the inspector is.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use bevy_ecs::world::Mut;
use maki_assets::{DATA_FOLDER, data_text, read_data};
use maki_events::TEXT_FOLDER;
use maki_events::{TextEntry, TextTable};
use maki_ui::egui::{self, Ui};
use maki_ui::theme::icon;
use maki_ui::{THEME, ToolPanel, ToolPlace, add_tool_panel};

use crate::Editor;

/// A text table file. `data` is `None` when the file does not read: the time stays,
/// so the file is read again only after the next change.
#[derive(Debug)]
struct Table {
    path: PathBuf,
    time: Option<SystemTime>,
    data: Option<TextTable>,
}

fn time_of(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|data| data.modified())
        .ok()
}

impl Table {
    fn save(&mut self) -> Result<(), String> {
        let data = self.data.as_ref().ok_or("The table is not read.")?;
        std::fs::write(&self.path, data_text(data))
            .map_err(|error| format!("{} not written: {error}", self.path.display()))?;
        self.time = time_of(&self.path);
        Ok(())
    }
}

/// The rows to draw: the languages, and for each key the text of each language.
#[derive(Debug, PartialEq)]
struct Grid {
    languages: Vec<String>,
    /// The key, and the text of each language: `None` where the language lacks the key.
    rows: Vec<(String, Vec<Option<String>>)>,
}

/// Keys in the order of the first table that has them, languages in the order of the
/// tables. Only the rows whose key or a text has `search` (no case) stay.
fn grid(tables: &[Table], search: &str) -> Grid {
    let tables: Vec<&TextTable> = tables.iter().filter_map(|t| t.data.as_ref()).collect();
    let mut languages: Vec<String> = Vec::new();
    for table in &tables {
        if !languages.contains(&table.language) {
            languages.push(table.language.clone());
        }
    }
    let mut seen = HashSet::new();
    let search = search.trim().to_lowercase();
    let found = |text: &str| text.to_lowercase().contains(&search);
    let mut rows = Vec::new();
    for entry in tables.iter().flat_map(|table| &table.texts) {
        if !seen.insert(&entry.key) {
            continue;
        }
        let cells: Vec<Option<String>> = languages
            .iter()
            .map(|language| text_of(&tables, language, &entry.key).map(str::to_owned))
            .collect();
        if found(&entry.key) || cells.iter().flatten().any(|text| found(text)) {
            rows.push((entry.key.clone(), cells));
        }
    }
    Grid { languages, rows }
}

fn text_of<'a>(tables: &[&'a TextTable], language: &str, key: &str) -> Option<&'a str> {
    let in_language = tables.iter().filter(|table| table.language == language);
    let mut entries = in_language.flat_map(|table| &table.texts);
    entries
        .find(|entry| entry.key == key)
        .map(|entry| entry.text.as_str())
}

/// Sets the text of a key in a language: in the table that has the key, else in the
/// first table of the language. The file is written.
fn set_cell(tables: &mut [Table], key: &str, language: &str, text: &str) -> Result<(), String> {
    let of_language = |table: &Table| table.data.as_ref().is_some_and(|d| d.language == language);
    let has_key = |table: &Table| {
        let data = table.data.as_ref();
        data.is_some_and(|d| d.texts.iter().any(|entry| entry.key == key))
    };
    let target = tables.iter().position(|t| of_language(t) && has_key(t));
    let target = target.or_else(|| tables.iter().position(of_language));
    let table = &mut tables[target.ok_or_else(|| format!("No table of \"{language}\"."))?];
    let texts = &mut table.data.as_mut().expect("a table of a language").texts;
    match texts.iter_mut().find(|entry| entry.key == key) {
        Some(entry) => entry.text = text.to_owned(),
        None => texts.push(TextEntry {
            key: key.to_owned(),
            text: text.to_owned(),
        }),
    }
    table.save()
}

/// Adds the key with an empty text to the first table of each language.
fn add_key(tables: &mut [Table], key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("A key has a name.".to_owned());
    }
    let data = tables.iter().filter_map(|t| t.data.as_ref());
    if data.flat_map(|t| &t.texts).any(|entry| entry.key == key) {
        return Err(format!("The key \"{key}\" is there already."));
    }
    let mut done = HashSet::new();
    for table in tables.iter_mut() {
        let Some(data) = table.data.as_mut() else {
            continue;
        };
        if done.insert(data.language.clone()) {
            data.texts.push(TextEntry {
                key: key.to_owned(),
                text: String::new(),
            });
            table.save()?;
        }
    }
    Ok(())
}

/// Writes `data/text/<code>.text.ron` with the language and no text: its path.
fn add_language(directory: &Path, tables: &[Table], code: &str) -> Result<PathBuf, String> {
    let code = code.trim();
    let letters = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    if code.is_empty() || !code.chars().all(letters) {
        return Err("A language code has letters, numbers, `_`, and `-`.".to_owned());
    }
    let mut data = tables.iter().filter_map(|t| t.data.as_ref());
    let path = maki_events::table_path(directory, code);
    if path.exists() || data.any(|t| t.language == code) {
        return Err(format!("The language \"{code}\" is there already."));
    }
    let table = TextTable::new(code);
    std::fs::create_dir_all(path.parent().unwrap_or(directory))
        .and_then(|()| std::fs::write(&path, data_text(&table)))
        .map_err(|error| format!("{} not written: {error}", path.display()))?;
    Ok(path)
}

/// The state of the panel: the tables as read, and the text of its fields.
#[derive(Default)]
#[cfg_attr(test, derive(bevy_ecs::resource::Resource))]
pub(crate) struct TextPanel {
    directory: PathBuf,
    /// The times of the data folder and of the text folder: a new table file changes one.
    folder_time: [Option<SystemTime>; 2],
    tables: Vec<Table>,
    search: String,
    new_key: String,
    new_language: String,
}

impl TextPanel {
    /// Reads the table files again when the directory, the folder, or a file changed.
    fn refresh(&mut self, directory: &Path, error: &mut Option<String>) {
        let data_folder = directory.join(DATA_FOLDER);
        let folder_time = [
            time_of(&data_folder),
            time_of(&data_folder.join(TEXT_FOLDER)),
        ];
        let same = |table: &Table| time_of(&table.path) == table.time;
        let unchanged = self.directory == directory && self.folder_time == folder_time;
        if unchanged && self.tables.iter().all(same) {
            return;
        }
        self.directory = directory.to_owned();
        self.folder_time = folder_time;
        self.tables.clear();
        for path in maki_events::table_files(directory) {
            let path = directory.join(path);
            let time = time_of(&path);
            let data = read_data::<TextTable>(&path);
            let data = data.map_err(|message| *error = Some(message)).ok();
            self.tables.push(Table { path, time, data });
        }
    }
}

pub(crate) fn add(app: &mut maki_core::App) {
    let panel = ToolPanel {
        id: "texts",
        title: "Texts",
        icon: icon::PANEL,
        place: ToolPlace::Bottom,
        open: true,
    };
    let mut state = TextPanel::default();
    add_tool_panel(app, panel, move |ui, world| {
        world.resource_scope(|_, mut editor: Mut<Editor>| text_panel(ui, &mut editor, &mut state));
    });
}

fn text_panel(ui: &mut Ui, editor: &mut Editor, state: &mut TextPanel) {
    if editor.playing() {
        ui.weak("A game runs. Stop it to edit the texts.");
        return;
    }
    let directory = editor.document.directory();
    state.refresh(&directory, &mut editor.error);

    ui.horizontal(|ui| {
        ui.label(icon::SEARCH);
        ui.add(egui::TextEdit::singleline(&mut state.search).hint_text("search"));
        ui.separator();
        let field = egui::TextEdit::singleline(&mut state.new_key)
            .desired_width(120.0)
            .hint_text("key");
        ui.add(field);
        if ui.button("New key").clicked() {
            match add_key(&mut state.tables, &state.new_key) {
                Ok(()) => state.new_key.clear(),
                Err(error) => editor.error = Some(error),
            }
        }
        ui.separator();
        let field = egui::TextEdit::singleline(&mut state.new_language)
            .desired_width(60.0)
            .hint_text("code");
        ui.add(field);
        if ui.button("New language").clicked() {
            match add_language(&directory, &state.tables, &state.new_language) {
                Ok(path) => {
                    editor.status = format!("{} written.", path.display());
                    state.new_language.clear();
                }
                Err(error) => editor.error = Some(error),
            }
        }
    });
    ui.separator();

    let Grid { languages, rows } = grid(&state.tables, &state.search);
    if languages.is_empty() {
        ui.weak("No text table. \"New language\" writes one.");
        return;
    }
    // At most one cell changes in a frame.
    let mut change = None;
    egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
        // A grid gives a column the width of its first frame: a cell takes its width here.
        let grid = egui::Grid::new("texts").striped(true).min_col_width(240.0);
        grid.show(ui, |ui| {
            ui.strong("key");
            for language in &languages {
                ui.strong(language);
            }
            ui.end_row();
            for (key, cells) in rows {
                ui.label(&key);
                for (language, cell) in languages.iter().zip(cells) {
                    let missing = cell.is_none();
                    let mut text = cell.unwrap_or_default();
                    let mut field = egui::TextEdit::multiline(&mut text)
                        .desired_rows(1)
                        .desired_width(240.0);
                    if missing {
                        field = field.hint_text(egui::RichText::new("missing").color(THEME.error));
                    }
                    if ui.add(field).changed() {
                        change = Some((key.clone(), language.clone(), text));
                    }
                }
                ui.end_row();
            }
        });
    });
    if let Some((key, language, text)) = change
        && let Err(error) = set_cell(&mut state.tables, &key, &language, &text)
    {
        editor.error = Some(error);
    }
}

#[cfg(test)]
mod tests {
    use bevy_ecs::resource::Resource;

    use super::*;

    fn table(language: &str, texts: &[(&str, &str)]) -> TextTable {
        let texts = texts.iter().map(|(key, text)| TextEntry {
            key: key.to_string(),
            text: text.to_string(),
        });
        TextTable {
            language: language.to_owned(),
            texts: texts.collect(),
        }
    }

    /// A room directory with `en` (a, b) and `ja` (b only).
    fn directory(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("maki-texts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(directory.join("data/text")).unwrap();
        let en = table("en", &[("a", "Door"), ("b", "Key")]);
        std::fs::write(directory.join("data/text/en.text.ron"), data_text(&en)).unwrap();
        let ja = table("ja", &[("b", "Kagi")]);
        std::fs::write(directory.join("data/text/ja.text.ron"), data_text(&ja)).unwrap();
        directory
    }

    fn loaded(directory: &Path) -> TextPanel {
        let mut state = TextPanel::default();
        state.refresh(directory, &mut None);
        state
    }

    fn read(directory: &Path, language: &str) -> TextTable {
        read_data(&directory.join(format!("data/text/{language}.text.ron"))).unwrap()
    }

    #[test]
    fn the_grid_has_a_cell_for_each_language_and_none_for_a_missing_one() {
        let directory = directory("grid");
        let state = loaded(&directory);
        let grid = grid(&state.tables, "");
        assert_eq!(grid.languages, ["en", "ja"]);
        let some = |text: &str| Some(text.to_owned());
        assert_eq!(grid.rows[0], ("a".to_owned(), vec![some("Door"), None]));
        assert_eq!(
            grid.rows[1],
            ("b".to_owned(), vec![some("Key"), some("Kagi")])
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn the_search_keeps_the_rows_with_the_text_in_a_key_or_a_text() {
        let directory = directory("search");
        let state = loaded(&directory);
        let keys = |search: &str| -> Vec<String> {
            grid(&state.tables, search)
                .rows
                .into_iter()
                .map(|r| r.0)
                .collect()
        };
        assert_eq!(keys("DOOR"), ["a"]);
        assert_eq!(keys("kag"), ["b"]);
        assert_eq!(keys("b"), ["b"]);
        assert_eq!(keys(" "), ["a", "b"]);
        assert!(keys("zzz").is_empty());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_cell_change_writes_the_table_of_its_language() {
        let directory = directory("cell");
        let mut state = loaded(&directory);
        set_cell(&mut state.tables, "b", "ja", "Kagi!").unwrap();
        // A missing cell goes to the first table of the language.
        set_cell(&mut state.tables, "a", "ja", "Tobira").unwrap();
        assert_eq!(
            read(&directory, "ja"),
            table("ja", &[("b", "Kagi!"), ("a", "Tobira")])
        );
        assert_eq!(
            read(&directory, "en"),
            table("en", &[("a", "Door"), ("b", "Key")])
        );
        assert!(set_cell(&mut state.tables, "a", "fr", "x").is_err());
        // The write is not a change by another program: no new read.
        assert!(state.tables.iter().all(|t| time_of(&t.path) == t.time));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn new_key_and_new_language_write_their_files() {
        let directory = directory("new");
        let mut state = loaded(&directory);
        add_key(&mut state.tables, "c").unwrap();
        assert_eq!(
            read(&directory, "en").texts[2],
            TextEntry {
                key: "c".into(),
                text: String::new()
            }
        );
        assert_eq!(read(&directory, "ja").texts[1].key, "c");
        assert!(add_key(&mut state.tables, "a").is_err());
        assert!(add_key(&mut state.tables, " ").is_err());

        let path = add_language(&directory, &state.tables, "fr").unwrap();
        assert_eq!(path, directory.join("data/text/fr.text.ron"));
        assert_eq!(read(&directory, "fr"), table("fr", &[]));
        assert!(add_language(&directory, &state.tables, "fr").is_err());
        assert!(add_language(&directory, &state.tables, "en").is_err());
        assert!(add_language(&directory, &state.tables, "../x").is_err());

        // The new file shows at the next draw, and the key goes to it too.
        state.refresh(&directory, &mut None);
        assert_eq!(grid(&state.tables, "").languages, ["en", "fr", "ja"]);
        std::fs::remove_dir_all(directory).unwrap();
    }

    /// The texts that the panel drew.
    #[derive(Resource, Default)]
    struct Shown(Vec<String>);

    fn draw(world: &mut bevy_ecs::world::World) {
        let mut output = egui::Context::default().run_ui(egui::RawInput::default(), |ui| {
            world.resource_scope(|world, mut editor: Mut<Editor>| {
                world.resource_scope(|_, mut state: Mut<TextPanel>| {
                    text_panel(ui, &mut editor, &mut state)
                });
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
    fn the_panel_draws_the_keys_the_languages_and_the_missing_mark() {
        use bevy_ecs::schedule::IntoScheduleConfigs;
        let (mut app, room) = crate::tests::editor_app("texts-draw");
        let directory = directory("draw");
        // The room directory of the editor app takes the tables.
        std::fs::create_dir_all(room.join("data/text")).unwrap();
        for language in ["en", "ja"] {
            let from = directory.join(format!("data/text/{language}.text.ron"));
            std::fs::copy(from, room.join(format!("data/text/{language}.text.ron"))).unwrap();
        }
        app.init_resource::<Shown>().init_resource::<TextPanel>();
        app.add_systems(maki_core::Update, draw.ambiguous_with_all());
        app.run(|mut app| {
            crate::tests::frames(&mut app, 3);
            let texts = &app.world().resource::<Shown>().0;
            for expected in ["en", "ja", "a", "b", "Door", "Kagi", "missing"] {
                assert!(texts.iter().any(|t| t == expected), "{expected}: {texts:?}");
            }
            assert_eq!(texts.iter().filter(|t| *t == "missing").count(), 1);
        });
        std::fs::remove_dir_all(directory).unwrap();
        std::fs::remove_dir_all(room).unwrap();
    }
}
