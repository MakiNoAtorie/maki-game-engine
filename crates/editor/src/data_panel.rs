//! A data file in the inspector: the fields of its type, drawn from the table of the
//! type. A change is written to the file at once, with no undo, as a routine is.

use std::path::{Path, PathBuf};

use bevy_ecs::world::World;
use maki_assets::{DATA_FOLDER, DataTypes};
use maki_core::fields::Value;
use maki_ui::egui::{self, Ui};

use crate::Editor;
use crate::fields::Lists;

/// A data file that the inspector shows in place of the selection.
#[derive(Debug, Clone, PartialEq)]
pub struct DataEdit {
    pub path: PathBuf,
    pub type_name: String,
    pub tree: Value,
    /// The time of the file at the last read or write: a newer file is read again.
    pub time: Option<std::time::SystemTime>,
}

fn time_of(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path)
        .and_then(|data| data.modified())
        .ok()
}

impl DataEdit {
    /// Reads the file as the type that its name says.
    pub fn open(path: &Path, types: &DataTypes) -> Result<Self, String> {
        let type_name = DataTypes::type_of(path).unwrap_or_default();
        let data_type = types
            .get(type_name)
            .ok_or_else(|| format!("No module registered the data type \"{type_name}\"."))?;
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("{} not read: {error}", path.display()))?;
        let tree = (data_type.tree_of_text)(&text)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        Ok(Self {
            path: path.to_owned(),
            type_name: type_name.to_owned(),
            tree,
            time: time_of(path),
        })
    }

    pub fn save(&mut self, types: &DataTypes) -> Result<(), String> {
        let data_type = types.get(&self.type_name).ok_or("The data type is gone.")?;
        let text = (data_type.text)(&self.tree)?;
        std::fs::write(&self.path, text)
            .map_err(|error| format!("{} not written: {error}", self.path.display()))?;
        self.time = time_of(&self.path);
        Ok(())
    }
}

/// Writes the default file `data/<name>.<type>.ron` in `directory`: its path. `Err`:
/// why no file was written. The scan of the editor gives the file its id.
pub fn new_data_asset(
    directory: &Path,
    types: &DataTypes,
    type_name: &str,
    name: &str,
) -> Result<PathBuf, String> {
    let name = name.trim();
    let letters = |c: char| c.is_alphanumeric() || c == '_' || c == '-' || c == ' ';
    if name.is_empty() || !name.chars().all(letters) {
        return Err("A name of a data asset has letters, numbers, `_`, and `-`.".to_owned());
    }
    let data_type = types
        .get(type_name)
        .ok_or_else(|| format!("No data type \"{type_name}\"."))?;
    let path = directory
        .join(DATA_FOLDER)
        .join(format!("{name}.{}", DataTypes::extension(type_name)));
    if path.exists() {
        return Err(format!("{} is there already.", path.display()));
    }
    let written = std::fs::create_dir_all(directory.join(DATA_FOLDER))
        .and_then(|()| std::fs::write(&path, (data_type.default_text)()));
    written.map_err(|error| format!("{} not written: {error}", path.display()))?;
    Ok(path)
}

impl Editor {
    /// Opens a data file in the inspector.
    pub fn open_data(&mut self, path: &Path, types: &DataTypes) {
        match DataEdit::open(path, types) {
            Ok(edit) => self.data = Some(edit),
            Err(error) => self.error = Some(error),
        }
    }
}

/// The type and the name that "New data asset" has in its fields.
#[derive(Default)]
pub(crate) struct NewData {
    pub type_name: String,
    pub name: String,
}

