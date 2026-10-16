//! The identity of a file, and a reference to a file by path and id.
//!
//! An asset file can have an id in a file beside it, `<file>.id` (`cabinet.glb.id`),
//! as the `.meta` of Unity and the `.uid` of Godot 4. A reference keeps its file when
//! the file moves with its id file: [`FileRef::resolve`] finds the new path.

use std::collections::HashMap;
use std::fmt;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use maki_core::fields::{Fields, Kind, Map, Value};
use serde::de::{self, MapAccess, Visitor};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The identity of a file: a random 64-bit number, written as 16 hexadecimal digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId(pub u64);

impl FileId {
    /// The id file of an asset: the name of the asset with `.id` added.
    pub fn file_of(asset: &Path) -> PathBuf {
        let mut name = asset.as_os_str().to_owned();
        name.push(".id");
        PathBuf::from(name)
    }

    /// The id in the id file beside `asset`. `None`: no id file, or no id in it.
    pub fn read(asset: &Path) -> Option<FileId> {
        std::fs::read_to_string(Self::file_of(asset))
            .ok()?
            .trim()
            .parse()
            .ok()
    }

    /// A new id. `seed`: the path of the asset, so two files of one moment differ.
    // ponytail: std `RandomState` keys over time, path, and a count; a generator crate if ids collide.
    pub fn random(seed: &Path) -> FileId {
        use std::hash::{BuildHasher, Hash, Hasher};
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNT: AtomicU64 = AtomicU64::new(0);
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        seed.hash(&mut hasher);
        std::time::SystemTime::now().hash(&mut hasher);
        COUNT.fetch_add(1, Ordering::Relaxed).hash(&mut hasher);
        FileId(hasher.finish())
    }

    /// Writes the id file of `asset`. The only writer of id files: the text goes to
    /// a second file first, so a reader never sees half of an id. The game never
    /// calls it; the editor does.
    pub fn write(self, asset: &Path) -> std::io::Result<()> {
        let file = Self::file_of(asset);
        let mut new = file.as_os_str().to_owned();
        new.push(".new");
        std::fs::write(&new, format!("{self}\n"))?;
        std::fs::rename(&new, &file)
    }
}

impl fmt::Display for FileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

impl std::str::FromStr for FileId {
    type Err = std::num::ParseIntError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        u64::from_str_radix(text, 16).map(FileId)
    }
}

impl Serialize for FileId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for FileId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(|_| {
            de::Error::custom(format!(
                "`{text}` is not a file id of 16 hexadecimal digits"
            ))
        })
    }
}

/// A reference to a file: its path, and its id when it has one. In RON a path alone
/// (`"models/cabinet.glb"`) or `(path: "models/cabinet.glb", id: "9f3a2c...")`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileRef {
    pub path: PathBuf,
    pub id: Option<FileId>,
}

impl FileRef {
    /// The file of this reference, for a store. The path from `directory` if its file
    /// is there with the id. Else the path of the id, in a table of the id files under
    /// `directory`, with a warning. Else the path: the store reports the missing file.
    pub fn resolve(&self, directory: &Path) -> PathBuf {
        let path = directory.join(&self.path);
        let Some(id) = self.id else {
            return path;
        };
        if path.is_file() && FileId::read(&path) == Some(id) {
            return path;
        }
        match find_by_id(directory, id) {
            Some(found) => {
                tracing::warn!(old = %path.display(), new = %found.display(), %id, "a file has moved");
                found
            }
            None => {
                if !path.is_file() {
                    tracing::error!(path = %path.display(), %id, "no file with this path or id");
                }
                path
            }
        }
    }
}

/// The tables of id files, by the folder they are of. A table is built at the first
/// look by id under its folder, then kept: the game does not move files.
static TABLES: Mutex<Option<HashMap<PathBuf, HashMap<FileId, PathBuf>>>> = Mutex::new(None);

fn find_by_id(folder: &Path, id: FileId) -> Option<PathBuf> {
    let mut tables = TABLES.lock().unwrap_or_else(PoisonError::into_inner);
    let tables = tables.get_or_insert_with(HashMap::new);
    let table = tables.entry(folder.to_path_buf()).or_insert_with(|| {
        let mut table = HashMap::new();
        add_id_files(folder, &mut table);
        table
    });
    table.get(&id).cloned()
}

/// Forgets the tables of id files: the next look by id walks the folder again. The
/// editor calls it when the files of the asset folder change.
pub fn forget_id_tables() {
    *TABLES.lock().unwrap_or_else(PoisonError::into_inner) = None;
}

/// True if the table of id files of `folder` is built.
pub fn has_id_table(folder: &Path) -> bool {
    let tables = TABLES.lock().unwrap_or_else(PoisonError::into_inner);
    tables
        .as_ref()
        .is_some_and(|tables| tables.contains_key(folder))
}

