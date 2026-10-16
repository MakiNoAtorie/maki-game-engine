//! Save and load: one file with the room, the pose of the player, and the flags and
//! variables. The event command `SavePoint` writes it; the start of the game and a
//! debug key read it.
//!
//! The state of a room is not in the file: the room file gives it, and an `Autorun`
//! page with a flag condition sets right what an event changed (a lamp that is off).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use bevy_ecs::query::With;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Query, Res, ResMut};
use bevy_ecs::world::World;
use glam::Quat;
use maki_core::{App, FixedUpdate, Interpolated, Module, ResetPlay, Settings, Startup, Update};
use maki_events::{CommandRegistry, EventRunner, GameState, run_events};
use maki_platform::{Action, ActionState, Key, add_bindings};
use maki_scene::{
    CurrentRoom, Player, RoomRequest, RoomTarget, RoomValidators, load_requested_room, ron_options,
};
use serde::{Deserialize, Serialize};

/// Debug: reads the save file again and goes to its state.
pub const LOAD_GAME: Action = Action("load_game");

/// The `[save]` table of the config file.
#[derive(Resource, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SaveSettings {
    /// False: the game reads and writes no save file (a benchmark).
    pub enabled: bool,
    /// The save file. No key: the file that the game gave to [`SaveModule`].
    pub file: Option<PathBuf>,
}

impl Default for SaveSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            file: None,
        }
    }
}

impl Settings for SaveSettings {
    const TABLE: &'static str = "save";
}

/// The save file of this run. `None`: no file is read or written, and `SavePoint`
/// does nothing.
#[derive(Resource, Debug, Clone, PartialEq, Default)]
pub struct SaveFile(pub Option<PathBuf>);

/// The content of a save file.
// ponytail: no version number. A field with `#[serde(default)]` for a new value;
// a version when a field changes its meaning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaveData {
    /// The room file, as the game names it.
    pub room: PathBuf,
    /// The place of the player in the room.
    pub position: [f32; 3],
    /// The turn of the player, as a quaternion: x, y, z, w.
    pub rotation: [f32; 4],
    pub flags: std::collections::BTreeMap<String, bool>,
    pub variables: std::collections::BTreeMap<String, i32>,
}

/// The event commands of this module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SaveCommand {
    /// Saves the game. The file is written when the event of the command ends, with
    /// the state of that time: a command after this one counts too. It waits for
    /// nothing.
    SavePoint,
}

/// The event whose end writes the save file.
#[derive(Resource, Debug, Default)]
struct SaveRequest(Option<String>);

/// Adds the command `SavePoint`, the load at the start, and the debug load key.
/// Needs `maki_events::EventsModule`, `PlayerModule`, and `RoomsModule`.
pub struct SaveModule {
    /// The save file for a config file that names none.
    pub file: PathBuf,
    /// False: no file is read or written in this run. The command is there, so a
    /// tool reads each room file.
    pub active: bool,
}

impl Module for SaveModule {
    fn build(&self, app: &mut App) {
        app.add_settings::<SaveSettings>();
        let settings = app.world().resource::<SaveSettings>().clone();
        let file = settings.file.unwrap_or_else(|| self.file.clone());
        let file = (self.active && settings.enabled).then_some(file);
        add_bindings(app, &[(Key::F9, LOAD_GAME, "Load the saved game (debug)")]);
        let registry = app.world().resource::<CommandRegistry>();
        registry.add(|_: &SaveCommand, step, world| {
            if world.resource::<SaveFile>().0.is_some() {
                world.resource_mut::<SaveRequest>().0 = Some(step.event.to_owned());
            } else {
                tracing::debug!(
                    event = step.event,
                    "no save file: a save point does nothing"
                );
            }
            true
        });
        registry.add_texts(
            |_: &SaveCommand| Some("Save point".to_owned()),
            |_| Some("Saves the game: the room, the place of the player, the flags and variables. The file is written when the event ends."),
        );
        registry.add_defaults(|_, _| Some(SaveCommand::SavePoint));
        app.insert_resource(SaveFile(file))
            .init_resource::<SaveRequest>()
            .add_systems(
                Startup,
                // After the start room: the room of the save takes its place. After
                // the player: it gets the pose of the save.
                load_at_start
                    .after(crate::rooms::request_start_room)
                    .after(crate::player::spawn_player)
                    // It has the world: these two orders are the ones that count.
                    .ambiguous_with_all(),
            )
            .add_systems(FixedUpdate, save_when_the_event_ends.after(run_events))
            // It has the world (`ResetPlay`): its order to the loader is the one that
            // counts.
            .add_systems(
                Update,
                load_on_action
                    .before(load_requested_room)
                    .ambiguous_with_all(),
            )
            // A save point of a game that was stopped writes nothing.
            .add_systems(ResetPlay, |mut request: ResMut<SaveRequest>| {
                if request.0.is_some() {
                    request.0 = None;
                }
            });
    }
}

