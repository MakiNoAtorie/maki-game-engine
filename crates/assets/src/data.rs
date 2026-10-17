//! Data assets: a file that holds one value of a type that a module registers (as
//! the ScriptableObject of Unity, the Resource of Godot).
//!
//! ```ignore
//! /// A kind of item.
//! #[derive(Default, Serialize, Deserialize, Fields)]
//! struct Item {
//!     name: String,
//! }
//!
//! impl DataAsset for Item {
//!     const NAME: &'static str = "item";
//! }
//!
//! add_data_type::<Item>(app);
//! ```
//!
//! A file of the type `item` is `<name>.item.ron`: the end of the name says the type,
//! as `.routine.ron` does, and the content is the RON of the value, with no header.
//! A field [`DataRef<Item>`] names one. [`DataAssets`] reads a file at its first use
//! and forgets it when the watch tells of a change, as the routine files do.

use std::any::Any;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bevy_ecs::change_detection::DetectChanges;
use bevy_ecs::message::MessageReader;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Res, ResMut};
use maki_core::fields::{self, Kind, Value};
use maki_core::{App, Fields};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{AssetChanged, AssetWatch, Check, FileRef};

/// The folder of the data files, in the directory of the room files.
pub const DATA_FOLDER: &str = "data";

/// A type whose values are data files. `NAME` is the type in the file name
/// (`key.item.ron`) and the group of the files that a [`DataRef`] picks from.
pub trait DataAsset:
    Fields + Serialize + DeserializeOwned + Default + Send + Sync + 'static
{
    const NAME: &'static str;
}

/// What a tool needs of a data asset type, with no Rust type.
pub struct DataType {
    pub name: &'static str,
    /// The table of the fields.
    pub kind: Kind,
    /// Reads a tree as the type: the tree of what was read, or why it does not read.
    pub check_tree: fn(&Value) -> Result<Value, String>,
    /// The tree of the text of a file. `Err`: the text does not read as the type.
    pub tree_of_text: fn(&str) -> Result<Value, String>,
    /// The text of a file of a tree. `Err`: the tree is not of the type.
    pub text: fn(&Value) -> Result<String, String>,
    /// The text of a file with the default value.
    pub default_text: fn() -> String,
}

/// The data asset types that the modules registered, by name. Not `FieldTypes`: a
/// data type and a component type of one name both stay.
#[derive(Resource, Default)]
pub struct DataTypes(BTreeMap<&'static str, DataType>);

impl DataTypes {
    pub fn get(&self, name: &str) -> Option<&DataType> {
        self.0.get(name)
    }

    /// In the order of the names.
    pub fn iter(&self) -> impl Iterator<Item = &DataType> {
        self.0.values()
    }

    /// The end of the name of the files of a type: `item.ron`.
    pub fn extension(name: &str) -> String {
        format!("{name}.ron")
    }

    /// The type in the name of a data file (`item` of `key.item.ron`), registered or
    /// not. `None`: the name has no type (`room_a.ron`).
    pub fn type_of(path: &Path) -> Option<&str> {
        let name = path.file_name()?.to_str()?.strip_suffix(".ron")?;
        let (stem, kind) = name.rsplit_once('.')?;
        (!stem.is_empty() && !kind.is_empty()).then_some(kind)
    }

    /// Each data file under `directory/data`, with its type and whether a module
    /// registered the type. A file of an unknown type is listed and kept, as a
    /// component with no code is (2.14). In the order of the paths.
    pub fn files(&self, directory: &Path) -> Vec<DataFile> {
        let mut files = Vec::new();
        add_files(&directory.join(DATA_FOLDER), &mut files);
        let mut list: Vec<DataFile> = files
            .into_iter()
            .filter_map(|path| {
                let type_name = Self::type_of(&path)?.to_owned();
                let known = self.0.contains_key(type_name.as_str());
                let path = path.strip_prefix(directory).ok()?.to_owned();
                Some(DataFile {
                    path,
                    type_name,
                    known,
                })
            })
            .collect();
        list.sort_by(|a, b| a.path.cmp(&b.path));
        list
    }
}

/// A data file of [`DataTypes::files`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataFile {
    /// From the directory of the room files.
    pub path: PathBuf,
    pub type_name: String,
    /// False: no module registered the type.
    pub known: bool,
}

fn add_files(folder: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for path in entries.filter_map(|entry| Some(entry.ok()?.path())) {
        if path.is_dir() {
            add_files(&path, files);
        } else {
            files.push(path);
        }
    }
}

