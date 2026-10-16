//! The project file: `project.toml` in the folder of a project. It has what an
//! engine crate must know of one game and has no constant for.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use maki_assets::FileRef;
use maki_scene::ObjectId;
use serde::Deserialize;

/// The name of the project file in a project folder.
pub const FILE: &str = "project.toml";

/// `project.toml` as it is written. A path is relative to the project folder.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectFile {
    name: String,
    rooms: PathBuf,
    start_room: PathBuf,
    start_spawn: ObjectId,
    save: PathBuf,
    scripts: String,
    /// The prefab file of the player, from `rooms`. No key: a capsule.
    #[serde(default)]
    player: Option<FileRef>,
}

/// A project: a folder with `project.toml`, the assets, and the script crate. The
/// paths are ready to use: they start at the working directory if the project folder
/// is in it, else they are absolute.
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    /// The project folder.
    pub dir: PathBuf,
    /// For the user: the title of the editor window.
    pub name: String,
    /// The directory of the room files: where the editor opens and saves.
    pub rooms: PathBuf,
    /// Where the game starts if the config file names no room: a file of `rooms`,
    /// and a spawn point of it.
    pub start_room: PathBuf,
    pub start_spawn: ObjectId,
    /// The save file of the game.
    pub save: PathBuf,
    /// The cargo package that the editor builds and loads: the script crate of the
    /// project as a DLL.
    pub scripts: String,
    /// The prefab file of the player, from `rooms` (a file name in it starts there,
    /// as in a prefab that a room names). `None`: a capsule.
    pub player: Option<FileRef>,
}

impl Project {
    /// Reads `project.toml` of a project folder.
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join(FILE);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read the project file {}", path.display()))?;
        Self::parse(dir, &text).with_context(|| format!("project file {}", path.display()))
    }

    fn parse(dir: &Path, text: &str) -> Result<Self> {
        let file: ProjectFile = toml::from_str(text)?;
        // As a person names the files: from the working directory, with `/` on each
        // system (a path of the project shows in the log and in the editor).
        let current = std::env::current_dir().unwrap_or_default();
        let dir = dir.strip_prefix(&current).unwrap_or(dir);
        let slashes = |path: PathBuf| PathBuf::from(path.to_string_lossy().replace('\\', "/"));
        let below = |path: &Path| slashes(dir.join(path));
        let rooms = below(&file.rooms);
        Ok(Self {
            name: file.name,
            start_room: below(&file.rooms.join(&file.start_room)),
            rooms,
            start_spawn: file.start_spawn,
            save: below(&file.save),
            scripts: file.scripts,
            player: file.player,
            dir: slashes(dir.to_owned()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_paths_of_a_project_file_start_at_the_project_folder() {
        let text = r#"
            name = "test"
            rooms = "assets/rooms"
            start_room = "a.ron"
            start_spawn = 2
            save = "save.ron"
            scripts = "test-dll"
            player = { path = "p.prefab", id = "9f3a2c0011223344" }
        "#;
        let project = Project::parse(Path::new("game"), text).unwrap();
        assert_eq!(project.rooms, Path::new("game/assets/rooms"));
        assert_eq!(project.start_room, Path::new("game/assets/rooms/a.ron"));
        assert_eq!(project.save, Path::new("game/save.ron"));
        // A project folder in the working directory, by its full path: the same.
        let full = std::env::current_dir().unwrap().join("game");
        assert_eq!(Project::parse(&full, text).unwrap(), project);
        // A key that the engine does not know is an error, with its name.
        let error = Project::parse(Path::new("game"), &format!("{text}\nrom = 1"));
        assert!(format!("{:#}", error.unwrap_err()).contains("rom"));
        // The start spawn by its id; the player prefab by its path and id.
        assert_eq!(project.start_spawn, ObjectId(2));
        let player = project.player.unwrap();
        assert_eq!(player.id, Some(maki_assets::FileId(0x9f3a_2c00_1122_3344)));
        assert_eq!(player.path, Path::new("p.prefab"));
        // A name is no longer a spawn point.
        let named = text.replace("start_spawn = 2", r#"start_spawn = "start""#);
        assert!(Project::parse(Path::new("game"), &named).is_err());
    }
}
