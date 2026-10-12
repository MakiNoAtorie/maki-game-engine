//! The config file: one TOML table for each module.
//!
//! A module declares its settings as a type ([`Settings`]) and adds them with
//! `App::add_settings`. The type becomes a resource with the values of its table, so
//! a system reads `Res<CameraSettings>` and no system reads a value by a key name.
//! This module knows no table and no key of another crate.
//!
//! ```toml
//! [window]
//! width = 1280
//!
//! [camera]
//! follow_time = 0.25
//! ```

use std::fmt;
use std::fs;
use std::io::ErrorKind;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use bevy_ecs::change_detection::DetectChanges;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Local, Res, ResMut};
use serde::de::{DeserializeOwned, IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

/// The settings of one module: one table of the config file.
///
/// Give the type `#[serde(default, deny_unknown_fields)]`: a file can omit a key, and
/// a key with a typing error is an error, not a silent default.
pub trait Settings: Resource + Serialize + DeserializeOwned + Default {
    /// The name of the table: `"camera"` for `[camera]`.
    const TABLE: &'static str;

    /// The limits of the values. `Err`: the reason, with the name of the key.
    fn check(&self) -> Result<(), String> {
        Ok(())
    }
}

/// Rate of the fixed steps. The table of this crate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Resource)]
#[serde(default, deny_unknown_fields)]
pub struct TimeSettings {
    /// Rate of the FixedUpdate schedule, in steps per second.
    pub fixed_hz: f64,
    /// Limit of FixedUpdate steps in one frame. A longer frame counts as this many steps.
    pub max_fixed_steps: u32,
}

impl Default for TimeSettings {
    fn default() -> Self {
        Self {
            fixed_hz: 60.0,
            max_fixed_steps: 8,
        }
    }
}

impl Settings for TimeSettings {
    const TABLE: &'static str = "time";

    fn check(&self) -> Result<(), String> {
        // A bad rate would make the step length infinite or zero.
        if !(self.fixed_hz.is_finite() && self.fixed_hz > 0.0) {
            return Err(format!(
                "fixed_hz must be a positive number, got {}",
                self.fixed_hz
            ));
        }
        if self.max_fixed_steps == 0 {
            return Err("max_fixed_steps must be 1 or more".to_owned());
        }
        Ok(())
    }
}

/// The text of the config file, and what the modules read from it.
#[derive(Resource, Debug, Default)]
pub struct ConfigFile {
    path: PathBuf,
    text: String,
    /// True: there was no file. `finish` writes one.
    missing: bool,
    /// True: the text is of a file on the disk, and `write` writes that file.
    on_disk: bool,
    /// Each table that a module added: its name and its default values as TOML.
    tables: Vec<(&'static str, String)>,
    /// The errors of all tables. `finish` returns them, so the user sees all of them
    /// in one run.
    errors: Vec<String>,
}

impl ConfigFile {
    pub const DEFAULT_PATH: &str = "config.toml";

    /// Reads the file. No file is not an error: each module then gets its defaults,
    /// and `finish` writes them to the path.
    pub fn load(path: &Path) -> Result<Self> {
        let (text, missing) = match fs::read_to_string(path) {
            Ok(text) => (text, false),
            Err(e) if e.kind() == ErrorKind::NotFound => (String::new(), true),
            Err(e) => {
                return Err(e).with_context(|| format!("cannot read config {}", path.display()));
            }
        };
        Ok(Self {
            path: path.to_owned(),
            text,
            missing,
            on_disk: true,
            ..Self::default()
        })
    }

    /// A config from a text, for tests and tools. Nothing is written to the disk.
    pub fn from_text(text: &str) -> Self {
        Self {
            path: PathBuf::from("<text>"),
            text: text.to_owned(),
            ..Self::default()
        }
    }

    /// The settings of one table. A table that the file does not have, and a table
    /// with an error, give the defaults; the error waits for `finish`.
    pub(crate) fn read<S: Settings>(&mut self) -> S {
        let defaults = toml::to_string(&S::default())
            .unwrap_or_else(|error| panic!("settings of [{}]: {error}", S::TABLE));
        self.tables.push((S::TABLE, defaults));

        // The settings are read from the text, not from a parsed table: the error
        // of a bad value then has its line.
        let read = toml::Deserializer::parse(&self.text)
            .and_then(|file| file.deserialize_any(TableOf::<S>(PhantomData)));
        let error = match read {
            Ok(settings) => {
                let settings = settings.unwrap_or_default();
                match settings.check() {
                    Ok(()) => return settings,
                    Err(reason) => reason,
                }
            }
            Err(error) => error.to_string(),
        };
        self.errors
            .push(format!("{}: [{}] {error}", self.path.display(), S::TABLE));
        S::default()
    }

    /// Puts the values of one table into the text, and writes the file if the text
    /// is of a file. The other tables and all comments stay. `Err`: the file was not
    /// changed.
    pub fn write<S: Settings>(&mut self, settings: &S) -> Result<()> {
        let values = toml::to_string(settings)?;
        let text = replace_table(&self.text, S::TABLE, &values)
            .with_context(|| format!("{}: [{}] not written", self.path.display(), S::TABLE))?;
        if text == self.text {
            return Ok(());
        }
        if self.on_disk {
            fs::write(&self.path, &text)
                .with_context(|| format!("cannot write config {}", self.path.display()))?;
        }
        self.text = text;
        Ok(())
    }

    /// The text of the file, with what `write` put into it.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The errors of the tables that were read up to now.
    pub(crate) fn finish_errors(&self) -> Result<()> {
        if !self.errors.is_empty() {
            bail!(
                "{}",
                self.errors.join(
                    "
"
                )
            );
        }
        Ok(())
    }

    /// Call after the last module was added. Returns the errors of all tables. Writes
    /// the defaults of all tables if there was no file.
    pub fn finish(&self) -> Result<()> {
        if !self.errors.is_empty() {
            bail!("{}", self.errors.join("\n"));
        }
        if self.missing {
            let mut text = String::new();
            for (table, defaults) in &self.tables {
                text += &format!("[{table}]\n{defaults}\n");
            }
            fs::write(&self.path, text)
                .with_context(|| format!("cannot write default config {}", self.path.display()))?;
            return Ok(());
        }
        // Each table was read with success, so the text is valid TOML.
        let file: toml::Table = toml::from_str(&self.text)?;
        for (key, value) in &file {
            if !value.is_table() {
                bail!(
                    "{}: the key `{key}` is in no table. A file of the old format \
                     (all keys in one list): convert it with \
                     `python tools/convert_config.py {}`",
                    self.path.display(),
                    self.path.display()
                );
            }
            // Not an error: a build without a module (the editor) reads the same file.
            if !self.tables.iter().any(|(table, _)| table == key) {
                tracing::warn!(path = %self.path.display(), table = key, "no module reads this config table");
            }
        }
        Ok(())
    }
}

/// A change of a settings resource is written this long after the last change: a
/// drag of a value changes it in each frame.
const SAVE_DELAY: Duration = Duration::from_millis(400);

/// Runs in `Update` for a settings type of `App::add_saved_settings`.
// The clock of the wall, not `Time`: a tool pauses the time of the game.
pub(crate) fn save_settings<S: Settings>(
    settings: Res<S>,
    mut file: ResMut<ConfigFile>,
    mut changed: Local<Option<Instant>>,
) {
    // The resource also counts as changed when the module adds it.
    if settings.is_changed() && !settings.is_added() {
        *changed = Some(Instant::now());
    }
    if changed.is_some_and(|at| at.elapsed() >= SAVE_DELAY) {
        *changed = None;
        match file.write(&*settings) {
            Ok(()) => tracing::debug!(table = S::TABLE, "settings written to the config file"),
            Err(error) => tracing::error!("{error:#}"),
        }
    }
}

/// The text of a TOML file with the values of one table replaced. `values`: the
/// table as TOML, one `key = value` line for each key. A key that the table has gets
/// its new value in its line; a new key goes to the end of the table; a table that
/// the file does not have goes to the end of the file. All other lines stay.
///
/// `Err`: the result is not the same file with the new table (a value of the old
/// file over more than one line, a table in the table). Such a file is not written.
// The `toml` crate writes a file with no comments. An editing crate would be a new
// dependency for this one function.
fn replace_table(text: &str, table: &str, values: &str) -> Result<String> {
    let end_of_line = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let is_header = |line: &str| line.trim_start().starts_with('[');
    let header = format!("[{table}]");
    let own_header = |line: &String| {
        let line = line.split('#').next().unwrap_or_default();
        line.trim() == header
    };
    let start = match lines.iter().position(own_header) {
        Some(start) => start + 1,
        None => {
            if lines.last().is_some_and(|line| !line.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push(header.clone());
            lines.len()
        }
    };
    for value in values.lines().filter(|line| !line.trim().is_empty()) {
        let key = value.split('=').next().unwrap_or_default().trim();
        // The lines of the table: up to the next table.
        let length = lines[start..].iter().position(|line| is_header(line));
        let end = start + length.unwrap_or(lines.len() - start);
        let has_key = |line: &String| {
            let (name, rest) = line.split_once('=').unwrap_or_default();
            name.trim() == key && !rest.is_empty()
        };
        match lines[start..end].iter().position(has_key) {
            Some(at) => lines[start + at] = value.to_owned(),
            None => {
                // After the last line of the table that is not empty.
                let used = lines[start..end]
                    .iter()
                    .rposition(|line| !line.trim().is_empty());
                lines.insert(start + used.map_or(0, |used| used + 1), value.to_owned());
            }
        }
    }
    let new = lines.join(end_of_line) + end_of_line;

    // The proof: the new text has the new table, and each other table as before.
    let mut old_file: toml::Table = toml::from_str(text)?;
    let mut new_file: toml::Table = toml::from_str(&new)?;
    let wanted: toml::Table = toml::from_str(values)?;
    ensure!(
        new_file.remove(table) == Some(toml::Value::Table(wanted)),
        "the table has a value that this writer cannot replace"
    );
    old_file.remove(table);
    ensure!(old_file == new_file, "another table would change");
    Ok(new)
}

/// Reads one table of a TOML document as `S` and skips the others.
struct TableOf<S>(PhantomData<S>);

impl<'de, S: Settings> Visitor<'de> for TableOf<S> {
    /// `None`: the document does not have the table.
    type Value = Option<S>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "a TOML document")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut found = None;
        while let Some(key) = map.next_key::<String>()? {
            if key == S::TABLE {
                found = Some(map.next_value::<S>()?);
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Resource)]
    #[serde(default, deny_unknown_fields)]
    struct Lamp {
        watts: f32,
        name: String,
    }

    impl Default for Lamp {
        fn default() -> Self {
            Self {
                watts: 40.0,
                name: "lamp".to_owned(),
            }
        }
    }

    impl Settings for Lamp {
        const TABLE: &'static str = "lamp";

        fn check(&self) -> Result<(), String> {
            if self.watts < 0.0 {
                return Err("watts must not be negative".to_owned());
            }
            Ok(())
        }
    }

    #[test]
    fn a_table_gives_its_keys_and_defaults_for_the_rest() {
        let mut file = ConfigFile::from_text("[time]\nfixed_hz = 50.0\n\n[lamp]\nwatts = 60.0\n");
        assert_eq!(
            file.read::<Lamp>(),
            Lamp {
                watts: 60.0,
                name: "lamp".to_owned()
            }
        );
        assert_eq!(file.read::<TimeSettings>().fixed_hz, 50.0);
        file.finish().unwrap();
        // No table: the defaults.
        let mut file = ConfigFile::from_text("");
        assert_eq!(file.read::<Lamp>(), Lamp::default());
        file.finish().unwrap();
    }

    #[test]
    fn an_error_names_the_table_the_key_and_the_line() {
        let error = |text: &str| {
            let mut file = ConfigFile::from_text(text);
            assert_eq!(file.read::<Lamp>(), Lamp::default());
            file.finish().unwrap_err().to_string()
        };
        // A value of the wrong type.
        let bad_type = error("[time]\nfixed_hz = 50.0\n[lamp]\nwatts = \"many\"\n");
        assert!(bad_type.contains("[lamp]"), "{bad_type}");
        assert!(bad_type.contains("line 4"), "{bad_type}");
        assert!(bad_type.contains("watts"), "{bad_type}");
        // A key that the table does not have.
        let unknown = error("[lamp]\nwats = 60.0\n");
        assert!(unknown.contains("[lamp]"), "{unknown}");
        assert!(unknown.contains("line 2"), "{unknown}");
        assert!(unknown.contains("wats"), "{unknown}");
        // A value out of its limits.
        let limit = error("[lamp]\nwatts = -1.0\n");
        assert!(
            limit.contains("[lamp] watts must not be negative"),
            "{limit}"
        );
        // The old format: keys in no table.
        let mut flat = ConfigFile::from_text("vsync = false\n");
        flat.read::<Lamp>();
        let flat = flat.finish().unwrap_err().to_string();
        assert!(
            flat.contains("`vsync`") && flat.contains("convert_config.py"),
            "{flat}"
        );
    }

    #[test]
    fn a_table_is_written_back_and_the_rest_of_the_file_stays() {
        let path = std::env::temp_dir().join(format!("maki-saved-{}.toml", std::process::id()));
        let old = "# The game.\r\n[time]\r\nfixed_hz = 50.0 # steps\r\n\r\n\
            [lamp]\r\n# How bright.\r\nwatts = 60.0\r\n\r\n[other]\r\nlist = [\r\n  [1, 2],\r\n]\r\n";
        fs::write(&path, old).unwrap();
        let mut file = ConfigFile::load(&path).unwrap();
        let mut lamp: Lamp = file.read();
        lamp.watts = 75.5;
        file.write(&lamp).unwrap();
        // One line changed, and one new line for the key that the file did not have.
        let new = old.replace("watts = 60.0\r\n", "watts = 75.5\r\nname = \"lamp\"\r\n");
        assert_eq!(fs::read_to_string(&path).unwrap(), new);
        // The file reads as the new values.
        let mut again = ConfigFile::load(&path).unwrap();
        assert_eq!(again.read::<Lamp>(), lamp);
        assert_eq!(again.read::<TimeSettings>().fixed_hz, 50.0);
        // The same values: no write.
        fs::remove_file(&path).unwrap();
        file.write(&lamp).unwrap();
        assert!(!path.exists());

        // A table that the file does not have goes to its end.
        let mut file = ConfigFile::from_text("[time]\nfixed_hz = 50.0");
        file.write(&lamp).unwrap();
        let expected = "[time]\nfixed_hz = 50.0\n\n[lamp]\nwatts = 75.5\nname = \"lamp\"\n";
        assert_eq!(file.text(), expected);
        // A value over more than one line is not replaced: no change.
        let mut file = ConfigFile::from_text("[lamp]\nname = \"\"\"a\nb\"\"\"\n");
        assert!(file.write(&lamp).is_err());
        assert_eq!(file.text(), "[lamp]\nname = \"\"\"a\nb\"\"\"\n");
    }

    #[test]
    fn a_saved_settings_resource_is_written_some_time_after_its_last_change() {
        use crate::{App, Update};
        use bevy_ecs::system::ResMut;

        let mut app =
            App::new(ConfigFile::from_text("# Keep me.\n[lamp]\nwatts = 60.0\n")).unwrap();
        app.add_saved_settings::<Lamp>();
        // A drag: the value changes in each of the first 3 frames.
        app.add_systems(Update, |mut lamp: ResMut<Lamp>, mut frame: Local<u32>| {
            *frame += 1;
            if *frame <= 3 {
                lamp.watts += 1.0;
            }
        });
        let text = |app: &App| app.world().resource::<ConfigFile>().text().to_owned();
        for _ in 0..5 {
            app.advance(Duration::from_millis(16));
        }
        assert!(text(&app).contains("watts = 60.0"), "no write in the drag");
        std::thread::sleep(SAVE_DELAY);
        app.advance(Duration::from_millis(16));
        assert_eq!(
            text(&app),
            "# Keep me.\n[lamp]\nwatts = 63.0\nname = \"lamp\"\n"
        );
    }

    #[test]
    fn a_missing_file_is_written_with_the_defaults_of_all_tables() {
        let path = std::env::temp_dir().join(format!("maki-config-{}.toml", std::process::id()));
        let _ = fs::remove_file(&path);

        let mut file = ConfigFile::load(&path).unwrap();
        assert_eq!(file.read::<TimeSettings>(), TimeSettings::default());
        assert_eq!(file.read::<Lamp>(), Lamp::default());
        file.finish().unwrap();

        // The file that was written gives the same values.
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("[time]") && text.contains("[lamp]"), "{text}");
        let mut file = ConfigFile::load(&path).unwrap();
        assert_eq!(file.read::<TimeSettings>(), TimeSettings::default());
        assert_eq!(file.read::<Lamp>(), Lamp::default());
        file.finish().unwrap();

        fs::remove_file(&path).unwrap();
    }
}
