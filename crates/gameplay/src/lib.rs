//! Gameplay that many games of this kind share: a camera that follows the player
//! from above, a character that walks with the move actions, the start room, save
//! and load, and debug controls. A game adds the modules that it wants.
//!
//! Each module has its own settings (a table of the config file) and its own input
//! actions. No module has a value that belongs to one game: no room name, no size of
//! a character.
//!
//! This crate is right of `events` (the modules are ordered to the event interpreter,
//! and the player stops while an event has the control) and must not depend on
//! `gpu`, `render`, or `ui`.

mod camera;
mod control;
mod controls;
mod game;
mod light_control;
mod player;
mod rooms;
mod save;

pub use camera::{CameraModule, CameraSettings, FollowCamera, follow_player};
pub use control::{Controller, give_control};
pub use controls::{
    ControlsModule, MOVE_BACK, MOVE_FORWARD, MOVE_LEFT, MOVE_RIGHT, MoveCapture, MoverToggleModule,
    QUIT, RUN, TOGGLE_MOVERS, move_direction,
};
pub use game::{GameCommand, GameModule, GameRules, PAUSE};
pub use light_control::{
    LIGHT_DOWN, LIGHT_INTENSITY_DOWN, LIGHT_INTENSITY_UP, LIGHT_UP, LightControlModule,
    TOGGLE_LIGHT_CONTROL, clamp_to_room,
};
pub use player::{PlayerDef, PlayerModule, Walker, steer_player, walk};
pub use rooms::{RELOAD_ROOM, RoomSettings, RoomsModule, StartRoom, check_start_room};
pub use save::{LOAD_GAME, SaveCommand, SaveData, SaveFile, SaveModule, SaveSettings};
