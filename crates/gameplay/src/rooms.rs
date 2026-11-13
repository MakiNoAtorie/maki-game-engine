//! Which room the game shows at the start, and the debug reload.

use std::path::PathBuf;

use anyhow::Result;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Res, ResMut};
use maki_core::{App, Module, Settings, Startup, Update};
use maki_platform::{Action, ActionState, Key, add_bindings};
use maki_scene::{
    CurrentRoom, ObjectId, RoomRequest, RoomTarget, RoomValidators, load_requested_room,
};
use serde::{Deserialize, Serialize};

/// Debug: reads the file of the current room again.
pub const RELOAD_ROOM: Action = Action("reload_room");

/// The `[rooms]` table of the config file. A key that the file does not have takes
/// the value that the game gave to [`RoomsModule`]: the engine has no default room.
#[derive(Resource, Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RoomSettings {
    /// The room file that the game starts in.
    pub start: Option<PathBuf>,
    /// The spawn point of that room where the player starts.
    pub start_spawn: Option<ObjectId>,
}

impl Settings for RoomSettings {
    const TABLE: &'static str = "rooms";
}

/// Where the game starts: the settings, or else the values of the game.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct StartRoom {
    pub path: PathBuf,
    pub spawn: ObjectId,
}

impl StartRoom {
    pub fn target(&self) -> RoomTarget {
        RoomTarget {
            path: self.path.clone(),
            spawn: Some(self.spawn),
            text: None,
        }
    }
}

/// Asks for the start room, and reloads the current room on the reload action.
/// Needs `maki_scene::SceneModule`.
pub struct RoomsModule {
    /// The start room and its spawn point for a config file that names none.
    pub start_room: PathBuf,
    pub start_spawn: ObjectId,
}

impl Module for RoomsModule {
    fn build(&self, app: &mut App) {
        app.add_settings::<RoomSettings>();
        let settings = app.world().resource::<RoomSettings>().clone();
        add_bindings(
            app,
            &[(Key::F5, RELOAD_ROOM, "Reload the room file (debug)")],
        );
        app.insert_resource(StartRoom {
            path: settings.start.unwrap_or_else(|| self.start_room.clone()),
            spawn: settings.start_spawn.unwrap_or(self.start_spawn),
        })
        .add_systems(Startup, request_start_room)
        .add_systems(Update, reload_on_action.before(load_requested_room));
        add_console_commands(app);
    }
}

/// The console commands of the rooms.
fn add_console_commands(app: &mut App) {
    use maki_core::add_console_command;
    let help = "Goes to a room: its file, from the folder of the room, and a spawn id";
    add_console_command(app, "room", "ROOM_FILE [SPAWN_ID]", help, |world, args| {
        let (file, spawn) = match args {
            [file] => (file, None),
            [file, spawn] => {
                let id: u32 = spawn
                    .parse()
                    .map_err(|_| format!("`{spawn}` is not an id"))?;
                (file, Some(ObjectId(id)))
            }
            _ => return Err("a room file".into()),
        };
        let room = &world.resource::<CurrentRoom>().path;
        let path = room.parent().unwrap_or(std::path::Path::new("")).join(file);
        if !path.is_file() {
            return Err(format!("no room file {}", path.display()));
        }
        let text = format!("room {}", path.display());
        world.resource_mut::<RoomRequest>().0 = Some(RoomTarget {
            path,
            spawn,
            text: None,
        });
        Ok(text)
    });
    add_console_command(app, "reload", "", "Loads the room again", |world, _| {
        let path = world.resource::<CurrentRoom>().path.clone();
        let text = format!("reload {}", path.display());
        world.resource_mut::<RoomRequest>().0 = Some(RoomTarget {
            path,
            spawn: None,
            text: None,
        });
        Ok(text)
    });
}

/// Reads and checks the start room, with the checks of all modules (the events).
/// Call after the last module was added: a game with a bad start room does not
/// start. The error has the file and the line.
pub fn check_start_room(app: &App) -> Result<()> {
    let world = app.world();
    let checks = world.resource::<RoomValidators>();
    maki_scene::read_room(&world.resource::<StartRoom>().path, checks)?;
    Ok(())
}

pub(crate) fn request_start_room(start: Res<StartRoom>, mut request: ResMut<RoomRequest>) {
    request.0 = Some(start.target());
}

/// The player stays where it is.
fn reload_on_action(
    actions: Res<ActionState>,
    room: Res<CurrentRoom>,
    mut request: ResMut<RoomRequest>,
) {
    if actions.just_pressed(RELOAD_ROOM) {
        request.0 = Some(RoomTarget {
            path: room.path.clone(),
            spawn: None,
            text: None,
        });
    }
}
