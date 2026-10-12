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

use anyhow::{Context, Result, bail};
use bevy_ecs::resource::Resource;
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
