//! The editor corrects the paths of moved files: a reference of the document whose
//! path has no file, but whose id has one (`FileRef::resolve`), gets the new path.

use std::path::Path;

use maki_assets::FileRef;
use maki_scene::{RoomDef, ron_options};

use crate::document::Document;

/// Gives each reference of the document that resolves by its id the new path, and
/// marks the document as changed. Returns the count of references that changed.
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
    while let Some(start) = rest.find("(path:\"") {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some((reference, length)) = reference_at(rest) else {
            out.push('(');
            rest = &rest[1..];
            continue;
        };
        let found = reference.resolve(&directory);
        match found.strip_prefix(&directory) {
            Ok(new) if found != directory.join(&reference.path) => {
                // Forward slashes: the same room file on each system.
                let path = new.to_string_lossy().replace('\\', "/");
                let moved = FileRef {
                    path: path.into(),
                    id: reference.id,
                };
                out.push_str(&ron_options().to_string(&moved).expect("a path and an id"));
                count += 1;
            }
            _ => out.push_str(&rest[..length]),
        }
        rest = &rest[length..];
    }
    out.push_str(rest);
    if count > 0 {
        match ron_options().from_str::<RoomDef>(&out) {
            Ok(room) => {
                document.room = room;
                document.dirty = true;
            }
            Err(error) => {
                tracing::error!("the new paths of moved files do not read: {error}");
                return 0;
            }
        }
    }
    count
}

/// The reference with an id at the start of `text`, `(path:"...",id:"...")`, and
/// the length of its text. `None`: another value starts there.
fn reference_at(text: &str) -> Option<(FileRef, usize)> {
    // The end of the path string: the first quote with no backslash before it.
    let body = &text["(path:".len()..];
    let mut escaped = false;
    let close = body[1..].char_indices().find_map(|(index, c)| {
        let end = c == '"' && !escaped;
        escaped = c == '\\' && !escaped;
        end.then_some(index + 1)
    })?;
    let after = &body[close + 1..];
    let tail = after.strip_prefix(",id:\"")?;
    let id = tail.get(..16)?;
    tail.get(16..)?.strip_prefix("\")")?;
    let length = "(path:".len() + close + 1 + ",id:\"".len() + 16 + "\")".len();
    let reference = ron_options().from_str(&text[..length]).ok()?;
    id.parse::<maki_assets::FileId>().ok()?;
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
}