fn add_id_files(folder: &Path, table: &mut HashMap<FileId, PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for path in entries.filter_map(|entry| Some(entry.ok()?.path())) {
        if path.is_dir() {
            add_id_files(&path, table);
        } else if path.extension().is_some_and(|extension| extension == "id") {
            // `cabinet.glb.id` is the id of `cabinet.glb`.
            let asset = path.with_extension("");
            if let Some(id) = FileId::read(&asset) {
                table.insert(id, asset);
            }
        }
    }
}

impl Deref for FileRef {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<Path> for FileRef {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl From<PathBuf> for FileRef {
    fn from(path: PathBuf) -> Self {
        FileRef { path, id: None }
    }
}

impl From<&str> for FileRef {
    fn from(path: &str) -> Self {
        PathBuf::from(path).into()
    }
}

// A reference with no id is a path alone, as the files of before.
impl Serialize for FileRef {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.id {
            None => self.path.serialize(serializer),
            Some(id) => {
                let mut state = serializer.serialize_struct("FileRef", 2)?;
                state.serialize_field("path", &self.path)?;
                state.serialize_field("id", &id)?;
                state.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for FileRef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(FileRefVisitor)
    }
}

struct FileRefVisitor;

impl<'de> Visitor<'de> for FileRefVisitor {
    type Value = FileRef;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a path, or (path: \"...\", id: \"...\")")
    }

    fn visit_str<E: de::Error>(self, path: &str) -> Result<FileRef, E> {
        Ok(path.into())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<FileRef, A::Error> {
        let (mut path, mut id) = (None, None);
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "path" => path = Some(map.next_value()?),
                "id" => id = Some(map.next_value()?),
                other => return Err(de::Error::unknown_field(other, &["path", "id"])),
            }
        }
        let path = path.ok_or_else(|| de::Error::missing_field("path"))?;
        Ok(FileRef { path, id })
    }
}

impl Fields for FileRef {
    fn kind() -> Kind {
        Kind::File("")
    }

    // With an id the tree keeps it: a tool that changes another field writes it back.
    fn tree(&self) -> Value {
        let path = Value::String(self.path.to_string_lossy().into_owned());
        match self.id {
            None => path,
            Some(id) => {
                let mut map = Map::new();
                map.insert("path".into(), path);
                map.insert("id".into(), Value::String(id.to_string()));
                Value::Object(map)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("maki-ids-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("models/old")).unwrap();
        std::fs::create_dir_all(dir.join("models/new")).unwrap();
        dir
    }

    fn asset(path: &Path, id: Option<&str>) {
        std::fs::write(path, "glb").unwrap();
        if let Some(id) = id {
            std::fs::write(FileId::file_of(path), id).unwrap();
        }
    }

    const ID: &str = "9f3a2c0011223344";

    #[test]
    fn a_reference_reads_as_a_path_or_with_an_id() {
        let bare: FileRef = ron::from_str(r#""models/cabinet.glb""#).unwrap();
        assert_eq!(bare, FileRef::from("models/cabinet.glb"));
        let full: FileRef = ron::from_str(&format!(r#"(path: "a.glb", id: "{ID}")"#)).unwrap();
        assert_eq!(full.id, Some(FileId(0x9f3a2c0011223344)));
        assert_eq!(full.path, Path::new("a.glb"));
        // A path alone is written as before; with an id, both.
        assert_eq!(ron::to_string(&bare).unwrap(), r#""models/cabinet.glb""#);
        assert_eq!(
            ron::from_str::<FileRef>(&ron::to_string(&full).unwrap()).unwrap(),
            full
        );
        // The tree of a tool reads back.
        let tree: FileRef = serde_json::from_value(full.tree()).unwrap();
        assert_eq!(tree, full);
    }

    #[test]
    fn a_moved_file_resolves_by_its_id() {
        let dir = folder("moved");
        asset(&dir.join("models/new/cabinet.glb"), Some(ID));
        let file = FileRef {
            path: "models/old/cabinet.glb".into(),
            id: Some(ID.parse().unwrap()),
        };
        assert_eq!(file.resolve(&dir), dir.join("models/new/cabinet.glb"));
    }

    #[test]
    fn the_table_is_built_only_when_a_path_does_not_resolve() {
        let dir = folder("table");
        asset(&dir.join("models/old/with_id.glb"), Some(ID));
        asset(&dir.join("models/old/no_id.glb"), None);
        let with_id = FileRef {
            path: "models/old/with_id.glb".into(),
            id: Some(ID.parse().unwrap()),
        };
        assert_eq!(with_id.resolve(&dir), dir.join("models/old/with_id.glb"));
        let bare = FileRef::from("models/old/gone.glb");
        assert_eq!(bare.resolve(&dir), dir.join("models/old/gone.glb"));
        assert!(!has_id_table(&dir));

        // An id that no file has: the path, for the error of the store.
        let lost = FileRef {
            path: "models/old/gone.glb".into(),
            id: Some(FileId(7)),
        };
        assert_eq!(lost.resolve(&dir), dir.join("models/old/gone.glb"));
        assert!(has_id_table(&dir));
    }
}