/// Runs in `FixedUpdate` after the events: writes the save file in the first step
/// where the event of the save point does not run.
fn save_when_the_event_ends(
    mut request: ResMut<SaveRequest>,
    file: Res<SaveFile>,
    runner: Res<EventRunner>,
    room: Res<CurrentRoom>,
    state: Res<GameState>,
    players: Query<&Interpolated, With<Player>>,
) {
    // No write of the resource in a step with no request.
    let Some(event) = &request.0 else { return };
    if runner
        .running()
        .iter()
        .any(|running| running.name == *event)
    {
        return;
    }
    request.0 = None;
    let (Some(file), Ok(player)) = (&file.0, players.single()) else {
        return;
    };
    let save = SaveData {
        // Forward slashes: the same file on each system.
        room: PathBuf::from(room.path.to_string_lossy().replace('\\', "/")),
        position: player.current.translation.into(),
        rotation: player.current.rotation.into(),
        flags: state.flags.clone(),
        variables: state.variables.clone(),
    };
    match write(file, &save) {
        Ok(()) => tracing::info!(file = %file.display(), room = %save.room.display(), "game saved"),
        Err(error) => tracing::error!("game not saved: {error:#}"),
    }
}

/// Writes a save file. The text goes to a second file first: the old save is whole
/// until the new one is.
pub fn write(file: &Path, save: &SaveData) -> Result<()> {
    let text = ron_options().to_string_pretty(save, ron::ser::PrettyConfig::default())?;
    let new = file.with_extension("new");
    std::fs::write(&new, text).with_context(|| format!("cannot write {}", new.display()))?;
    std::fs::rename(&new, file).with_context(|| format!("cannot write {}", file.display()))
}

/// Reads a save file. `None`: no file.
pub fn read(file: &Path) -> Result<Option<SaveData>> {
    if !file.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(file)?;
    let save = ron_options().from_str(&text);
    Ok(Some(save.with_context(|| file.display().to_string())?))
}

/// Goes to the state of the save file of this run, if there is one. An error: no
/// change.
fn load(world: &mut World) {
    let Some(file) = world.resource::<SaveFile>().0.clone() else {
        return;
    };
    let loaded = read(&file).and_then(|save| match save {
        Some(save) => apply(world, &save).map(|()| Some(save)),
        None => Ok(None),
    });
    match loaded {
        Ok(Some(save)) => {
            tracing::info!(file = %file.display(), room = %save.room.display(), "game loaded")
        }
        Ok(None) => tracing::debug!(file = %file.display(), "no save file"),
        Err(error) => tracing::error!("save file not loaded: {error:#}"),
    }
}

/// Goes to a saved state: the play state of each module goes back to its start
/// (`ResetPlay`), then the flags and variables, the room, and the player. A room
/// file that does not read: an error, and no change.
pub fn apply(world: &mut World, save: &SaveData) -> Result<()> {
    // The room loader changes nothing for a bad file: without this check the game
    // would have the state of the save in the room before.
    maki_scene::read_room(&save.room, world.resource::<RoomValidators>())
        .context("the room of the save file")?;
    world.run_schedule(ResetPlay);
    *world.resource_mut::<GameState>() = GameState {
        flags: save.flags.clone(),
        variables: save.variables.clone(),
    };
    // No spawn point: the player stays where this puts it.
    world.resource_mut::<RoomRequest>().0 = Some(RoomTarget {
        path: save.room.clone(),
        spawn: None,
        text: None,
    });
    let mut players = world.query_filtered::<&mut Interpolated, With<Player>>();
    for mut pose in players.iter_mut(world) {
        let mut at = pose.current;
        at.translation = save.position.into();
        // A file that a person changed: a length of 1 again.
        at.rotation = Quat::from_array(save.rotation).normalize();
        // No blend from the old place.
        pose.snap(at);
    }
    Ok(())
}

fn load_at_start(world: &mut World) {
    load(world);
}

