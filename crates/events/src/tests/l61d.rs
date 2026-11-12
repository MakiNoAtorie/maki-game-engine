//! Line of sight: `Physics::line_of_sight`, `ScriptContext::sees`, the condition `Sees`.

use maki_physics::{Physics, QueryFilter};

use super::*;

/// The watcher stands at (-3, -3), away from the player at the origin; positions below
/// are relative to it.
const AT: [f32; 2] = [-3.0, -3.0];

/// A position relative to the watcher, as room text.
fn rel(x: f32, y: f32, z: f32) -> String {
    format!("({}, {y}, {})", x + AT[0], z + AT[1])
}

/// Sets the flag `go` after 0.2 s, when the colliders of the room are in the physics.
const TIMER: &str =
    r#"(name: "timer", pages: [(trigger: Autorun, commands: [Wait(0.2), SetFlag("go", true)])])"#;

/// The watcher (30, facing +Z), the target (31), `wall` objects, and the page of the watcher.
fn sight_room(target_at: [f32; 3], yaw: u32, wall: &str, page: &str) -> String {
    let (watcher, target) = (
        rel(0.0, 0.0, 0.0),
        rel(target_at[0], target_at[1], target_at[2]),
    );
    let objects = format!(
        r#"objects: [
        (id: 30, name: "watcher", pose: (position: {watcher}, rotation: (0, {yaw}, 0)), components: {{"event": (pages: [{page}])}}),
        (id: 31, name: "target", pose: (position: {target}), components: {{"event": (pages: [])}}),
        {wall}"#
    );
    room(TIMER).replacen("objects: [", &objects, 1)
}

fn wall() -> String {
    format!(
        r#"(id: 32, name: "wall", pose: (position: {}), components: {{"prop": (mesh: Cuboid((-2.0, 0.0, -0.1), (2.0, 3.0, 0.1)))}}),"#,
        rel(0.0, 0.0, 1.5)
    )
}
fn sensor() -> String {
    format!(
        r#"(id: 32, name: "fog", pose: (position: {}), components: {{"collider": (shape: Box(half: (2.0, 1.5, 0.1)), sensor: true)}}),"#,
        rel(0.0, 1.5, 1.5)
    )
}

/// The script function: sets the flag `saw` if the watcher sees the target within 5 m
/// and a cone of 90 degrees.
fn look(context: &mut ScriptContext) {
    let mut query = context
        .world
        .query::<(bevy_ecs::entity::Entity, &ObjectId)>();
    let target = query
        .iter(context.world)
        .find(|(_, id)| **id == ObjectId(31))
        .map(|(entity, _)| entity);
    let seen = target.is_some_and(|target| context.sees(target, 5.0, 90.0));
    context.state().set_flag("saw", seen);
}

/// Runs the script function in a room; true if the watcher saw the target.
fn saw(test: &str, target_at: [f32; 3], yaw: u32, wall: &str) -> bool {
    let page = r#"(trigger: Autorun, commands: [CallScript("look")])"#;
    let directory = directory(test);
    std::fs::write(
        directory.join("a.ron"),
        sight_room(target_at, yaw, wall, page),
    )
    .unwrap();
    let mut app = game(&directory);
    add_script_function(&mut app, "look", look);
    let seen = app.run(|mut app| {
        steps(&mut app, 4);
        flag(&app, "saw")
    });
    std::fs::remove_dir_all(directory).unwrap();
    seen
}

#[test]
fn a_script_sees_inside_its_distance_and_cone_and_not_through_a_wall() {
    assert!(saw("sees-near", [0.0, 0.0, 3.0], 0, ""));
    // 6 m: beyond the 5 m.
    assert!(!saw("sees-far", [0.0, 0.0, 6.0], 0, ""));
    // The cone is 90 degrees in all: 40 degrees off the facing is in, 76 is out.
    assert!(saw("sees-in", [2.5, 0.0, 3.0], 0, ""));
    assert!(!saw("sees-out", [4.0, 0.0, 1.0], 0, ""));
    // Turned around: the target is behind.
    assert!(!saw("sees-behind", [0.0, 0.0, 3.0], 180, ""));
    // A wall between the two; a sensor does not block.
    assert!(!saw("sees-wall", [0.0, 0.0, 3.0], 0, &wall()));
    assert!(saw("sees-sensor", [0.0, 0.0, 3.0], 0, &sensor()));
}

#[test]
fn the_condition_sees_holds_in_sight_and_not_behind_a_wall() {
    let page = r#"(conditions: [Flag("go"), Sees(0, 31, 5.0)], trigger: Autorun, commands: [AddVariable("seen", 1)])"#;
    let run = |test: &str, wall: &str, target_at: [f32; 3]| {
        let directory = directory(test);
        let room = sight_room(target_at, 0, wall, page);
        std::fs::write(directory.join("a.ron"), room).unwrap();
        let seen = game(&directory).run(|mut app| {
            steps(&mut app, 40);
            variable(&app, "seen")
        });
        std::fs::remove_dir_all(directory).unwrap();
        seen
    };
    assert_eq!(run("cond-open", "", [0.0, 0.0, 3.0]), 1);
    assert_eq!(run("cond-wall", &wall(), [0.0, 0.0, 3.0]), 0);
    assert_eq!(run("cond-far", "", [0.0, 0.0, 6.0]), 0);
}

#[test]
fn a_line_of_sight_is_blocked_by_a_wall_not_by_a_sensor_nor_by_the_two_ends() {
    let blocked = |test: &str, wall: &str| {
        let directory = directory(test);
        std::fs::write(
            directory.join("a.ron"),
            sight_room([0.0, 0.0, 3.0], 0, wall, ""),
        )
        .unwrap();
        let clear = game(&directory).run(|mut app| {
            steps(&mut app, 4);
            let world = app.world();
            let physics = world.resource::<Physics>();
            let (from, to) = (
                glam::Vec3::new(AT[0], 1.5, AT[1]),
                glam::Vec3::new(AT[0], 1.5, AT[1] + 3.0),
            );
            let mut ids = world
                .try_query::<(bevy_ecs::entity::Entity, &ObjectId)>()
                .unwrap();
            let mut entity = |n| {
                ids.iter(world)
                    .find(|(_, id)| **id == ObjectId(n))
                    .map(|(e, _)| e)
            };
            let plain = physics.line_of_sight(from, to, QueryFilter::default(), None);
            // The wall is a collider too: skipping it as an end makes the way clear.
            let skipped = physics.line_of_sight(from, to, QueryFilter::default(), entity(32));
            let excluded = QueryFilter {
                exclude: entity(32),
                ..QueryFilter::default()
            };
            let by_filter = physics.line_of_sight(from, to, excluded, None);
            // Sensors count when the filter asks.
            let with_sensors = QueryFilter {
                sensors: true,
                ..QueryFilter::default()
            };
            let all = physics.line_of_sight(from, to, with_sensors, None);
            (plain, skipped, by_filter, all)
        });
        std::fs::remove_dir_all(directory).unwrap();
        clear
    };
    assert_eq!(blocked("los-wall", &wall()), (false, true, true, false));
    assert_eq!(blocked("los-sensor", &sensor()), (true, true, true, false));
    assert_eq!(blocked("los-none", ""), (true, true, true, true));
}
