//! Text in tables: a text that the player reads names a key, and a text table of
//! each language has the text of the key.
//!
//! ```ron
//! // data/text/en.text.ron
//! (
//!     language: "en",
//!     texts: [(key: "door.locked", text: "The door is locked.")],
//! )
//! ```
//!
//! [`Texts`] merges the tables of the language and of the fallback language of the
//! `[text]` table of the config. A key that neither has shows itself, with one
//! warning: a room file of before, with a text in place of a key, still plays.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::SystemTime;

use bevy_ecs::change_detection::DetectChanges;
use bevy_ecs::message::MessageReader;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Local, Res, ResMut};
use bevy_ecs::world::World;
use maki_assets::{
    AssetChanged, AssetWatch, DATA_FOLDER, DataAsset, DataAssets, DataTypes, add_data_type,
};
use maki_core::fields::{Kind, Value};
use maki_core::{App, Fields, ResetPlay, Settings, Update};
use maki_scene::CurrentRoom;
use serde::{Deserialize, Serialize};

use crate::CommandRegistry;
use crate::command::Step;

/// The texts of one language: a data asset (`<name>.text.ron`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
pub struct TextTable {
    /// The code of the language: `"en"`, `"ja"`.
    pub language: String,
    /// A list, not a map: the order of the file stays as its writer made it.
    pub texts: Vec<TextEntry>,
}

impl DataAsset for TextTable {
    const NAME: &'static str = "text";
}

impl TextTable {
    /// A table of a language with no text.
    pub fn new(language: &str) -> Self {
        Self {
            language: language.to_owned(),
            texts: Vec::new(),
        }
    }
}

/// The folder of the text tables in the data folder: a new table goes there.
pub const TEXT_FOLDER: &str = "text";

/// The text table files under `directory/data`, from `directory`, in the order of the paths.
pub fn table_files(directory: &Path) -> Vec<PathBuf> {
    let files = DataTypes::default().files(directory).into_iter();
    let tables = files.filter(|file| file.type_name == TextTable::NAME);
    tables.map(|file| file.path).collect()
}

/// The file of a new table of a language: `directory/data/text/<language>.text.ron`.
pub fn table_path(directory: &Path, language: &str) -> PathBuf {
    let name = format!("{language}.{}", DataTypes::extension(TextTable::NAME));
    directory.join(DATA_FOLDER).join(TEXT_FOLDER).join(name)
}

/// One text of a table.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
pub struct TextEntry {
    pub key: String,
    pub text: String,
}

/// A key of a text table. In RON a string, as a text in place of a key was before.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TextKey(pub String);

impl From<&str> for TextKey {
    fn from(key: &str) -> Self {
        TextKey(key.to_owned())
    }
}

impl Fields for TextKey {
    fn kind() -> Kind {
        Kind::Pick {
            list: "texts",
            text: true,
        }
    }

    fn tree(&self) -> Value {
        Value::String(self.0.clone())
    }
}

/// The `[text]` table of the config file.
#[derive(Resource, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TextSettings {
    /// The language of the game at the start.
    pub language: String,
    /// The language of a key that the language does not have.
    pub fallback: String,
}

impl Default for TextSettings {
    fn default() -> Self {
        Self {
            language: "en".to_owned(),
            fallback: "en".to_owned(),
        }
    }
}

impl Settings for TextSettings {
    const TABLE: &'static str = "text";
}

/// The texts of the game in its language. Made again at its next use after a change
/// of the language, of the room directory, or of a table file: never in a frame with
/// no change.
#[derive(Resource, Debug)]
pub struct Texts {
    language: String,
    fallback: String,
    /// The text of each key: of the fallback language, then of the language over it.
    texts: HashMap<String, String>,
    /// True: `texts` is not of the language or of the files now.
    stale: bool,
    /// The keys that no table has and that a warning told of.
    warned: Mutex<HashSet<String>>,
}