/// Under the commands of the "Assets" menu: "Open data asset", and "New data asset"
/// with its type pick and its name field.
pub(crate) fn data_menu(ui: &mut Ui, world: &World, editor: &mut Editor, new: &mut NewData) {
    let Some(types) = world.get_resource::<DataTypes>() else {
        return;
    };
    ui.add_enabled_ui(!editor.playing(), |ui| {
        let directory = editor.document.directory();
        ui.menu_button("Open data asset", |ui| {
            let files = types.files(&directory);
            if files.is_empty() {
                ui.weak("No data file. \"New data asset\" writes one.");
            }
            for file in files {
                let text = file.path.to_string_lossy();
                let button = ui.add_enabled(file.known, egui::Button::new(text));
                let button = button.on_disabled_hover_text("No module registered this type.");
                if button.clicked() {
                    editor.open_data(&directory.join(&file.path), types);
                    ui.close();
                }
            }
        });
        if types.get(&new.type_name).is_none() {
            let first = types.iter().next();
            new.type_name = first.map(|t| t.name.to_owned()).unwrap_or_default();
        }
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("new data type")
                .selected_text(&new.type_name)
                .show_ui(ui, |ui| {
                    for data_type in types.iter() {
                        let name = data_type.name.to_owned();
                        ui.selectable_value(&mut new.type_name, name, data_type.name);
                    }
                });
            let field = egui::TextEdit::singleline(&mut new.name)
                .desired_width(120.0)
                .hint_text("name of the data asset");
            ui.add(field);
        });
        let ready = !new.name.trim().is_empty() && !new.type_name.is_empty();
        let button = ui
            .add_enabled(ready, egui::Button::new("New data asset"))
            .on_hover_text("Writes data/<name>.<type>.ron with the default value and opens it.");
        if button.clicked() {
            match new_data_asset(&directory, types, &new.type_name, &new.name) {
                Ok(path) => {
                    editor.open_data(&path, types);
                    editor.status = format!("{} written.", path.display());
                    new.name.clear();
                }
                Err(error) => editor.error = Some(error),
            }
            ui.close();
        }
    });
}

/// The inspector with a data file open. True: it showed the file.
pub(crate) fn data_inspector(ui: &mut Ui, types: &DataTypes, editor: &mut Editor) -> bool {
    let playing = editor.playing();
    let Some(edit) = editor.data.as_mut() else {
        return false;
    };
    // Another program wrote the file: show its values, so a change does not undo them.
    if time_of(&edit.path) != edit.time {
        match DataEdit::open(&edit.path, types) {
            Ok(new) => *edit = new,
            Err(error) => editor.error = Some(error),
        }
    }
    let Some(edit) = editor.data.as_mut() else {
        return false;
    };
    let mut close = false;
    ui.horizontal(|ui| {
        ui.strong(edit.path.file_name().unwrap_or_default().to_string_lossy());
        close = ui.button("Close").clicked();
    });
    let mut changed = false;
    if let Some(data_type) = types.get(&edit.type_name) {
        let tree = &mut edit.tree;
        ui.add_enabled_ui(!playing, |ui| {
            changed = crate::fields::show(ui, &data_type.kind, tree, Lists::objects(&[]));
        });
    }
    let saved = if changed { edit.save(types) } else { Ok(()) };
    if let Err(error) = saved {
        editor.error = Some(error);
    }
    if close {
        editor.data = None;
    }
    true
}

#[cfg(test)]
mod tests {
    use maki_assets::{DataAsset, DataRef, FileId, FileRef, add_data_type};
    use maki_core::Fields;
    use maki_core::fields::Kind;
    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
    #[serde(default, deny_unknown_fields)]
    #[fields(default)]
    struct Lamp {
        name: String,
        #[field(range = 0.0..=10.0)]
        power: f32,
    }

    impl Default for Lamp {
        fn default() -> Self {
            Self {
                name: "lamp".into(),
                power: 2.0,
            }
        }
    }

    impl DataAsset for Lamp {
        const NAME: &'static str = "lamp";
    }

    /// A type that holds a reference to a lamp.
    #[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Fields)]
    #[serde(default)]
    #[fields(default)]
    struct Desk {
        lamp: DataRef<Lamp>,
    }

    impl DataAsset for Desk {
        const NAME: &'static str = "desk";
    }

    /// An app that has the two types, for its `DataTypes`.
    fn types_app() -> maki_core::App {
        let mut app = maki_core::App::new(maki_core::ConfigFile::default()).unwrap();
        app.add_module(maki_assets::AssetsModule);
        add_data_type::<Lamp>(&mut app);
        add_data_type::<Desk>(&mut app);
        app
    }

