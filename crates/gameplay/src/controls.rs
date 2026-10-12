//! Controls that belong to no single object: the move actions, quit, a log line for
//! each action press, and the debug toggle of the movers.

use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Query, Res, ResMut};
use glam::Vec3;
use maki_core::{App, FixedUpdate, Module, Update};
use maki_events::run_events;
use maki_platform::{Action, ActionState, FixedActionState, Key, QuitRequested, add_bindings};
use maki_scene::{Camera, Mover, move_movers};

/// The four move actions: a direction on the floor. See [`move_direction`].
pub const MOVE_FORWARD: Action = Action("move_forward");
pub const MOVE_BACK: Action = Action("move_back");
pub const MOVE_LEFT: Action = Action("move_left");
pub const MOVE_RIGHT: Action = Action("move_right");
pub const RUN: Action = Action("run");
pub const QUIT: Action = Action("quit");
/// Debug: opens or closes all doors and movers.
pub const TOGGLE_MOVERS: Action = Action("toggle_movers");

/// True: another consumer has the move actions (a debug tool that moves a light), and
/// the player does not walk. The consumer writes it.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MoveCapture(pub bool);

/// The move actions with their keys, quit, and the action log.
pub struct ControlsModule;

impl Module for ControlsModule {
    fn build(&self, app: &mut App) {
        add_bindings(
            app,
            &[
                (Key::W, MOVE_FORWARD, "Move forward"),
                (Key::ArrowUp, MOVE_FORWARD, "Move forward"),
                (Key::S, MOVE_BACK, "Move back"),
                (Key::ArrowDown, MOVE_BACK, "Move back"),
                (Key::A, MOVE_LEFT, "Move left"),
                (Key::ArrowLeft, MOVE_LEFT, "Move left"),
                (Key::D, MOVE_RIGHT, "Move right"),
                (Key::ArrowRight, MOVE_RIGHT, "Move right"),
                (Key::Shift, RUN, "Run"),
                (Key::Escape, QUIT, "Quit"),
            ],
        );
        app.init_resource::<MoveCapture>()
            .add_systems(Update, (quit_on_action, log_action_presses));
    }
}

/// Debug: one key turns each door and mover around.
pub struct MoverToggleModule;

impl Module for MoverToggleModule {
    fn build(&self, app: &mut App) {
        add_bindings(
            app,
            &[(Key::O, TOGGLE_MOVERS, "Open or close all doors (debug)")],
        );
        // Between the events and the movers: the two write the same movers.
        app.add_systems(
            FixedUpdate,
            toggle_movers.after(run_events).before(move_movers),
        );
    }
}

/// The direction on the floor that the move actions ask for, in world space. Length 1,
/// or zero for no move.
///
/// "Forward" is the view direction of the camera along the floor, so the move goes up
/// on the screen. A camera that looks straight down has no such direction: -Z is up
/// on the screen then (see `maki_render::ray_basis`).
pub fn move_direction(actions: &ActionState, camera: Option<&Camera>) -> Vec3 {
    // 1.0, -1.0, or 0.0 from a pair of opposite actions.
    let axis = |positive, negative| {
        f32::from(actions.pressed(positive)) - f32::from(actions.pressed(negative))
    };
    let forward = camera
        .and_then(|camera| {
            (camera.target - camera.position)
                .with_y(0.0)
                .try_normalize()
        })
        .unwrap_or(Vec3::NEG_Z);
    let right = forward.cross(Vec3::Y);
    (forward * axis(MOVE_FORWARD, MOVE_BACK) + right * axis(MOVE_RIGHT, MOVE_LEFT))
        .normalize_or_zero()
}

fn quit_on_action(actions: Res<ActionState>, mut quit: ResMut<QuitRequested>) {
    if actions.just_pressed(QUIT) {
        quit.0 = true;
    }
}

fn log_action_presses(actions: Res<ActionState>) {
    for action in actions.iter_just_pressed() {
        tracing::info!(action = action.0, "action pressed");
    }
}

fn toggle_movers(actions: Res<FixedActionState>, mut movers: Query<&mut Mover>) {
    if actions.just_pressed(TOGGLE_MOVERS) {
        for mut mover in &mut movers {
            mover.toggle();
        }
    }
}