/// Registers a data asset type. A name in use gets the new type, with a warning: a
/// project can put its own type in the place of one of the engine.
pub fn add_data_type<T: DataAsset>(app: &mut App) {
    let data_type = DataType {
        name: T::NAME,
        kind: T::kind(),
        check_tree: |tree| {
            let value = fields::read::<T>(tree.clone()).map_err(|error| error.to_string())?;
            Ok(value.tree())
        },
        tree_of_text: |text| {
            let value: T = ron_options()
                .from_str(text)
                .map_err(|error| error.to_string())?;
            Ok(value.tree())
        },
        text: |tree| {
            let value = fields::read::<T>(tree.clone()).map_err(|error| error.to_string())?;
            Ok(data_text(&value))
        },
        default_text: || data_text(&T::default()),
    };
    app.init_resource::<DataTypes>();
    let mut types = app.resource_mut::<DataTypes>();
    if types.0.insert(T::NAME, data_type).is_some() {
        tracing::warn!(name = T::NAME, "a data asset type of this name is replaced");
    }
}

fn ron_options() -> ron::Options {
    ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
}

/// The text of a data file.
pub fn data_text<T: Serialize>(value: &T) -> String {
    let text = ron_options().to_string_pretty(value, ron::ser::PrettyConfig::default());
    text.expect("a data asset has no value that RON cannot write")
}

/// Reads a data file as its type. The error has the path, the line, and the column.
pub fn read_data<T: DataAsset>(path: &Path) -> Result<T, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("no data file {}: {error}", path.display()))?;
    ron_options()
        .from_str(&text)
        .map_err(|error| format!("{}:{error}", path.display()))
}

/// The check of the watch: the file reads as its type.
fn check_file<T: DataAsset>(path: &Path) -> anyhow::Result<()> {
    read_data::<T>(path).map(drop).map_err(anyhow::Error::msg)
}

/// A reference to a data file of the type `T`: a [`FileRef`] that a tool picks from
/// the files of the type. In RON as a `FileRef`.
pub struct DataRef<T> {
    pub file: FileRef,
    // `fn() -> T`: the reference is `Send` and `Sync` for every `T`.
    marker: PhantomData<fn() -> T>,
}

impl<T> DataRef<T> {
    pub fn new(file: impl Into<FileRef>) -> Self {
        Self {
            file: file.into(),
            marker: PhantomData,
        }
    }
}

// Written by hand: `derive` would demand the same trait of `T`.
impl<T> Clone for DataRef<T> {
    fn clone(&self) -> Self {
        Self::new(self.file.clone())
    }
}
impl<T> PartialEq for DataRef<T> {
    fn eq(&self, other: &Self) -> bool {
        self.file == other.file
    }
}
impl<T> Default for DataRef<T> {
    fn default() -> Self {
        Self::new(FileRef::default())
    }
}
impl<T> fmt::Debug for DataRef<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DataRef({:?})", self.file)
    }
}

impl<T> Serialize for DataRef<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.file.serialize(serializer)
    }
}

impl<'de, T> Deserialize<'de> for DataRef<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        FileRef::deserialize(deserializer).map(Self::new)
    }
}

impl<T: DataAsset> Fields for DataRef<T> {
    fn kind() -> Kind {
        Kind::File(T::NAME)
    }

    fn tree(&self) -> Value {
        self.file.tree()
    }
}

struct Loaded {
    value: Arc<dyn Any + Send + Sync>,
    check: Check,
}

/// The data files that were read, by path. A file that changed on the disk is read
/// again at its next use.
#[derive(Resource, Default)]
pub struct DataAssets(HashMap<PathBuf, Loaded>);

impl DataAssets {
    /// The value of a data file under `directory` (the directory of the room file):
    /// from the store, else read now. `Err`: why there is no value; nothing is kept.
    pub fn get<T: DataAsset>(
        &mut self,
        file: &FileRef,
        directory: &Path,
    ) -> Result<Arc<T>, String> {
        let path = file.resolve(directory);
        if let Some(loaded) = self.0.get(&path) {
            // A path has one type: the end of its name.
            return Arc::clone(&loaded.value).downcast::<T>().map_err(|_| {
                format!(
                    "{} is not a data asset of the type `{}`",
                    path.display(),
                    T::NAME
                )
            });
        }
        let value = Arc::new(read_data::<T>(&path)?);
        let check: Check = check_file::<T>;
        let loaded = Loaded {
            value: value.clone(),
            check,
        };
        self.0.insert(path, loaded);
        Ok(value)
    }

    /// Forgets a file: its next use reads it again.
    pub fn forget(&mut self, path: &Path) {
        self.0.remove(path);
    }
}

/// Runs in `Update`: a data file that changed is read again at its next use.
pub(crate) fn forget_changed_data(
    mut changed: MessageReader<AssetChanged>,
    mut data: ResMut<DataAssets>,
) {
    for AssetChanged(path) in changed.read() {
        if data.0.contains_key(path) {
            data.forget(path);
        }
    }
}

/// Runs in `Update`: tells the watch the data files that were read.
pub(crate) fn watch_data(data: Res<DataAssets>, mut watch: ResMut<AssetWatch>) {
    if watch.on && data.is_changed() {
        let files = data
            .0
            .iter()
            .map(|(path, loaded)| (path.clone(), loaded.check));
        watch.set("data", files.collect());
    }
}