    #[test]
    fn a_new_data_asset_is_the_default_file_and_a_change_writes_it() {
        let types_app = types_app();
        let types = types_app.world().resource::<DataTypes>();
        let (app, directory) = crate::tests::editor_app("data-new");
        app.run(|mut app| {
            crate::tests::frames(&mut app, 3);
            let path = new_data_asset(&directory, types, "lamp", "hall").unwrap();
            assert_eq!(path, directory.join("data/hall.lamp.ron"));
            let default = (types.get("lamp").unwrap().default_text)();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), default);
            assert!(new_data_asset(&directory, types, "lamp", "hall").is_err());
            assert!(new_data_asset(&directory, types, "lamp", "../out").is_err());
            assert!(new_data_asset(&directory, types, "enemy", "orc").is_err());

            // The editor opens it; a change of the tree writes the file.
            let mut editor = app.resource_mut::<Editor>();
            editor.open_data(&path, types);
            let edit = editor.data.as_mut().unwrap();
            assert_eq!(edit.tree, Lamp::default().tree());
            edit.tree["power"] = 7.0.into();
            edit.save(types).unwrap();
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(text.contains("power: 7.0"), "{text}");

            // A file of a type that no module registered does not open.
            let other = directory.join("data/orc.enemy.ron");
            std::fs::write(&other, "()").unwrap();
            editor.data = None;
            editor.open_data(&other, types);
            assert!(editor.data.is_none() && editor.error.is_some());
        });
        std::fs::remove_dir_all(directory).unwrap();
    }

    /// The inspector draws the fields of the open file, and "Close" is there.
    #[test]
    fn the_inspector_draws_the_open_file() {
        let types_app = types_app();
        let types = types_app.world().resource::<DataTypes>();
        let (app, directory) = crate::tests::editor_app("data-draw");
        app.run(|mut app| {
            crate::tests::frames(&mut app, 3);
            let path = new_data_asset(&directory, types, "lamp", "hall").unwrap();
            let mut editor = app.resource_mut::<Editor>();
            editor.open_data(&path, types);
            let mut output = egui::Context::default().run_ui(egui::RawInput::default(), |ui| {
                assert!(data_inspector(ui, types, &mut editor));
            });
            output.textures_delta.clear();
            let texts: Vec<String> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Text(text) => Some(text.galley.text().to_owned()),
                    _ => None,
                })
                .collect();
            assert!(texts.iter().any(|t| t == "Close"), "{texts:?}");
            assert!(texts.iter().any(|t| t == "hall.lamp.ron"), "{texts:?}");
        });
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_data_ref_field_lists_the_files_of_its_type_only() {
        let types_app = types_app();
        let types = types_app.world().resource::<DataTypes>();
        let directory = std::env::temp_dir().join(format!("maki-data-pick-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        new_data_asset(&directory, types, "lamp", "hall").unwrap();
        new_data_asset(&directory, types, "desk", "oak").unwrap();
        let Kind::Group(rows) = Desk::kind() else {
            panic!("a group")
        };
        let Kind::File(group) = &rows[0].kind else {
            panic!("{:?}", rows[0].kind)
        };
        let extension = DataTypes::extension(group);
        let found = crate::widgets::files(&directory, &[extension.as_str()], &Default::default());
        assert_eq!(found, [PathBuf::from("data/hall.lamp.ron")]);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_new_data_file_gets_an_id_and_a_moved_file_resolves_by_it() {
        let directory = std::env::temp_dir().join(format!("maki-data-id-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let types_app = types_app();
        let types = types_app.world().resource::<DataTypes>();
        let path = new_data_asset(&directory, types, "lamp", "hall").unwrap();
        // Two looks of the scan thread: the id file comes at the second.
        let tools = crate::sources::Tools::default();
        let converters = crate::sources::Converters::default();
        let mut known = Vec::new();
        for _ in 0..2 {
            known = crate::sources::scan(
                &directory,
                &known,
                &converters,
                &tools,
                &mut Default::default(),
                &mut |_| {},
            );
        }
        let id = FileId::read(&path).expect("the scan wrote an id file");
        let reference = FileRef {
            path: PathBuf::from("data/hall.lamp.ron"),
            id: Some(id),
        };
        // The file moves with its id file.
        std::fs::create_dir_all(directory.join("data/old")).unwrap();
        let moved = directory.join("data/old/hall.lamp.ron");
        std::fs::rename(&path, &moved).unwrap();
        std::fs::rename(FileId::file_of(&path), FileId::file_of(&moved)).unwrap();
        assert_eq!(reference.resolve(&directory), moved);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
