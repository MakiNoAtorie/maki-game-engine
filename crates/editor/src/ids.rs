//! The editor keeps the file references of the document right: a reference whose
//! path has no file, but whose id has one (`FileRef::resolve`), gets the new path;
//! a reference with a path only gets the id of its file.

use std::path::{Path, PathBuf};

use maki_assets::{FileId, FileRef};
use maki_scene::{RoomDef, ron_options};

use crate::document::Document;

/// The texts before a file reference with a path only, in the compact RON text.
// ponytail: found by the field names of the engine types; a walk of the field tables if a script type has another name.
const BARE: [&str; 3] = ["path:", "file:", "File("];

/// Gives each reference of the document that resolves by its id the new path, and
/// each reference with a path only the id of its file, and marks the document as
/// changed. Returns the count of references that changed.
///
/// The room is walked as its compact RON text: a reference with an id is
/// `(path:"...",id:"...")` there, also in the text of a component of any module.
pub fn correct_paths(document: &mut Document) -> usize {
    let directory = document.path.parent().unwrap_or(Path::new("")).to_owned();
    let Ok(text) = ron_options().to_string(&document.room) else {
        return 0;
    };
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    let mut count = 0;
    while let Some(quote) = rest.find('"') {
        let (before, from) = rest.split_at(quote);
        if let Some(start) = before.strip_suffix("(path:").map(str::len)
            && let Some((reference, length)) = reference_at(&rest[start..])
        {
            out.push_str(&rest[..start]);
            match moved(&reference, &directory) {
                Some(new) => {
                    out.push_str(&new);
                    count += 1;
                }
                None => out.push_str(&rest[start..start + length]),
            }
            rest = &rest[start + length..];
            continue;
        }
        let Some(length) = string_length(from) else {
            break;
        };
        out.push_str(before);
        let literal = &from[..length];
        let path = BARE
            .iter()
            .any(|key| before.ends_with(key))
            .then(|| ron_options().from_str::<PathBuf>(literal).ok())
            .flatten();
        match path.and_then(|path| Some((FileId::read(&directory.join(&path))?, path))) {
            Some((id, path)) => {
                let reference = FileRef { path, id: Some(id) };
                out.push_str(
                    &ron_options()
                        .to_string(&reference)
                        .expect("a path and an id"),
                );
                count += 1;
            }
            None => out.push_str(literal),
        }
        rest = &from[length..];
    }
    out.push_str(rest);
    if count > 0 {
        match ron_options().from_str::<RoomDef>(&out) {
            Ok(room) => {
                document.room = room;
                document.dirty = true;
            }
            Err(error) => {
                tracing::error!("the corrected file references do not read: {error}");
                return 0;
            }
        }
    }
    count
}

/// The text of `reference` with its new path. `None`: its file is at its path.
fn moved(reference: &FileRef, directory: &Path) -> Option<String> {
    let found = reference.resolve(directory);
    let new = found.strip_prefix(directory).ok()?;
    if found == directory.join(&reference.path) {
        return None;
    }
    // Forward slashes: the same room file on each system.
    let moved = FileRef {
        path: new.to_string_lossy().replace('\\', "/").into(),
        id: reference.id,
    };
    Some(ron_options().to_string(&moved).expect("a path and an id"))
}

/// The length of the RON string at the start of `text`, with its two quotes.
fn string_length(text: &str) -> Option<usize> {
    // The end: the first quote with no backslash before it.
    let mut escaped = false;
    text[1..].char_indices().find_map(|(index, c)| {
        let end = c == '"' && !escaped;
        escaped = c == '\\' && !escaped;
        end.then_some(index + 2)
    })
}

/// The reference with an id at the start of `text`, `(path:"...",id:"...")`, and
/// the length of its text. `None`: another value starts there.
fn reference_at(text: &str) -> Option<(FileRef, usize)> {
    let path_end = "(path:".len() + string_length(&text["(path:".len()..])?;
    let tail = text[path_end..].strip_prefix(",id:\"")?;
    tail.get(..16)?.parse::<FileId>().ok()?;
    tail.get(16..)?.strip_prefix("\")")?;
    let length = path_end + ",id:\"".len() + 16 + "\")".len();
    let reference = ron_options().from_str(&text[..length]).ok()?;
    Some((reference, length))
}

