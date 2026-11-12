//! Stage 61, ticket E: the templates "Chaser" and "Patrol" on the path search and the
//! line of sight.

use std::time::Duration;

use bevy_ecs::entity::Entity;
use bevy_ecs::query::With;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Query, Res};
use glam::Vec3;
use maki_engine::core::{App, ConfigFile, FixedUpdate, Interpolated};
use maki_engine::scene::{Name, Player, RoomRequest, RoomTarget, room_text};
use maki_engine::{events, gameplay, physics, platform, scene};

#[path = "../templates/chaser.rs"]
mod chaser;
#[path = "../templates/patrol.rs"]
mod patrol;

/// A wall from z -3 to 3 in column 4 (x 0 to 1) of a room of 8 x 8 m: the ways around
/// it are the rows at both ends.
const WALL: &str = r#"(name: "wall", pose: (position: (0.5, 0, 0)), components: {"collider": (shape: Box(half: (0.5, 2.0, 3.0)))})"#;

/// Where the player starts: behind the wall, away from the walker.
const SPAWN: &str =
    r#"(id: 9, name: "start", pose: (position: (-3, 0, 3.5)), components: {"spawn": ()})"#;

/// The start of the walker, and the place on the other side of the wall.
const START: Vec3 = Vec3::new(2.5, 0.0, -2.5);

/// Where the test puts the player, in each fixed step.
#[derive(Resource)]
struct PlayerAt(Vec3);

fn put_player(at: Res<PlayerAt>, mut players: Query<&mut Interpolated, With<Player>>) {
    for mut pose in &mut players {
        pose.current.translation = at.0;
        pose.previous.translation = at.0;
    }
}

fn app(add: fn(&mut App), npc: &str) -> App {
    let config = "[time]\nfixed_hz = 50.0\n[assets]\nbackground = false";
    let mut app = App::new(ConfigFile::from_text(config)).unwrap();
    app.add_module(platform::PlatformModule)
        .add_module(maki_engine::assets::AssetsModule)
        .add_module(scene::SceneModule)
        .add_module(physics::PhysicsModule)
        .add_module(events::EventsModule)
        .add_module(gameplay::ControlsModule)
        .add_module(gameplay::PlayerModule::default())
        .add_module(gameplay::GameModule)
        .add_module(gameplay::GridModule)
        .add_module(gameplay::WalkersModule)
        .insert_resource(PlayerAt(Vec3::new(-3.0, 0.0, 3.5)))
        .add_systems(FixedUpdate, put_player.before(events::run_events));
    add(&mut app);
    let npc = format!(
        r#"(id: 5, name: "npc", pose: (position: (2.5, 0, -2.5)), components: {{"walker": (speed: 4.0), "script_name": {npc}}})"#
    );
    app.resource_mut::<RoomRequest>().0 = Some(RoomTarget {
        path: "l61e.ron".into(),
        spawn: Some(maki_engine::scene::ObjectId(9)),
        text: Some(room_text(8.0, 8.0, 3.0, &[WALL, SPAWN, &npc])),
    });
    app
}

fn steps(app: &mut App, count: usize) {
    for _ in 0..count {
        app.advance(Duration::from_millis(20));
    }
}

fn place(app: &App) -> Vec3 {
    let world = app.world();
    let mut query = world.try_query::<(Entity, &Name)>().unwrap();
    let npc = query.iter(world).find(|(_, n)| n.0 == "npc").unwrap().0;
    world.get::<Interpolated>(npc).unwrap().current.translation
}

fn near(a: Vec3, b: Vec3) -> bool {
    Vec3::new(a.x - b.x, 0.0, a.z - b.z).length() < 0.2
}

fn player_at(app: &mut App, at: Vec3) {
    app.resource_mut::<PlayerAt>().0 = at;
}

