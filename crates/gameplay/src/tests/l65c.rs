//! Stage 65, ticket C: the analog walk and the default pad bindings.

use std::time::Duration;

use glam::{IVec2, Vec3};
use maki_core::{App, ConfigFile};
use maki_events::{BACK, CHOICE_DOWN, CHOICE_UP, EventsModule, INTERACT};
use maki_platform::{ActionState, Axis, Key, PlatformModule, RawInput, Stick};

use super::l61c::{START, app_in, at, center, steps};
use crate::controls::{ControlsModule, RUN, move_direction};
use crate::player::grid_input;

fn test_app() -> App {
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

/// The left stick at (x, y).
fn left_stick(x: f32, y: f32) -> [RawInput; 2] {
    [
        RawInput::PadAxis(Stick::Left, Axis::X, x),
        RawInput::PadAxis(Stick::Left, Axis::Y, y),
    ]
}

fn actions(app: &App) -> &ActionState {
    app.world().resource::<ActionState>()
}

#[test]
fn half_a_tilt_has_length_half() {
    let mut app = test_app();
    frame(&mut app, &left_stick(0.0, 0.5));
    // No camera: forward is -Z.
    let d = move_direction(actions(&app), None);
    assert!((d - Vec3::new(0.0, 0.0, -0.5)).length() < 1e-6, "{d}");
}

#[test]
fn diagonals_have_length_one() {
    let mut app = test_app();
    frame(&mut app, &left_stick(1.0, 1.0));
    assert!((move_direction(actions(&app), None).length() - 1.0).abs() < 1e-6);
    let mut app = test_app();
    frame(
        &mut app,
        &[RawInput::KeyDown(Key::W), RawInput::KeyDown(Key::D)],
    );
    let d = move_direction(actions(&app), None);
    assert!(
        (d - Vec3::new(1.0, 0.0, -1.0).normalize()).length() < 1e-6,
        "{d}"
    );
}

#[test]
fn a_key_and_a_tilt_at_once_have_length_at_most_one() {
    let mut app = test_app();
    frame(&mut app, &[RawInput::KeyDown(Key::W)]);
    frame(&mut app, &left_stick(0.5, 0.0));
    let d = move_direction(actions(&app), None);
    assert!(d.length() <= 1.0 + 1e-6, "{d}");
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

/// How far the player walks freely in 25 steps with the stick tilted right.
fn walked(tilt: f32) -> f32 {
    let mut app = app_in("", &[]);
    for event in left_stick(tilt, 0.0) {
        app.write_message(event);
    }
    steps(&mut app, 25);
    at(&app).distance(START)
}

#[test]
fn the_free_walk_at_half_a_tilt_walks_at_half_the_speed() {
    // 0.6 past the dead zone 0.2 is 0.5.
    let (full, half) = (walked(1.0), walked(0.6));
    assert!(full > 0.3, "{full}");
    assert!((half / full - 0.5).abs() < 0.02, "{half} / {full}");
}

#[test]
fn on_the_grid_a_tilt_moves_one_cell_along_the_dominant_axis() {
    let mut app = app_in("[walk]\nmode = \"Grid\"", &[]);
    // Past the dead zone 0.2: (0.625, 0.25). The player faces +X.
    for event in left_stick(0.7, 0.4) {
        app.write_message(event);
    }
    steps(&mut app, 1);
    for event in left_stick(0.0, 0.0) {
        app.write_message(event);
    }
    steps(&mut app, 100);
    assert!(at(&app).distance(center(3.0, 2.0)) < 1e-4, "{:?}", at(&app));
}

#[test]
fn pad_buttons_trigger_actions() {
    let mut app = test_app();
    frame(&mut app, &[RawInput::KeyDown(Key::PadA)]);
    assert!(actions(&app).just_pressed(INTERACT));
    frame(
        &mut app,
        &[RawInput::KeyUp(Key::PadA), RawInput::KeyDown(Key::PadB)],
    );
    assert!(actions(&app).just_pressed(BACK));
    frame(
        &mut app,
        &[RawInput::KeyUp(Key::PadB), RawInput::KeyDown(Key::PadUp)],
    );
    assert!(actions(&app).just_pressed(CHOICE_UP));
    frame(
        &mut app,
        &[
            RawInput::KeyUp(Key::PadUp),
            RawInput::PadAxis(Stick::Left, Axis::Y, -1.0),
        ],
    );
    assert!(actions(&app).just_pressed(CHOICE_DOWN));
    frame(&mut app, &[RawInput::KeyDown(Key::PadX)]);
    assert!(actions(&app).pressed(RUN));
}
