//! Stage 64, ticket E: the camera object.

use std::time::Duration;

use bevy_ecs::entity::Entity;
use bevy_ecs::query::With;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Commands, ResMut};
use bevy_ecs::world::World;
use glam::Vec3;
use maki_core::{App, ConfigFile, Update};
use maki_scene::{
    ActiveCamera, Camera, Name, RoomRequest, RoomTarget, SceneModule, room_text, write_field,
};
use serde_json::json;

use crate::{CameraModule, FollowCamera};

const A: &str = r#"(id: 1, name: "a", pose: (position: (1, 2, 3), rotation: (0, 90, 0)), components: {"camera": (fov: 30, active: true)})"#;
const B: &str =
    r#"(id: 2, name: "b", pose: (position: (7, 7, 7)), components: {"camera": (fov: 60)})"#;
const OFF: &str =
    r#"(id: 1, name: "a", pose: (position: (1, 2, 3)), components: {"camera": (active: false)})"#;

fn app_with(objects: &[&str]) -> App {
    let mut app = App::new(ConfigFile::from_text("[time]\nfixed_hz = 50.0\n")).unwrap();
    app.add_module(SceneModule)
        .add_module(CameraModule)
        .init_resource::<Switch>()
        .add_systems(Update, flip);
    app.resource_mut::<RoomRequest>().0 = Some(RoomTarget {
        path: "l64e-camera.ron".into(),
        spawn: None,
        text: Some(room_text(8.0, 8.0, 3.0, objects)),
    });
    // `run` does the Startup schedule, which spawns the camera.
    app.run(|mut app| {
        steps(&mut app);
        app
    })
}

/// The value that `flip` writes into `active` of object "a", once.
#[derive(Resource, Default)]
struct Switch(Option<bool>);

// A queued command, not an exclusive system: that one would be ambiguous with all others.
fn flip(mut switch: ResMut<Switch>, mut commands: Commands) {
    let Some(on) = switch.0.take() else {
        return;
    };
    commands.queue(move |world: &mut World| {
        let entity = object(world, "a");
        write_field(world, entity, "camera", "active", &json!(on)).unwrap();
    });
}

fn steps(app: &mut App) {
    for _ in 0..3 {
        app.advance(Duration::from_millis(20));
    }
}

fn view(app: &App) -> Camera {
    let world = app.world();
    *world
        .try_query_filtered::<&Camera, (With<FollowCamera>, With<ActiveCamera>)>()
        .unwrap()
        .single(world)
        .unwrap()
}

fn object(world: &mut World, name: &str) -> Entity {
    world
        .query::<(Entity, &Name)>()
        .iter(world)
        .find(|(_, n)| n.0 == name)
        .unwrap()
        .0
}

#[test]
fn an_active_camera_object_takes_the_pose_and_the_fov() {
    let app = app_with(&[A]);
    let camera = view(&app);
    assert_eq!(camera.position, Vec3::new(1.0, 2.0, 3.0));
    // A turn of 90 degrees around Y: the front, +Z, goes to +X.
    assert!((camera.target - Vec3::new(2.0, 2.0, 3.0)).length() < 1e-4);
    assert_eq!(camera.fov_y_degrees, 30.0);
}

#[test]
fn an_inactive_camera_or_none_leaves_the_follow() {
    let app = app_with(&[OFF]);
    let camera = view(&app);
    assert_ne!(camera.position, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(camera.fov_y_degrees, 45.0);
}

#[test]
fn a_set_field_of_active_switches_the_view() {
    let mut app = app_with(&[OFF]);
    app.resource_mut::<Switch>().0 = Some(true);
    steps(&mut app);
    assert_eq!(view(&app).position, Vec3::new(1.0, 2.0, 3.0));
    app.resource_mut::<Switch>().0 = Some(false);
    steps(&mut app);
    assert_eq!(view(&app).fov_y_degrees, 45.0);
}

#[test]
fn of_two_active_cameras_the_lowest_id_wins() {
    let app = app_with(&[A, B]);
    assert_eq!(view(&app).fov_y_degrees, 30.0);
    // The order in the file does not matter.
    let app = app_with(&[B, A]);
    assert_eq!(view(&app).fov_y_degrees, 30.0);
}