#[cfg(test)]
mod tests {
    use bevy_ecs::message::Messages;
    use maki_core::ConfigFile;
    use serde::{Deserialize, Serialize};

    use super::*;

    /// A data type of a test.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, maki_core::Fields)]
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

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("maki-data-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(DATA_FOLDER)).unwrap();
        dir
    }

    fn app() -> App {
        let mut app = App::new(ConfigFile::default()).unwrap();
        app.add_module(crate::AssetsModule);
        add_data_type::<Lamp>(&mut app);
        app
    }

    #[test]
    fn a_registered_type_writes_its_default_file_and_reads_it_back() {
        let app = app();
        let dir = folder("default");
        let types = app.world().resource::<DataTypes>();
        let lamp = types.get("lamp").unwrap();
        let path = dir.join("data/hall.lamp.ron");
        std::fs::write(&path, (lamp.default_text)()).unwrap();
        assert_eq!(read_data::<Lamp>(&path).unwrap(), Lamp::default());
        // A tree of a tool writes the file of the type.
        let mut tree = Lamp::default().tree();
        tree["power"] = 5.0.into();
        std::fs::write(&path, (lamp.text)(&tree).unwrap()).unwrap();
        assert_eq!(read_data::<Lamp>(&path).unwrap().power, 5.0);
        assert!((lamp.text)(&serde_json::json!({"power": "x"})).is_err());
        // The text of a file is a tree of the type, and the tree writes the same text.
        let back = (lamp.tree_of_text)(&(lamp.default_text)()).unwrap();
        assert_eq!(back, Lamp::default().tree());
        assert!((lamp.tree_of_text)("(power: \"x\")").is_err());
    }

    #[test]
    fn a_file_that_does_not_read_gives_its_line_and_no_value() {
        let dir = folder("broken");
        std::fs::write(dir.join("data/hall.lamp.ron"), "(\n  power: \"x\",\n)").unwrap();
        let mut data = DataAssets::default();
        let error = data
            .get::<Lamp>(&"data/hall.lamp.ron".into(), &dir)
            .unwrap_err();
        assert!(error.contains("hall.lamp.ron:2:"), "{error}");
        assert!(data.0.is_empty());
    }

    #[test]
    fn a_changed_file_gives_the_new_value_after_asset_changed() {
        let mut app = app();
        let dir = folder("changed");
        let path = dir.join("data/hall.lamp.ron");
        std::fs::write(&path, "(power: 1.0)").unwrap();
        let file = FileRef::from("data/hall.lamp.ron");
        let get = |app: &mut App| {
            let mut data = app.resource_mut::<DataAssets>();
            data.get::<Lamp>(&file, &dir).unwrap().power
        };
        assert_eq!(get(&mut app), 1.0);
        std::fs::write(&path, "(power: 3.0)").unwrap();
        // The store keeps what it read until the watch tells of the change.
        assert_eq!(get(&mut app), 1.0);
        app.resource_mut::<Messages<AssetChanged>>()
            .write(AssetChanged(path.clone()));
        app.update();
        assert_eq!(get(&mut app), 3.0);
    }

    #[test]
    fn a_file_of_an_unknown_type_is_listed_and_kept() {
        let app = app();
        let dir = folder("unknown");
        std::fs::write(dir.join("data/hall.lamp.ron"), "()").unwrap();
        std::fs::write(dir.join("data/orc.enemy.ron"), "(life: 3)").unwrap();
        std::fs::write(dir.join("data/notes.txt"), "").unwrap();
        let files = app.world().resource::<DataTypes>().files(&dir);
        let listed: Vec<_> = files
            .iter()
            .map(|file| (file.path.to_str().unwrap().replace('\\', "/"), file.known))
            .collect();
        assert_eq!(
            listed,
            [
                ("data/hall.lamp.ron".to_owned(), true),
                ("data/orc.enemy.ron".to_owned(), false)
            ]
        );
        assert!(dir.join("data/orc.enemy.ron").is_file());
    }

    #[test]
    fn a_data_type_and_a_field_type_of_one_name_both_stay() {
        let mut app = app();
        maki_core::add_field_type::<FileRef>(&mut app, "lamp");
        assert!(app.world().resource::<DataTypes>().get("lamp").is_some());
        let field_types = app.world().resource::<maki_core::FieldTypes>();
        assert_eq!(field_types.get("lamp").unwrap().kind, Kind::File(""));
    }

    #[test]
    fn a_reference_picks_the_files_of_its_type_and_reads_as_a_file_ref() {
        assert_eq!(DataRef::<Lamp>::kind(), Kind::File("lamp"));
        let reference: DataRef<Lamp> = ron::from_str(r#""data/hall.lamp.ron""#).unwrap();
        assert_eq!(reference.file, FileRef::from("data/hall.lamp.ron"));
        assert_eq!(
            DataTypes::type_of(Path::new("a/key.item.ron")),
            Some("item")
        );
        assert_eq!(DataTypes::type_of(Path::new("room_a.ron")), None);
    }
}