fn load_on_action(world: &mut World) {
    if world.resource::<ActionState>().just_pressed(LOAD_GAME) {
        load(world);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use glam::Vec3;
    use maki_core::ConfigFile;

    use super::*;
    use crate::{PlayerModule, RoomsModule};

    /// An app with the modules of a game, in the room `a.ron` of the directory.
    fn game(directory: &Path) -> App {
        game_with(directory, true)
    }

    /// `active`: the run reads and writes the save file.
    fn game_with(directory: &Path, active: bool) -> App {
        // No loader thread: a test counts frames.
        let mut app = App::new(ConfigFile::from_text("[assets]\nbackground = false")).unwrap();
        app.add_module(maki_platform::PlatformModule)
            .add_module(maki_assets::AssetsModule)
            .add_module(maki_scene::SceneModule)
            .add_module(maki_events::EventsModule)
            .add_module(crate::ControlsModule)
            .add_module(PlayerModule::default())
            .add_module(RoomsModule {
                start_room: directory.join("a.ron"),
                start_spawn: "start".to_owned(),
            })
            .add_module(SaveModule {
                file: directory.join("save.ron"),
                active,
            });
        app
    }

    /// The room, a flag, a variable, and the place of the player, after some frames.
    fn state(app: &mut App) -> (PathBuf, bool, i32, Vec3) {
        for _ in 0..10 {
            app.advance(Duration::from_millis(20));
        }
        let world = app.world();
        let mut players = world
            .try_query_filtered::<&Interpolated, With<Player>>()
            .unwrap();
        let state = world.resource::<GameState>();
        (
            world.resource::<CurrentRoom>().path.clone(),
            state.flag("seen"),
            state.variable("count"),
            players.single(world).unwrap().current.translation,
        )
    }

    #[test]
    fn a_save_point_writes_at_the_end_of_its_event_and_the_next_start_is_there() {
        let directory = std::env::temp_dir().join(format!("maki-save-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        // The event of room A goes to room B, saves, and sets a variable after the
        // save point.
        let room_a = r#"(width: 4, depth: 4, height: 3, objects: [(name: "start", pose: (position: (1.0, 0.0, 1.0)), components: {"spawn": ()}),
            (name: "go", components: {"event": (pages: [(trigger: Autorun, commands: [
                SetFlag("seen", true), Transfer("b.ron", "in"), SavePoint, SetVariable("count", 3),
            ])])}),
        ])"#;
        let room_b = r#"(width: 4, depth: 4, height: 3, objects: [(name: "in", pose: (position: (-1.0, 0.0, 0.5)), components: {"spawn": ()})])"#;
        std::fs::write(directory.join("a.ron"), room_a).unwrap();
        std::fs::write(directory.join("b.ron"), room_b).unwrap();
        let (room_b, at) = (directory.join("b.ron"), Vec3::new(-1.0, 0.0, 0.5));

        // A run with no save file (a tool): the event runs, and nothing is written.
        game_with(&directory, false).run(|mut app| {
            assert_eq!(state(&mut app), (room_b.clone(), true, 3, at));
        });
        assert!(!directory.join("save.ron").exists());

        game(&directory).run(|mut app| {
            assert_eq!(state(&mut app), (room_b.clone(), true, 3, at));
        });
        let save = read(&directory.join("save.ron")).unwrap().unwrap();
        assert_eq!((&save.room, save.position), (&room_b, at.to_array()));
        assert_eq!(save.variables["count"], 3);

        // The next start: room B, with no event of room A.
        std::fs::write(directory.join("a.ron"), "not a room").unwrap();
        game(&directory).run(|mut app| {
            assert_eq!(state(&mut app), (room_b.clone(), true, 3, at));
        });

        // A save file that names a bad room, then one that is no save file: the
        // start room, with no flag.
        let start = (directory.join("a.ron"), false, 0, Vec3::new(1.0, 0.0, 1.0));
        let room_a = r#"(width: 4, depth: 4, height: 3, objects: [(name: "start", pose: (position: (1.0, 0.0, 1.0)), components: {"spawn": ()})])"#;
        std::fs::write(directory.join("a.ron"), room_a).unwrap();
        std::fs::write(&room_b, "not a room").unwrap();
        game(&directory).run(|mut app| assert_eq!(state(&mut app), start));
        std::fs::write(directory.join("save.ron"), "not a save").unwrap();
        game(&directory).run(|mut app| assert_eq!(state(&mut app), start));
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
