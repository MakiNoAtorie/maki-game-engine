//! Stage 61, ticket B: characters that walk by a path search, `WalkTo`, and the walk
//! of a script.

use std::time::Duration;

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Query, Res};
use glam::Vec3;
use maki_core::{App, ConfigFile, Fields, Interpolated, Update};
use maki_events::{GameState, Script, ScriptContext, WalkGoal, add_script};
use maki_scene::{Mover, Name, RoomRequest, RoomTarget, room_text};
use serde::{Deserialize, Serialize};

use crate::{PathFollow, PlayerModule, WalkersModule};

/// A wall in column 4 (x 0 to 1) of a room of 8 x 8 m, from z -3 to 3: the ways around
/// it are row 0 (a gate, closed) and row 7.
const WALL: &str = r#"(name: "wall", pose: (position: (0.5, 0, 0)), components: {"collider": (shape: Box(half: (0.5, 1.0, 3.0)))})"#;
const GATE: &str = r#"(name: "gate", pose: (position: (0.5, 0.5, -3.5)), components: {"mover": (mesh: Cube(1.0), b: (position: (0.5, 2.5, -3.5)), seconds: 0.1)})"#;

/// The start and the place of the walks: on the two sides of the wall.
const START: Vec3 = Vec3::new(-2.5, 0.0, -2.5);
const PLACE: Vec3 = Vec3::new(2.5, 0.0, -2.5);

/// A walker of id 5 at [`START`], with these commands in an autorun event.
fn walker(commands: &str) -> String {
    format!(
        r#"(id: 5, name: "npc", pose: (position: (-2.5, 0, -2.5)), components: {{"walker": (speed: 4.0), "event": (pages: [(trigger: Autorun, commands: [{commands}])])}})"#
    )
}

fn room_app(objects: &[&str]) -> App {
    let config = "[time]\nfixed_hz = 50.0\n[assets]\nbackground = false";
    let mut app = App::new(ConfigFile::from_text(config)).unwrap();
    app.add_module(maki_platform::PlatformModule)
        .add_module(maki_assets::AssetsModule)
        .add_module(maki_scene::SceneModule)
        .add_module(maki_physics::PhysicsModule)
        .add_module(maki_events::EventsModule)
        .add_module(crate::ControlsModule)
        .add_module(PlayerModule::default())
        .add_module(crate::GameModule)
        .add_module(crate::GridModule)
        .add_module(WalkersModule)
        .init_resource::<GateOrder>()
        .add_systems(Update, order_gates);
    add_script::<Goer>(&mut app, "goer");
    app.resource_mut::<RoomRequest>().0 = Some(RoomTarget {
        path: "l61b-walk.ron".into(),
        spawn: None,
        text: Some(room_text(8.0, 8.0, 3.0, objects)),
    });
    app
}

fn steps(app: &mut App, count: usize) {
    for _ in 0..count {
        app.advance(Duration::from_millis(20));
    }
}

/// `Some(true)`: the gates go open (to B); `Some(false)`: closed.
#[derive(Resource, Default)]
struct GateOrder(Option<bool>);

fn order_gates(order: Res<GateOrder>, mut movers: Query<&mut Mover>) {
    for mut mover in &mut movers {
        match order.0 {
            Some(true) => mover.go_to_b(),
            Some(false) => mover.go_to_a(),
            None => {}
        }
    }
}

fn npc(app: &App) -> Entity {
    let world = app.world();
    let mut query = world.try_query::<(Entity, &Name)>().unwrap();
    query
        .iter(world)
        .find(|(_, name)| name.0 == "npc")
        .expect("the walker")
        .0
}

fn place(app: &App) -> Vec3 {
    let pose = app.world().get::<Interpolated>(npc(app)).unwrap();
    pose.current.translation
}

fn goal(app: &App) -> Option<WalkGoal> {
    app.world().get::<WalkGoal>(npc(app)).copied()
}

fn flag(app: &App, name: &str) -> bool {
    app.world().resource::<GameState>().flag(name)
}

/// Steps until the flag is set; the count of steps. Panics after `most`.
fn steps_until(app: &mut App, name: &str, most: usize) -> usize {
    for count in 0..most {
        if flag(app, name) {
            return count;
        }
        steps(app, 1);
    }
    panic!(
        "no flag `{name}` in {most} steps; the walker is at {}",
        place(app)
    );
}

fn near(a: Vec3, b: Vec3) -> bool {
    Vec3::new(a.x - b.x, 0.0, a.z - b.z).length() < 0.1
}