#[test]
fn the_chaser_follows_searches_gives_up_and_goes_back() {
    app(chaser::add, "(distance: 5.0, give_up: 3.0)").run(|mut app| {
        // Settle: the first steps see the player at the origin, then it gives up.
        steps(&mut app, 400);
        assert!(near(place(&app), START), "{}", place(&app));
        // In view on its side: it follows.
        let seen = Vec3::new(2.5, 0.0, 2.0);
        player_at(&mut app, seen);
        steps(&mut app, 120);
        assert!(place(&app).z > 1.0, "{}", place(&app));
        // The player is gone behind the wall: it goes to the last place and waits.
        player_at(&mut app, Vec3::new(-3.0, 0.0, 1.0));
        steps(&mut app, 60);
        assert!(near(place(&app), seen), "{}", place(&app));
        // After the give-up time it walks back to its start.
        steps(&mut app, 400);
        assert!(near(place(&app), START), "{}", place(&app));
    });
}

/// The script state of the walker, as its `Debug` text: the states are private.
fn state_text<T: bevy_ecs::component::Component + std::fmt::Debug>(app: &App) -> String {
    let world = app.world();
    let mut query = world.try_query::<&T>().unwrap();
    format!("{:?}", query.iter(world).next().unwrap())
}

/// From now on each walk has no path: an empty grid has no cell.
fn no_paths(app: &mut App) {
    *app.resource_mut::<gameplay::WalkGrid>() = gameplay::WalkGrid::default();
}

#[test]
fn the_chaser_searches_at_the_last_place_where_it_saw_the_player() {
    app(chaser::add, "(distance: 5.0, give_up: 3.0)").run(|mut app| {
        steps(&mut app, 400);
        // Seen, then seen less than a cell from the end of its path: no new path.
        player_at(&mut app, Vec3::new(2.5, 0.0, 2.0));
        steps(&mut app, 5);
        let last = Vec3::new(2.5, 0.0, 1.3);
        player_at(&mut app, last);
        steps(&mut app, 5);
        player_at(&mut app, Vec3::new(-3.0, 0.0, 1.0));
        steps(&mut app, 100);
        assert!(near(place(&app), last), "{}", place(&app));
    });
}

#[test]
fn the_chaser_that_gives_up_with_no_path_rests_where_it_stands() {
    app(chaser::add, "(distance: 5.0, give_up: 0.5)").run(|mut app| {
        steps(&mut app, 400);
        no_paths(&mut app);
        player_at(&mut app, Vec3::new(2.5, 0.0, 2.0));
        steps(&mut app, 5);
        player_at(&mut app, Vec3::new(-3.0, 0.0, 1.0));
        steps(&mut app, 100);
        let text = state_text::<chaser::ScriptName>(&app);
        assert!(text.contains("state: Rest"), "{text}");
    });
}

#[test]
fn the_patrol_with_no_path_waits_and_goes_on_to_the_next_point() {
    // The far point is out of the room: no path to it.
    app(patrol::add, "(to: (-20.0, 0.0, 0.0), wait: 0.1)").run(|mut app| {
        let mut targets = std::collections::BTreeSet::new();
        for _ in 0..100 {
            steps(&mut app, 1);
            let text = state_text::<patrol::ScriptName>(&app);
            targets.extend(
                ["target: 0", "target: 1"]
                    .into_iter()
                    .filter(|t| text.contains(t)),
            );
        }
        assert_eq!(targets.len(), 2, "{targets:?}");
    });
}

#[test]
fn the_patrol_walks_its_points_around_the_wall() {
    app(patrol::add, "(to: (-5.0, 0.0, 0.0), wait: 0.1)").run(|mut app| {
        let mut nearest_z = f32::MAX;
        let mut arrived = false;
        for _ in 0..800 {
            steps(&mut app, 1);
            nearest_z = nearest_z.min(place(&app).z);
            arrived |= near(place(&app), START + Vec3::new(-5.0, 0.0, 0.0));
        }
        assert!(arrived, "{}", place(&app));
        assert!(nearest_z < -3.0, "around the end of the wall");
    });
}
