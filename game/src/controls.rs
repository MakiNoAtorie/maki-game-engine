//! Controls that belong to no single object: quit, who gets the move actions, the
//! debug toggle of the movers, and a log line for each action press.

use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Query, Res, ResMut};
use glam::Vec3;
use maki_core::{App, FixedUpdate, Module, Update};
use maki_events::run_events;
use maki_platform::{Action, ActionState, FixedActionState, QuitRequested};
use maki_scene::{Camera, Mover, move_movers};

/// Who gets the four move actions. The controller of each one reads this.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MoveTarget {
    #[default]
    Player,
    Light,
}

pub struct ControlsModule;

impl Module for ControlsModule {
    fn build(&self, app: &mut App) {
        app.init_resource::<MoveTarget>()
            .add_systems(
                Update,
                (quit_on_action, log_action_presses, toggle_move_target),
            )
            // Between the events and the movers: the two write the same movers.
            .add_systems(
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
    (forward * axis(Action::MoveForward, Action::MoveBack)
        + right * axis(Action::MoveRight, Action::MoveLeft))
    .normalize_or_zero()
}

fn quit_on_action(actions: Res<ActionState>, mut quit: ResMut<QuitRequested>) {
    if actions.just_pressed(Action::Quit) {
        quit.0 = true;
    }
}

fn log_action_presses(actions: Res<ActionState>) {
    for action in actions.iter_just_pressed() {
        tracing::info!(?action, "action pressed");
    }
}

fn toggle_move_target(actions: Res<ActionState>, mut target: ResMut<MoveTarget>) {
    if actions.just_pressed(Action::ToggleLightControl) {
        *target = match *target {
            MoveTarget::Player => MoveTarget::Light,
            MoveTarget::Light => MoveTarget::Player,
        };
        tracing::info!(target = ?*target, "move actions");
    }
}

/// Debug: each door and mover turns around.
fn toggle_movers(actions: Res<FixedActionState>, mut movers: Query<&mut Mover>) {
    if actions.just_pressed(Action::ToggleMovers) {
        for mut mover in &mut movers {
            mover.toggle();
        }
    }
}