/// The gate of the stage: the wall between the walker and the place.
#[test]
fn walk_to_goes_around_the_wall_and_waits_until_it_is_there() {
    let npc = walker(r#"WalkTo(0, Place((2.5, 0.0, -2.5))), SetFlag("there", true)"#);
    room_app(&[WALL, GATE, &npc]).run(|mut app| {
        steps(&mut app, 30);
        // On the way, with the row after it waiting.
        assert!(!flag(&app, "there"));
        assert!(matches!(goal(&app), Some(WalkGoal::Walking(_))));
        let mut farthest_z = f32::MIN;
        for _ in 0..600 {
            if flag(&app, "there") {
                break;
            }
            farthest_z = farthest_z.max(place(&app).z);
            steps(&mut app, 1);
        }
        assert!(flag(&app, "there"), "the walker is at {}", place(&app));
        assert!(near(place(&app), PLACE), "{}", place(&app));
        assert!(farthest_z > 3.0, "around the end of the wall in row 7");
        assert_eq!(goal(&app), Some(WalkGoal::Arrived(PLACE)));
    });
}

#[test]
fn walk_to_an_object_goes_to_its_place() {
    let spot =
        r#"(id: 7, name: "spot", pose: (position: (2.5, 0, 2.5)), components: {"area": ()})"#;
    let npc = walker(r#"WalkTo(0, Object(7)), SetFlag("there", true)"#);
    room_app(&[WALL, spot, &npc]).run(|mut app| {
        steps_until(&mut app, "there", 600);
        assert!(
            near(place(&app), Vec3::new(2.5, 0.0, 2.5)),
            "{}",
            place(&app)
        );
    });
}

#[test]
fn walk_to_a_place_with_no_path_warns_and_goes_on() {
    // A place in the wall, and a place in the cell of the closed gate.
    for target in ["(0.5, 0.0, 0.0)", "(0.5, 0.0, -3.5)"] {
        let npc = walker(&format!(
            r#"WalkTo(5, Place({target})), SetFlag("on", true)"#
        ));
        room_app(&[WALL, GATE, &npc]).run(|mut app| {
            let count = steps_until(&mut app, "on", 20);
            assert!(count < 10, "{count} steps");
            assert_eq!(goal(&app), Some(WalkGoal::NoPath));
            assert!(near(place(&app), START));
        });
    }
}

/// Watch point 3: a gate that closes in the way of the walker does not hold it: it
/// stops, asks a new path, and goes the other way.
#[test]
fn a_gate_that_closes_in_the_way_makes_the_walker_find_another_way() {
    let object = walker(r#"Wait(0.3), WalkTo(0, Place((2.5, 0.0, -2.5))), SetFlag("there", true)"#);
    room_app(&[WALL, GATE, &object]).run(|mut app| {
        app.resource_mut::<GateOrder>().0 = Some(true);
        steps(&mut app, 25);
        // Through the open gate, the short way.
        let Some(WalkGoal::Walking(_)) = goal(&app) else {
            panic!("walking: {:?}", goal(&app));
        };
        let gate = Vec3::new(0.5, 0.0, -3.5);
        let path = app.world().get::<PathFollow>(npc(&app)).unwrap();
        assert!(path.points.contains(&gate), "{:?}", path.points);
        // Closed when the walker is 1 m from the start, before the gate.
        while place(&app).distance(START) < 1.0 {
            steps(&mut app, 1);
        }
        assert!(place(&app).x < -0.5, "{}", place(&app));
        app.resource_mut::<GateOrder>().0 = Some(false);
        let mut farthest_z = f32::MIN;
        for _ in 0..800 {
            if flag(&app, "there") {
                break;
            }
            farthest_z = farthest_z.max(place(&app).z);
            steps(&mut app, 1);
        }
        assert!(near(place(&app), PLACE), "{}", place(&app));
        assert!(farthest_z > 3.0, "the long way, in row 7");
    });
}

/// With no other way, the walker stops at the closed gate and the command goes on.
#[test]
fn a_gate_that_closes_with_no_other_way_ends_the_walk() {
    let end = r#"(name: "end", pose: (position: (0.5, 0, 3.5)), components: {"collider": (shape: Box(half: (0.5, 1.0, 0.5)))})"#;
    let object = walker(r#"Wait(0.3), WalkTo(0, Place((2.5, 0.0, -2.5))), SetFlag("there", true)"#);
    room_app(&[WALL, GATE, end, &object]).run(|mut app| {
        app.resource_mut::<GateOrder>().0 = Some(true);
        steps(&mut app, 25);
        while place(&app).distance(START) < 1.0 {
            steps(&mut app, 1);
        }
        app.resource_mut::<GateOrder>().0 = Some(false);
        steps_until(&mut app, "there", 200);
        assert_eq!(goal(&app), Some(WalkGoal::NoPath));
        assert!(place(&app).x < 0.0, "{}", place(&app));
    });
}

/// A script that walks its object to [`PLACE`], stops at the flag `stop`, and writes
/// `arrived()` to the flag `arrived`.
#[derive(Component, Debug, Clone, Default, PartialEq, Serialize, Deserialize, Fields)]
#[serde(default)]
#[fields(default)]
struct Goer {
    started: bool,
}

impl Script for Goer {
    fn step(&mut self, context: &mut ScriptContext) {
        if context.state().flag("stop") {
            context.stop();
        } else if !self.started {
            self.started = true;
            context.walk_to(PLACE);
        } else {
            // Again with the same place: no new search.
            context.walk_to(PLACE);
        }
        let arrived = context.arrived();
        context.state().set_flag("arrived", arrived);
    }
}

const GOER: &str = r#"(id: 5, name: "npc", pose: (position: (-2.5, 0, -2.5)), components: {"walker": (speed: 4.0), "goer": ()})"#;

#[test]
fn a_script_walks_with_walk_to_and_reads_arrived() {
    room_app(&[WALL, GOER]).run(|mut app| {
        steps(&mut app, 10);
        assert!(!flag(&app, "arrived"));
        steps_until(&mut app, "arrived", 600);
        assert!(near(place(&app), PLACE), "{}", place(&app));
    });
}

#[test]
fn a_script_stops_the_walk() {
    room_app(&[WALL, GOER]).run(|mut app| {
        steps(&mut app, 20);
        let walked = place(&app);
        assert!(walked.distance(START) > 0.5, "{walked}");
        app.resource_mut::<GameState>().set_flag("stop", true);
        steps(&mut app, 2);
        let stopped = place(&app);
        steps(&mut app, 20);
        assert_eq!(place(&app), stopped);
        assert_eq!(goal(&app), None);
        assert!(!flag(&app, "arrived"));
    });
}