#[cfg(test)]
mod tests {
    use super::*;
    use maki_assets::FileId;
    use maki_scene::MeshDef;

    #[test]
    fn a_model_moved_with_its_id_file_gets_its_new_path_and_the_room_is_changed() {
        let directory = std::env::temp_dir().join(format!("maki-moved-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(directory.join("models/new")).unwrap();
        let (old, new) = (
            directory.join("models/cabinet.glb"),
            directory.join("models/new/cabinet.glb"),
        );
        std::fs::write(&old, "glb").unwrap();
        let id = FileId(0x9f3a_2c00_1122_3344);
        id.write(&old).unwrap();
        // A model by path and id, a picture with a path only, and a sound.
        let text = format!(
            r#"(width: 4.0, depth: 4.0, height: 3.0,
            floor: Some((path: "floor.png")),
            objects: [
                (name: "cabinet", components: {{"prop": (mesh: File((path: "models/cabinet.glb", id: "{id}")))}}),
                (name: "other", components: {{"prop": (mesh: File((path: "models/cabinet.glb", id: "{id}")))}}),
            ])"#
        );
        let room = ron_options().from_str(&text).unwrap();
        let mut document = Document {
            path: directory.join("a.ron"),
            room,
            notes: Default::default(),
            dirty: false,
        };
        // In its place: nothing changes.
        assert_eq!(correct_paths(&mut document), 0);
        assert!(!document.dirty);

        // Moved in Explorer, with its id file.
        std::fs::rename(&old, &new).unwrap();
        std::fs::rename(FileId::file_of(&old), FileId::file_of(&new)).unwrap();
        maki_assets::forget_id_tables();
        assert_eq!(correct_paths(&mut document), 2);
        assert!(document.dirty);
        let mesh = document.room.objects[0]
            .component::<maki_scene::PropDef>("prop")
            .unwrap()
            .unwrap()
            .mesh;
        let MeshDef::File(file) = mesh else {
            panic!("a file")
        };
        assert_eq!(file.path, Path::new("models/new/cabinet.glb"));
        assert_eq!(file.id, Some(id));
        assert_eq!(
            document.room.floor.as_ref().unwrap().path.path,
            Path::new("floor.png")
        );
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_reference_with_a_path_only_gets_the_id_of_its_file() {
        let directory = std::env::temp_dir().join(format!("maki-bare-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let id = FileId(0x0123_4567_89ab_cdef);
        for name in ["cabinet.glb", "b.ron"] {
            std::fs::write(directory.join(name), "file").unwrap();
            id.write(&directory.join(name)).unwrap();
        }
        // The room of a `Transfer` is a text, not a file reference: it gets no id.
        let text = maki_scene::room_text(
            4.0,
            4.0,
            3.0,
            &[
                r#"(name: "cabinet", components: {"prop": (mesh: File("cabinet.glb"))})"#,
                r#"(name: "other", components: {"prop": (mesh: File("other.glb"))})"#,
                r#"(name: "go", components: {"event": (pages: [(commands: [Transfer("b.ron", 1)])])})"#,
            ],
        );
        let mut document = Document {
            path: directory.join("a.ron"),
            room: ron_options().from_str(&text).unwrap(),
            notes: Default::default(),
            dirty: false,
        };
        assert_eq!(correct_paths(&mut document), 1);
        assert!(document.dirty);
        let mesh = |index: usize| {
            document.room.objects[index]
                .component::<maki_scene::PropDef>("prop")
                .unwrap()
                .unwrap()
                .mesh
        };
        let MeshDef::File(file) = mesh(0) else {
            panic!("a file")
        };
        assert_eq!(file.id, Some(id));
        let MeshDef::File(file) = mesh(1) else {
            panic!("a file")
        };
        assert_eq!(file.id, None);
        let event = ron_options().to_string(&document.room.objects[2]).unwrap();
        assert!(event.contains(r#"Transfer("b.ron",1)"#), "{event}");
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
