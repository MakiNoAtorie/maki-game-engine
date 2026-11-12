//! Stage 61, ticket C: the player walks on the grid of the room.

use std::time::Duration;

use bevy_ecs::query::With;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Local, Query};
use glam::{Quat, Vec3};
use maki_core::{App, ConfigFile, FixedUpdate, Interpolated};
use maki_events::EventsModule;
use maki_physics::PhysicsModule;
use maki_platform::{Action, PlatformModule, ScriptedActions};
use maki_scene::{Player, RoomRequest, RoomTarget, SceneModule, room_text};

use crate::control::Controller;
use crate::controls::{MOVE_BACK, MOVE_RIGHT};
use crate::{ControlsModule, GridModule, PlayerModule};

/// A wall in cell (5, 2), whose center is (1.5, -1.5) in a room of 8 x 8 m.
const WALL: &str = r#"(name: "wall", pose: (position: (1.5, 0, -1.5)), components: {"collider": (shape: Box(half: (0.5, 1.0, 0.5)))})"#;

/// Center of cell (2, 2): where the player starts.
const START: Vec3 = Vec3::new(-1.5, 0.0, -1.5);

/// Puts the player at `START` once, facing +X.
fn place(mut done: Local<bool>, mut players: Query<&mut Interpolated, With<Player>>) {
    for mut pose in &mut players {
        if !*done {
            pose.current.translation = START;
            pose.current.rotation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
            *done = true;
        }
    }
}

fn app(config: &str) -> App {
    let mut app = App::new(ConfigFile::from_text(&format!(
        "[time]\nfixed_hz = 50.0\n{config}"
    )))
    .unwrap();
    app.add_module(PlatformModule)
        .add_module(maki_assets::AssetsModule)
        .add_module(SceneModule)
        .add_module(PhysicsModule)
        .add_module(EventsModule)
        .add_module(GridModule)
        .add_module(ControlsModule)
        .add_module(PlayerModule::default())
        .add_systems(FixedUpdate, place.before(maki_events::run_events));
    // `run` does the startup (the player is made), then the room loads.
    app.run(|mut app| {
        app.resource_mut::<Controller>().active = true;
        app.resource_mut::<RoomRequest>().0 = Some(RoomTarget {
            path: "l61c.ron".into(),
            spawn: None,
            text: Some(room_text(8.0, 8.0, 3.0, &[WALL])),
        });
        steps(&mut app, 5);
        app
    })
}

fn steps(app: &mut App, count: usize) {
    for _ in 0..count {
        app.advance(Duration::from_millis(20));
    }
}

/// Holds `action` for `count` steps, then lets go and waits for any move to end.
fn hold(app: &mut App, action: Action, count: usize) {
    app.resource_mut::<ScriptedActions>().0.insert(action);
    steps(app, count);
    app.resource_mut::<ScriptedActions>().0.clear();
    steps(app, 100);
}

fn player(app: &App) -> (Vec3, Quat) {
    let world = app.world();
    let mut query = world
        .try_query_filtered::<&Interpolated, With<Player>>()
        .unwrap();
    let pose = query.single(world).unwrap().current;
    (pose.translation, pose.rotation)
}

fn at(app: &App) -> Vec3 {
    player(app).0
}

/// The center of cell (column, row).
fn center(column: f32, row: f32) -> Vec3 {
    Vec3::new(column - 3.5, 0.0, row - 3.5)
}

#[test]
fn a_press_moves_one_cell() {
    let mut app = app("[walk]\nmode = \"Grid\"");
    assert_eq!(at(&app), START);
    hold(&mut app, MOVE_RIGHT, 1);
    assert!(at(&app).distance(center(3.0, 2.0)) < 1e-4, "{:?}", at(&app));
}

#[test]
fn a_hold_goes_on_cell_by_cell() {
    let mut app = app("[walk]\nmode = \"Grid\"");
    // 40 steps at 1.6 m/s are 1.28 m: the second cell starts, and ends after the release.
    hold(&mut app, MOVE_RIGHT, 40);
    assert!(at(&app).distance(center(4.0, 2.0)) < 1e-4, "{:?}", at(&app));
}

#[test]
fn a_press_of_another_direction_turns_first() {
    let mut app = app("[walk]\nmode = \"Grid\"");
    hold(&mut app, MOVE_BACK, 1);
    let (position, rotation) = player(&app);
    assert_eq!(position, START, "turned, not moved");
    // Back is +Z for the default camera.
    assert!((rotation * Vec3::Z).distance(Vec3::Z) < 1e-4);
    hold(&mut app, MOVE_BACK, 1);
    assert!(at(&app).distance(center(2.0, 3.0)) < 1e-4);
}

#[test]
fn a_blocked_cell_does_not_take_the_player() {
    let mut app = app("[walk]\nmode = \"Grid\"");
    hold(&mut app, MOVE_RIGHT, 300);
    assert!(at(&app).distance(center(4.0, 2.0)) < 1e-4, "{:?}", at(&app));
}

#[test]
fn the_free_walk_does_not_snap_to_cells() {
    let mut app = app("");
    hold(&mut app, MOVE_RIGHT, 25);
    let x = at(&app).x;
    // 25 steps at 1.6 m/s: 0.8 m, not a whole cell.
    assert!((x - (START.x + 0.8)).abs() < 0.1, "{x}");
}
