//! The rows of the inspector that need the room file: a file of its directory, a
//! name of one of its lists. They are rows of the one property grid of the tool
//! (`maki_ui::widgets`). Each returns true in a frame where the user changed the
//! value. `crate::fields` makes the rows of an item from the table of its fields.
//!
//! The public ones are for the argument widgets of the commands of a module
//! (`add_argument_editor`).

use std::path::{Path, PathBuf};

use maki_scene::{ObjectDef, ObjectId};
use maki_ui::egui::{self, Ui};

use crate::sources::Converters;
pub(crate) use maki_ui::widgets::{
    choice, color, drag, number as number_or, optional, row, text, toggle, vector,
};

/// A number with a label and its unit. Drag it, or click it and type.
pub fn number(
    ui: &mut Ui,
    text: &str,
    value: &mut f32,
    speed: f32,
    range: std::ops::RangeInclusive<f32>,
) -> bool {
    number_or(ui, text, value, None, speed, range)
}

/// The files with one of these extensions in a directory and one level below it,
/// as a room file names them: from that directory, with `/`. Also the result file
/// that a source file of another program will give (`crate::sources`), while it is
/// not there yet: a room can name it, and it shows when it is converted.
pub(crate) fn files(
    directory: &Path,
    extensions: &[&str],
    converters: &Converters,
) -> Vec<PathBuf> {
    let files_of = |directory: &Path| -> Vec<PathBuf> {
        let entries = std::fs::read_dir(directory).into_iter().flatten().flatten();
        entries.map(|entry| entry.path()).collect()
    };
    let top = files_of(directory);
    let below = top
        .iter()
        .filter(|path| path.is_dir())
        .flat_map(|path| files_of(path));
    // The end of the name: an extension can have a dot (`routine.ron`).
    let wanted = |path: &PathBuf| {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_lowercase());
        let ends = |name: &String| {
            extensions
                .iter()
                .any(|end| name.ends_with(&format!(".{end}")))
        };
        name.is_some_and(|name| ends(&name))
    };
    let mut found: Vec<PathBuf> = top.iter().cloned().chain(below).filter(wanted).collect();
    found.extend(converters.missing_results(directory, extensions));
    found.sort();
    found.dedup();
    let relative = |path: PathBuf| {
        path.strip_prefix(directory)
            .map(Path::to_owned)
            .unwrap_or(path)
    };
    let found = found.into_iter().map(relative);
    found
        .map(|path| PathBuf::from(path.to_string_lossy().replace('\\', "/")))
        .collect()
}

/// The directory of the open room file, and the converter table, for the file
/// pickers of this frame. The property panel sets them before it draws.
pub(crate) fn set_room_directory(ui: &Ui, directory: &Path, converters: &Converters) {
    let id = egui::Id::new("room directory");
    ui.data_mut(|data| data.insert_temp(id, (directory.to_owned(), converters.clone())));
}

/// The mark of a file that the editor makes from a source file of another program
/// (`crate::sources`), in a file list: its hover text names the source.
pub(crate) fn source_mark(ui: &mut Ui, file: &Path, converters: &Converters) {
    let Some(source) = converters.source_of(file) else {
        return;
    };
    let source = source.file_name().unwrap_or_default().to_string_lossy();
    let text = match file.exists() {
        true => format!("Made from {source}: the editor writes it again when that file is saved."),
        false => format!("Not there yet: the editor makes it from {source}."),
    };
    ui.weak(maki_ui::theme::icon::SOURCE).on_hover_text(text);
}

/// A file of the directory of the room file, picked from the list of the files
/// with these extensions. A path that the list does not have shows in red. An image
/// file and a model file show with a small picture, in the list and at the value.
pub fn file(ui: &mut Ui, text: &str, value: &mut PathBuf, extensions: &[&str]) -> bool {
    let id = egui::Id::new("room directory");
    let set: Option<(PathBuf, Converters)> = ui.data(|data| data.get_temp(id));
    let (directory, converters) = set.unwrap_or_default();
    let known: Vec<String> = files(&directory, extensions, &converters)
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    // Forward slashes: the same room file on each system.
    let mut name = value.to_string_lossy().replace('\\', "/");
    row(ui, text, |ui| {
        let show = |ui: &mut Ui, name: &str| {
            crate::pictures::show(ui, &directory.join(name));
            source_mark(ui, &directory.join(name), &converters);
        };
        let pictures = crate::pictures::has_picture(extensions);
        if pictures && !name.is_empty() {
            show(ui, &name);
        }
        let picture: Option<crate::event_panel::Picture> =
            if pictures { Some(&show) } else { None };
        let changed =
            crate::event_panel::picker_with(ui, text, &mut name, &known, &[], false, picture);
        if changed {
            *value = PathBuf::from(name);
        }
        changed
    })
}

