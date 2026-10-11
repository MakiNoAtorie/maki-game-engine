use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use bevy_ecs::resource::Resource;
use serde::{Deserialize, Serialize};

/// Engine settings, read from a TOML file at startup.
// `default` lets an old config file omit fields that were added later.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Resource)]
#[serde(default)]
pub struct Config {
    pub window_width: u32,
    pub window_height: u32,
    /// The path tracer renders at this size; the result is scaled to the window.
    pub render_width: u32,
    pub render_height: u32,
    pub vsync: bool,
    /// glTF file to show. `None` means an empty room.
    pub model_path: Option<PathBuf>,
    /// Rate of the FixedUpdate schedule, in steps per second.
    pub fixed_hz: f64,
    /// Limit of FixedUpdate steps in one frame. A longer frame counts as this many steps.
    pub max_fixed_steps: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            window_width: 1280,
            window_height: 720,
            render_width: 640,
            render_height: 360,
            vsync: true,
            model_path: None,
            fixed_hz: 60.0,
            max_fixed_steps: 8,
        }
    }
}

impl Config {
    pub const DEFAULT_PATH: &str = "config.toml";

    /// Loads the config. If the file does not exist, writes the defaults there and returns them.
    pub fn load_or_create(path: &Path) -> Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => {
                toml::from_str(&text).with_context(|| format!("invalid config {}", path.display()))
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {
                let config = Self::default();
                fs::write(path, toml::to_string_pretty(&config)?)
                    .with_context(|| format!("cannot write default config {}", path.display()))?;
                Ok(config)
            }
            Err(e) => Err(e).with_context(|| format!("cannot read config {}", path.display())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_created_then_read_back() {
        let path = std::env::temp_dir().join(format!("maki-config-{}.toml", std::process::id()));
        let _ = fs::remove_file(&path);

        let created = Config::load_or_create(&path).unwrap();
        assert_eq!(created, Config::default());

        // A partial file keeps defaults for the fields it omits.
        fs::write(&path, "vsync = false\nmodel_path = \"a.glb\"\n").unwrap();
        let loaded = Config::load_or_create(&path).unwrap();
        assert!(!loaded.vsync);
        assert_eq!(loaded.model_path, Some(PathBuf::from("a.glb")));
        assert_eq!(loaded.window_width, Config::default().window_width);

        fs::remove_file(&path).unwrap();
    }
}
