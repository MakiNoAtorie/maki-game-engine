//! Stage selection (debug): the list of the room files on the disk, and the way to go
//! to one of them with a click or a key.
//!
//! A stage is a room file. The list has the files in the directory of the start room,
//! and the rooms that `tools/bench.py` made for its scenarios with a generated scene.

use std::path::{Path, PathBuf};

use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Res, ResMut};
use maki_core::{App, Config, Module, Startup, Update};
use maki_platform::{Action, ActionState};
use maki_scene::{CurrentRoom, RoomRequest, RoomTarget, load_requested_room, parse_room};
use maki_ui::{add_panel, egui};

/// `tools/bench.py` writes `<scenario>/room.ron` here. Relative to the working directory.
const BENCH_DIR: &str = "target/bench";

/// The room files that the game can go to, in the order of the panel.
#[derive(Resource, Default)]
struct Stages(Vec<PathBuf>);

/// Adds the "Stages" panel and the two keys. Needs `UiModule` to show the panel.
pub struct StagesModule;

impl Module for StagesModule {
    fn build(&self, app: &mut App) {
        app.init_resource::<Stages>()
            .add_systems(Startup, scan_stages)
            .add_systems(Update, step_stage.before(load_requested_room));

        add_panel(app, "Stages", true, |ui, world| {
            if ui.button("Scan the disk again").clicked() {
                let found = find_stages(world.resource::<Config>());
                world.resource_mut::<Stages>().0 = found;
            }
            let stages = world.resource::<Stages>().0.clone();
            let current = world.resource::<CurrentRoom>().path.clone();
            let mut clicked = None;
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    for path in &stages {
                        if ui.selectable_label(*path == current, label(path)).clicked() {
                            clicked = Some(path);
                        }
                    }
                });
            if let Some(path) = clicked {
                let target = stage_target(path, world.resource::<Config>());
                world.resource_mut::<RoomRequest>().0 = Some(target);
            }
        });
    }
}

fn scan_stages(config: Res<Config>, mut stages: ResMut<Stages>) {
    stages.0 = find_stages(&config);
    tracing::info!(count = stages.0.len(), "stages found");
}

fn find_stages(config: &Config) -> Vec<PathBuf> {
    // A start room with no directory in its path is in the working directory.
    let rooms = match config.start_room.parent() {
        Some(parent) if parent != Path::new("") => parent,
        _ => Path::new("."),
    };
    let mut found = ron_files(rooms, 0);
    found.extend(ron_files(Path::new(BENCH_DIR), 1));
    found
}

/// The `.ron` files in `dir`, and in its directories down to `depth` levels. Sorted.
/// A directory that cannot be read gives no files.
fn ron_files(dir: &Path, depth: u32) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for path in std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
    {
        if path.is_dir() {
            if depth > 0 {
                found.extend(ron_files(&path, depth - 1));
            }
        } else if path.extension().is_some_and(|extension| extension == "ron") {
            found.push(path);
        }
    }
    found.sort();
    found
}

/// The directory and the file name: `rooms/room_a.ron`, `heavy_1080p/room.ron`.
fn label(path: &Path) -> String {
    let mut parts: Vec<_> = path
        .iter()
        .rev()
        .take(2)
        .map(|part| part.to_string_lossy())
        .collect();
    parts.reverse();
    parts.join("/")
}

/// The request for a stage. The player goes to the start spawn point of the config if
/// the room has it, else to the first spawn point of the room. A room with no spawn
/// point, or a bad file: the player stays (the loader logs the error of a bad file).
fn stage_target(path: &Path, config: &Config) -> RoomTarget {
    let spawns = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| parse_room(&text).ok())
        .map(|room| room.spawns)
        .unwrap_or_default();
    let spawn = if spawns.contains_key(&config.start_spawn) {
        Some(config.start_spawn.clone())
    } else {
        spawns.into_keys().next()
    };
    RoomTarget {
        path: path.to_owned(),
        spawn,
    }
}

/// The index of the stage after (`forward`) or before the current one. The list is a
/// ring. A current room that is not in the list gives the first stage.
fn next_index(current: Option<usize>, count: usize, forward: bool) -> usize {
    match current {
        Some(index) if forward => (index + 1) % count,
        Some(index) => (index + count - 1) % count,
        None => 0,
    }
}

fn step_stage(
    actions: Res<ActionState>,
    config: Res<Config>,
    stages: Res<Stages>,
    room: Res<CurrentRoom>,
    mut request: ResMut<RoomRequest>,
) {
    let forward = actions.just_pressed(Action::NextStage);
    if !forward && !actions.just_pressed(Action::PreviousStage) || stages.0.is_empty() {
        return;
    }
    let current = stages.0.iter().position(|path| *path == room.path);
    let path = &stages.0[next_index(current, stages.0.len(), forward)];
    tracing::info!(path = %path.display(), "stage");
    request.0 = Some(stage_target(path, &config));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scan_finds_ron_files_down_to_the_depth() {
        let dir = std::env::temp_dir().join(format!("maki-stages-{}", std::process::id()));
        let deep = dir.join("scenario").join("deep");
        std::fs::create_dir_all(&deep).unwrap();
        for file in [
            "b.ron",
            "a.ron",
            "notes.txt",
            "scenario/room.ron",
            "scenario/deep/c.ron",
        ] {
            std::fs::write(dir.join(file), "").unwrap();
        }

        assert_eq!(ron_files(&dir, 0), [dir.join("a.ron"), dir.join("b.ron")]);
        assert_eq!(
            ron_files(&dir, 1),
            [
                dir.join("a.ron"),
                dir.join("b.ron"),
                dir.join("scenario/room.ron")
            ]
        );
        assert!(ron_files(&dir.join("none"), 1).is_empty());
        assert_eq!(label(&dir.join("scenario/room.ron")), "scenario/room.ron");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_stage_list_is_a_ring() {
        assert_eq!(next_index(Some(2), 3, true), 0);
        assert_eq!(next_index(Some(0), 3, false), 2);
        assert_eq!(next_index(Some(1), 3, true), 2);
        assert_eq!(next_index(None, 3, false), 0);
    }
}
