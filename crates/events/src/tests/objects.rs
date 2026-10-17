//! Show, hide, and move of an object by its id.

use glam::Vec3;
use maki_scene::{Collider, Hidden};

use super::*;

/// The test room with a crate (11: a prop with a collider and a light) and a box
/// that the room file hides (13).
fn room_with_crate(events: &str) -> String {
    room(events).replacen(
        "objects: [",
        r#"objects: [(id: 11, name: "crate", pose: (position: (2, 0, 2)), components: {"prop": (mesh: Cube(1.0)), "light": (radius: 0.1, color: (1, 1, 1), intensity: 1.0)}),
        (id: 13, name: "box", pose: (position: (-2, 0, 2)), components: {"prop": (mesh: Cube(1.0)), "hidden": ()}),"#,
        1,
    )
}

/// The entities of an object, and how many of them are hidden.
fn hidden(app: &App, id: u32) -> (usize, usize) {
    let world = app.world();
    let mut query = world.try_query::<(&ObjectId, Has<Hidden>)>().unwrap();
    let of_object: Vec<bool> = query
        .iter(world)
        .filter(|(object, _)| **object == ObjectId(id))
        .map(|(_, hidden)| hidden)
        .collect();
    (
        of_object.len(),
        of_object.iter().filter(|hidden| **hidden).count(),
    )
}

use bevy_ecs::query::Has;

#[test]
fn a_hidden_object_has_no_area_output_and_a_show_brings_it_back() {
    let directory = directory("hide");
    let events = r#"
        (id: 12, name: "plate", area: (center: (2, 0), size: (1, 1)),
            pages: [(trigger: Touch, commands: [AddVariable("plate", 1)])]),
        (name: "hide", pages: [(trigger: Autorun, commands: [Hide(12), Hide(11)])]),
        (name: "show", pages: [(conditions: [Flag("show")], trigger: Autorun,
            commands: [Show(12), Show(11)])])"#;
    std::fs::write(directory.join("a.ron"), room_with_crate(events)).unwrap();
    let mut app = game(&directory);
    // Over the plate and past it in steps 10 to 21, and back in steps 40 to 51.
    app.add_systems(
        FixedUpdate,
        (|time: Res<Time>, mut players: Query<&mut Interpolated, With<Player>>| {
            let tick = time.fixed_tick;
            let step = if (10..22).contains(&tick) {
                0.25
            } else if (40..52).contains(&tick) {
                -0.25
            } else {
                0.0
            };
            players.single_mut().unwrap().current.translation.x += step;
        })
        .before(run_events),
    );
    app.run(|mut app| {
        steps(&mut app, 3);
        // Each entity of the crate (prop and light), and the box of the room file.
        assert_eq!(hidden(&app, 11), (2, 2));
        assert_eq!(hidden(&app, 12), (1, 1));
        assert_eq!(hidden(&app, 13), (1, 1));
        steps(&mut app, 25);
        assert_eq!(app.world().resource::<GameState>().variable("plate"), 0);
        app.resource_mut::<GameState>().set_flag("show", true);
        steps(&mut app, 3);
        assert_eq!(hidden(&app, 11), (2, 0));
        assert_eq!(hidden(&app, 12), (1, 0));
        assert_eq!(hidden(&app, 13), (1, 1));
        steps(&mut app, 25);
        assert_eq!(app.world().resource::<GameState>().variable("plate"), 1);
    });
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_move_ends_at_its_place_and_the_row_waits_for_it() {
    let directory = directory("move");
    let events = r#"(name: "move", pages: [(trigger: Autorun, commands: [
        MoveObject(11, (4.0, 0.0, 2.0), 0.2), SetFlag("there", true), Wait(0.1),
        MoveObject(11, (4.0, 0.0, -1.0), 0.0), SetFlag("back", true)])])"#;
    std::fs::write(directory.join("a.ron"), room_with_crate(events)).unwrap();
    let app = game(&directory);
    // The position of the prop, of the light, and the middle of the collider.
    let places = |app: &App| {
        let world = app.world();
        let mut props = world
            .try_query::<(&Transform, &ObjectId, &Collider)>()
            .unwrap();
        let mut props = props.iter(world).filter(|(_, id, _)| **id == ObjectId(11));
        let (prop, _, collider) = props.next().unwrap();
        let mut lights = world
            .try_query::<(&Interpolated, &ObjectId, &Light)>()
            .unwrap();
        let mut lights = lights.iter(world).filter(|(_, id, _)| **id == ObjectId(11));
        let light = lights.next().unwrap().0.current.translation;
        let middle = (collider.min + collider.max) / 2.0;
        [
            prop.translation,
            light,
            glam::Vec3::new(middle.x, 0.0, middle.z),
        ]
    };
    app.run(|mut app| {
        steps(&mut app, 3);
        let start = places(&app);
        assert!(
            start.iter().all(|place| place.x > 2.0 && place.x < 2.5),
            "{start:?}"
        );
        assert!(!flag(&app, "there"));
        // 0.2 s is 10 steps: the row waits until the crate is there.
        steps(&mut app, 8);
        assert!(!flag(&app, "there"));
        steps(&mut app, 2);
        assert!(flag(&app, "there"));
        let at = |x, z| glam::Vec3::new(x, 0.0, z);
        assert_eq!(places(&app), [at(4.0, 2.0); 3]);
        // After the wait, a time of 0: at once.
        steps(&mut app, 6);
        assert!(flag(&app, "back"));
        assert_eq!(places(&app), [at(4.0, -1.0); 3]);
    });
    std::fs::remove_dir_all(directory).unwrap();
}

/// A script function: casts a ray along X at the crate (the hidden box is not seen) and two overlaps.
fn look(context: &mut ScriptContext) {
    let hit = context
        .ray(Vec3::new(0.0, 0.5, 2.0), Vec3::X, 10.0)
        .unwrap();
    let near = (hit.distance - 1.5).abs() < 1e-3 && hit.normal.x < -0.99;
    let crate_ = context.object_id(hit.entity) == Some(ObjectId(11));
    let ball = maki_physics::Shape::Sphere { radius: 0.2 };
    let found = context.overlap(ball, Vec3::new(2.0, 0.5, 2.0));
    let hidden = context.overlap(ball, Vec3::new(-2.0, 0.5, 2.0));
    let seen = crate_ && near && found.len() == 1 && hidden.is_empty();
    context.state().set_flag("seen", seen);
}

#[test]
fn a_script_casts_a_ray_and_an_overlap() {
    let directory = directory("queries");
    let events =
        r#"(name: "look", pages: [(trigger: Autorun, commands: [Wait(0.3), CallScript("look")])])"#;
    std::fs::write(directory.join("a.ron"), room_with_crate(events)).unwrap();
    // `game` has the physics.
    let mut app = game(&directory);
    add_script_function(&mut app, "look", look);
    app.run(|mut app| {
        steps(&mut app, 40);
        assert!(flag(&app, "seen"));
    });
    std::fs::remove_dir_all(directory).unwrap();
}