impl Texts {
    pub fn new(language: &str, fallback: &str) -> Self {
        Self {
            language: language.to_owned(),
            fallback: fallback.to_owned(),
            texts: HashMap::new(),
            stale: true,
            warned: Mutex::default(),
        }
    }

    pub fn language(&self) -> &str {
        &self.language
    }

    pub fn fallback(&self) -> &str {
        &self.fallback
    }

    /// Changes the language. The texts of the new one show at the next lookup.
    pub fn set_language(&mut self, language: &str) {
        if self.language != language {
            self.language = language.to_owned();
            self.stale = true;
        }
    }

    /// The text of a key: in the language, else in the fallback, else the key itself
    /// with one warning for each key. Use [`text`] for a value of now.
    pub fn get(&self, key: &str) -> String {
        if let Some(text) = self.texts.get(key) {
            return text.clone();
        }
        let mut warned = self.warned.lock().unwrap_or_else(PoisonError::into_inner);
        if warned.insert(key.to_owned()) {
            tracing::warn!(
                key,
                language = self.language,
                "no text table has this key: the key shows"
            );
        }
        key.to_owned()
    }

    /// Reads the tables of the language and of the fallback under `directory`.
    fn build(&mut self, directory: &Path, data: &mut DataAssets) {
        let mut tables = Vec::new();
        for path in table_files(directory) {
            match data.get::<TextTable>(&path.into(), directory) {
                Ok(table) => tables.push(table),
                Err(error) => tracing::error!("text table: {error}"),
            }
        }
        self.texts.clear();
        // The fallback first: the language writes its texts over it.
        for language in [&self.fallback, &self.language] {
            for table in tables.iter().filter(|table| table.language == *language) {
                // An empty text is a text not yet written: the fallback stays.
                for entry in table.texts.iter().filter(|entry| !entry.text.is_empty()) {
                    self.texts.insert(entry.key.clone(), entry.text.clone());
                }
            }
        }
        self.stale = false;
    }
}

/// The text of a key now, with the `{name}` of the variables (see [`fill`]). Makes the
/// texts again first if they are stale.
pub fn text(world: &mut World, key: &str, variable: impl Fn(&str) -> i32) -> String {
    fill(&texts(world).get(key), variable)
}

/// The texts, made again first if they are stale.
pub fn texts(world: &mut World) -> &Texts {
    if world.resource::<Texts>().stale {
        let directory = room_directory(world);
        world.init_resource::<DataAssets>();
        world.resource_scope(|world, mut texts: bevy_ecs::world::Mut<Texts>| {
            texts.build(&directory, &mut world.resource_mut::<DataAssets>());
        });
    }
    world.resource::<Texts>()
}

fn room_directory(world: &World) -> PathBuf {
    let room = world.get_resource::<CurrentRoom>();
    let directory = room.and_then(|room| room.path.parent());
    directory.unwrap_or(Path::new("")).to_owned()
}

/// A text with each `{name}` in place of the value of the variable `name`. `{{` is
/// `{` and `}}` is `}`; a `{` with no `}` stays as it is.
pub fn fill(text: &str, variable: impl Fn(&str) -> i32) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(['{', '}']) {
        out.push_str(&rest[..start]);
        let from = &rest[start..];
        if from.starts_with("{{") || from.starts_with("}}") {
            out.push_str(&from[..1]);
            rest = &from[2..];
        } else if let Some(name) = name_at(from) {
            out.push_str(&variable(name).to_string());
            rest = &from[name.len() + 2..];
        } else {
            out.push_str(&from[..1]);
            rest = &from[1..];
        }
    }
    out.push_str(rest);
    out
}

/// The name of `{name}` at the start of `text`: letters, digits, `_`, `.`, `-`.
fn name_at(text: &str) -> Option<&str> {
    let inner = text.strip_prefix('{')?;
    let name = &inner[..inner.find('}')?];
    let letter = |c: char| c.is_alphanumeric() || matches!(c, '_' | '.' | '-');
    (!name.is_empty() && name.chars().all(letter)).then_some(name)
}

