//! Which room the game shows: the start room of the config, and the debug reload.

use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Res, ResMut};
use maki_core::{App, Config, Module, Startup, Update};
use maki_platform::{Action, ActionState};
use maki_scene::{CurrentRoom, RoomRequest, RoomTarget, load_requested_room};

pub struct RoomsModule;

impl Module for RoomsModule {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, request_start_room)
            .add_systems(Update, reload_on_action.before(load_requested_room));
    }
}

fn request_start_room(config: Res<Config>, mut request: ResMut<RoomRequest>) {
    request.0 = Some(RoomTarget {
        path: config.start_room.clone(),
        spawn: Some(config.start_spawn.clone()),
    });
}

/// Debug: reads the file of the current room again. The player stays where it is.
fn reload_on_action(
    actions: Res<ActionState>,
    room: Res<CurrentRoom>,
    mut request: ResMut<RoomRequest>,
) {
    if actions.just_pressed(Action::ReloadRoom) {
        request.0 = Some(RoomTarget {
            path: room.path.clone(),
            spawn: None,
        });
    }
}
