//! Stage 65, ticket C: the analog walk and the default pad bindings.

use std::time::Duration;

use glam::{IVec2, Vec3};
use maki_core::{App, ConfigFile};
use maki_events::{BACK, CHOICE_DOWN, CHOICE_UP, EventsModule, INTERACT};
use maki_platform::{ActionState, Key, PlatformModule, RawInput, Stick};

use crate::controls::{ControlsModule, RUN, move_direction};
use crate::player::grid_input;

fn app() -> App {
    let mut app = App::new(ConfigFile::from_text("[gamepad]\ndead_zone = 0.0")).unwrap();
    app.add_module(PlatformModule)
        .add_module(maki_scene::SceneModule)
        .add_module(maki_physics::PhysicsModule)
        .add_module(ControlsModule)
        .add_module(EventsModule);
    app
}

/// Sends the events and runs one frame.
fn frame(app: &mut App, events: &[RawInput]) {
    for event in events {
        app.write_message(*event);
    }
    app.advance(Duration::from_millis(16));
}

fn actions(app: &App) -> &ActionState {
    app.world().resource::<ActionState>()
}

#[test]
fn half_stick_walks_half_as_fast() {
    let mut app = app();
    frame(&mut app, &[RawInput::PadAxis(Stick::Left, [0.0, 0.5])]);
    let actions = actions(&app);
    // No camera: forward is -Z.
    let d = move_direction(actions, None);
    assert!((d - Vec3::new(0.0, 0.0, -0.5)).length() < 1e-6, "{d}");
}

#[test]
fn diagonals_have_length_one() {
    let mut app = app();
    frame(&mut app, &[RawInput::PadAxis(Stick::Left, [1.0, 1.0])]);
    let stick = actions(&app);
    assert!((move_direction(stick, None).length() - 1.0).abs() < 1e-6);
    let mut app = self::app();
    frame(
        &mut app,
        &[RawInput::KeyDown(Key::W), RawInput::KeyDown(Key::D)],
    );
    let keys = actions(&app);
    let d = move_direction(keys, None);
    assert!(
        (d - Vec3::new(1.0, 0.0, -1.0).normalize()).length() < 1e-6,
        "{d}"
    );
}

#[test]
fn grid_takes_the_dominant_axis_and_ignores_a_small_tilt() {
    assert_eq!(
        grid_input(Vec3::new(0.3, 0.0, -0.9)),
        Some(IVec2::new(0, -1))
    );
    assert_eq!(grid_input(Vec3::new(0.9, 0.0, 0.3)), Some(IVec2::new(1, 0)));
    assert_eq!(grid_input(Vec3::new(0.2, 0.0, -0.2)), None);
}

#[test]
fn pad_buttons_trigger_actions() {
    let mut app = app();
    frame(&mut app, &[RawInput::KeyDown(Key::PadA)]);
    let a = actions(&app);
    assert!(a.just_pressed(INTERACT));
    frame(
        &mut app,
        &[RawInput::KeyUp(Key::PadA), RawInput::KeyDown(Key::PadB)],
    );
    let b = actions(&app);
    assert!(b.just_pressed(BACK));
    frame(
        &mut app,
        &[RawInput::KeyUp(Key::PadB), RawInput::KeyDown(Key::PadUp)],
    );
    let up = actions(&app);
    assert!(up.just_pressed(CHOICE_UP));
    frame(
        &mut app,
        &[
            RawInput::KeyUp(Key::PadUp),
            RawInput::PadAxis(Stick::Left, [0.0, -1.0]),
        ],
    );
    let down = actions(&app);
    assert!(down.just_pressed(CHOICE_DOWN));
    frame(&mut app, &[RawInput::KeyDown(Key::PadX)]);
    let run = actions(&app);
    assert!(run.pressed(RUN));
}