/// Runs in `Update`: the texts are stale after a room load (another directory can
/// have other tables) and after a change of a table file. With the watch on (the
/// editor), also after a new table file: the times of the two folders change then.
// ponytail: a new table in another folder under `data/` shows at the next room load.
fn mark_stale(
    room: Option<Res<CurrentRoom>>,
    mut changed: MessageReader<AssetChanged>,
    watch: Option<Res<AssetWatch>>,
    mut folders: Local<[Option<SystemTime>; 2]>,
    mut texts: ResMut<Texts>,
) {
    let table = |path: &PathBuf| DataTypes::type_of(path) == Some(TextTable::NAME);
    let mut stale = changed.read().any(|AssetChanged(path)| table(path));
    stale |= room.as_ref().is_some_and(|room| room.is_changed());
    if let Some(room) = room.filter(|_| watch.is_some_and(|watch| watch.on)) {
        let data = room
            .path
            .parent()
            .unwrap_or(Path::new(""))
            .join(DATA_FOLDER);
        let time = |folder: &Path| std::fs::metadata(folder).and_then(|m| m.modified()).ok();
        let now = [time(&data), time(&data.join(TEXT_FOLDER))];
        stale |= *folders != now;
        *folders = now;
    }
    if stale {
        texts.stale = true;
    }
}

/// Runs in `ResetPlay`: the language of the settings again; a script of the play changed it.
fn reset_language(settings: Res<TextSettings>, mut texts: ResMut<Texts>) {
    let language = settings.language.clone();
    texts.set_language(&language);
}

/// The commands of the texts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
pub enum TextCommand {
    /// The texts show in this language from now: an options menu. A new play starts
    /// in the language of the settings again.
    SetLanguage(#[field(label = "language")] String),
}

fn run(command: &TextCommand, _: &mut Step, world: &mut World) -> bool {
    let TextCommand::SetLanguage(language) = command;
    if world.resource::<Texts>().language() != language {
        world.resource_mut::<Texts>().set_language(language);
    }
    true
}

/// Adds the text tables, the settings, the texts, and the command `SetLanguage`.
pub(crate) fn add(app: &mut App, registry: &CommandRegistry) {
    registry.add(run);
    registry.add_fields::<TextCommand>();
    registry.add_texts(
        |TextCommand::SetLanguage(language): &TextCommand| Some(format!("Language: {language}")),
        |name| (name == "SetLanguage").then_some("The texts show in another language."),
    );
    registry.add_defaults(|_, _| Some(TextCommand::SetLanguage("en".to_owned())));
    add_data_type::<TextTable>(app);
    app.add_settings::<TextSettings>();
    let settings = app.world().resource::<TextSettings>().clone();
    app.insert_resource(Texts::new(&settings.language, &settings.fallback))
        .add_message::<AssetChanged>()
        .add_systems(Update, mark_stale.after(maki_scene::load_requested_room))
        .add_systems(ResetPlay, reset_language);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_table_reads_and_writes_as_a_data_asset() {
        let table = TextTable {
            language: "en".into(),
            texts: vec![TextEntry {
                key: "door.locked".into(),
                text: "Locked, \"really\".".into(),
            }],
        };
        let path = std::env::temp_dir().join(format!("maki-text-{}.text.ron", std::process::id()));
        std::fs::write(&path, maki_assets::data_text(&table)).unwrap();
        assert_eq!(maki_assets::read_data::<TextTable>(&path).unwrap(), table);
        assert_eq!(DataTypes::type_of(&path), Some("text"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_name_in_braces_is_a_variable() {
        let variable = |name: &str| if name == "coins" { 7 } else { 0 };
        assert_eq!(
            fill("You have {coins} coins.", variable),
            "You have 7 coins."
        );
        assert_eq!(fill("{{coins}} {none}", variable), "{coins} 0");
        assert_eq!(fill("{ open, } close", variable), "{ open, } close");
        assert_eq!(fill("no brace", variable), "no brace");
    }
}