/// A name of a list, picked from it by a click, with a search field: a thing of the
/// room that a command names. A name that the list does not have shows in red.
pub fn pick(ui: &mut Ui, text: &str, value: &mut String, known: &[String]) -> bool {
    row(ui, text, |ui| {
        crate::event_panel::picker(ui, text, value, known, &[], false)
    })
}

/// The objects of a room (or of a prefab file) that a pick of objects offers: each
/// with an id, and its name or "object 7".
pub fn object_names(objects: &[ObjectDef]) -> Vec<(ObjectId, String)> {
    let name = |object: &ObjectDef, id: ObjectId| {
        object
            .name
            .clone()
            .unwrap_or_else(|| format!("object {}", id.0))
    };
    objects
        .iter()
        .filter_map(|object| Some((object.id?, name(object, object.id?))))
        .collect()
}

/// The text of a reference: the name of its object, or "missing (id 7)".
pub fn object_label(object: &ObjectId, objects: &[(ObjectId, String)]) -> String {
    match objects.iter().find(|(other, _)| other == object) {
        Some((_, name)) => name.clone(),
        None => format!("missing ({object})"),
    }
}

/// The id of the object of a picked name. No such name: no object (the default).
pub fn object_ref(name: &str, objects: &[(ObjectId, String)]) -> ObjectId {
    let found = objects.iter().find(|(_, other)| other == name);
    found.map(|(id, _)| *id).unwrap_or_default()
}

/// Edits an object reference as the name of its object: `edit` gets the name (or
/// "missing (id 7)", which no list has, so it shows in red) and picks another. The
/// id of the picked object is stored.
pub fn object_pick(
    object: &mut ObjectId,
    objects: &[(ObjectId, String)],
    edit: impl FnOnce(&mut String) -> bool,
) -> bool {
    let before = object_label(object, objects);
    let mut text = before.clone();
    let changed = edit(&mut text);
    if text != before {
        *object = object_ref(&text, objects);
    }
    changed
}

/// The extensions of the sound files.
pub const SOUNDS: [&str; 2] = ["wav", "ogg"];
/// The extensions of the picture files that a texture can be.
pub(crate) const PICTURES: [&str; 3] = ["png", "jpg", "jpeg"];
/// The extensions of the model files.
pub(crate) const MODELS: [&str; 2] = ["gltf", "glb"];
/// The extension of the prefab files.
pub(crate) const PREFABS: [&str; 1] = ["prefab"];
/// The end of the name of the routine files.
pub(crate) const ROUTINES: [&str; 1] = [maki_events::ROUTINE_EXTENSION];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pick_of_objects_shows_names_and_stores_ids() {
        let objects = [
            (ObjectId(3), "lamp".to_owned()),
            (ObjectId(5), "door".to_owned()),
        ];
        let mut object = ObjectId(3);
        assert_eq!(object_label(&object, &objects), "lamp");
        // The pick gets the name, and the id of the picked name is stored.
        let picked = object_pick(&mut object, &objects, |name| {
            assert_eq!(name, "lamp");
            *name = "door".to_owned();
            true
        });
        assert!(picked);
        assert_eq!(object, ObjectId(5));
        // An id with no object: no list has its text.
        let mut missing = ObjectId(7);
        assert_eq!(object_label(&missing, &objects), "missing (id 7)");
        assert!(!object_pick(&mut missing, &objects, |_| false));
        assert_eq!(missing, ObjectId(7));
    }

    #[test]
    fn an_object_with_an_id_and_no_name_is_in_the_pick() {
        let objects: Vec<ObjectDef> = maki_scene::ron_options()
            .from_str(r#"[(id: 2, name: "lamp"), (id: 4), ()]"#)
            .unwrap();
        let names = object_names(&objects);
        assert_eq!(
            names,
            [
                (ObjectId(2), "lamp".to_owned()),
                (ObjectId(4), "object 4".to_owned())
            ]
        );
        assert_eq!(object_label(&ObjectId(4), &names), "object 4");
    }
}
