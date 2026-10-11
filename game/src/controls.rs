//! Quits on the Quit action and logs each action press.

use bevy_ecs::system::{Res, ResMut};
use maki_core::{App, Module, Update};
use maki_platform::{Action, ActionState, QuitRequested};

pub struct ControlsModule;

impl Module for ControlsModule {
    fn build(&self, app: &mut App) {
        // The two systems only read `ActionState`, so they need no order.
        app.add_systems(Update, (quit_on_action, log_action_presses));
    }
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
