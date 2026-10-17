use std::time::Duration;

use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use glam::{Quat, Vec3};
use maki_core::{App, ConfigFile, Interpolated, Transform};
use maki_scene::{Collider, Hidden, Mover, Name, RoomRequest, RoomTarget, SceneModule, room_text};

use super::*;

/// A room of 8 x 8 x 3 m: a crate at (2, 0, 2), a closed door in the wall at -Z that opens in 0.2 s, and
/// a lift that goes from (-2, 0, -2) to (-2, 0, 0) in 0.2 s.
const OBJECTS: [&str; 3] = [
    r#"(name: "crate", pose: (position: (2, 0, 2)), components: {"prop": (mesh: Cube(1.0))})"#,
    r#"(name: "door", components: {"door": (seconds: 0.2), "doorway": (wall: NegZ, offset: 0.0, width: 1.0, height: 2.0, depth: 1.0)})"#,
    r#"(name: "lift", pose: (position: (-2, 0, -2)), components: {"mover": (mesh: Cube(0.5), b: (position: (-2, 0, 0)), seconds: 0.2)})"#,
];

/// Changes of a test to the world, made in the next fixed step before the movers.
#[derive(Resource, Default)]
struct Changes(Vec<Change>);

type Change = Box<dyn FnOnce(&mut World) + Send + Sync>;

fn apply_changes(world: &mut World) {
    for change in std::mem::take(&mut world.resource_mut::<Changes>().0) {
        change(world);
    }
}

/// Makes a change in the next step, and makes the step.
fn change(app: &mut App, change: impl FnOnce(&mut World) + Send + Sync + 'static) {
    app.resource_mut::<Changes>().0.push(Box::new(change));
    step(app);
}

/// The room loaded, and some fixed steps of 20 ms.
fn room_app() -> App {
    let mut app = App::new(ConfigFile::from_text("[time]\nfixed_hz = 50.0")).unwrap();
    app.add_module(SceneModule)
        .add_module(PhysicsModule)
        .init_resource::<Changes>()
        .add_systems(FixedUpdate, apply_changes.before(maki_scene::move_movers));
    app.resource_mut::<RoomRequest>().0 = Some(RoomTarget {
        path: "physics-test.ron".into(),
        spawn: None,
        text: Some(room_text(8.0, 8.0, 3.0, &OBJECTS)),
    });
    for _ in 0..3 {
        step(&mut app);
    }
    app
}

fn step(app: &mut App) {
    app.advance(Duration::from_millis(20));
}

fn named(app: &App, name: &str) -> Entity {
    let world = app.world();
    let mut query = world.try_query::<(Entity, &Name)>().unwrap();
    let found = query.iter(world).find(|(_, named)| named.0 == name);
    found.expect("in the room").0
}

fn physics(app: &App) -> &Physics {
    app.world().resource::<Physics>()
}

fn close(a: Vec3, b: Vec3) -> bool {
    a.distance(b) < 1e-4
}

/// Starts the movers of these entities to B.
fn go_to_b(app: &mut App, entities: [Entity; 2]) {
    change(app, move |world| {
        for entity in entities {
            world.get_mut::<Mover>(entity).unwrap().go_to_b();
        }
    });
}

#[test]
fn a_room_gives_a_fixed_collider_at_each_box_and_a_ray_hits_the_wall() {
    let app = room_app();
    let (crate_, door) = (named(&app, "crate"), named(&app, "door"));
    let world = app.world();
    let mut query = world.try_query::<(Entity, &Collider)>().unwrap();
    let boxes: Vec<(Entity, Collider)> = query
        .iter(world)
        .map(|(entity, collider)| (entity, *collider))
        .collect();
    let physics = physics(&app);
    // Three walls, two beside the doorway, one at its end, the crate, the door.
    assert_eq!(boxes.len(), 8);
    for (entity, collider) in &boxes {
        let handle = physics.links[entity].fixed.expect("a fixed collider");
        let aabb = physics.world.colliders[handle].compute_aabb();
        assert!(close(vec3(aabb.mins), collider.min), "{entity}");
        assert!(close(vec3(aabb.maxs), collider.max), "{entity}");
    }
    let fixed = physics.world.colliders.iter();
    assert_eq!(fixed.filter(|(_, c)| c.parent().is_none()).count(), 8);

    // From the middle of the room to the wall at +X, 4 m away.
    let filter = QueryFilter::default();
    let wall = physics.ray(Vec3::Y, Vec3::X, 10.0, filter).unwrap();
    assert!((wall.distance - 4.0).abs() < 1e-4, "{wall:?}");
    assert!(close(wall.point, Vec3::new(4.0, 1.0, 0.0)) && close(wall.normal, Vec3::NEG_X));
    assert!(boxes.iter().any(|(entity, _)| *entity == wall.entity));
    // The crate, at the face of its box.
    let crate_box = boxes
        .iter()
        .find(|(entity, _)| *entity == crate_)
        .unwrap()
        .1;
    let hit = physics.ray(Vec3::new(0.0, 0.5, 2.0), Vec3::X, 10.0, filter);
    let hit = hit.unwrap();
    assert_eq!(hit.entity, crate_);
    assert!((hit.point.x - crate_box.min.x).abs() < 1e-4);
    // The closed door.
    let hit = physics.ray(Vec3::Y, Vec3::NEG_Z, 10.0, filter);
    assert_eq!(hit.unwrap().entity, door);
    // Too short a ray, and a ray with no direction: no hit.
    assert_eq!(physics.ray(Vec3::Y, Vec3::X, 3.0, filter), None);
    assert_eq!(physics.ray(Vec3::Y, Vec3::ZERO, 10.0, filter), None);
}

/// The pose of the body of a [`Kinematic`] entity, and the pose that it follows.
fn body_and_pose(app: &App, entity: Entity) -> (Transform, Transform) {
    let physics = physics(app);
    let (body, _) = physics.links[&entity].moving.expect("a body");
    let at = physics.world.bodies[body].position();
    let body = Transform {
        translation: vec3(at.translation),
        rotation: Quat::from_array(at.rotation.to_array()),
        scale: Vec3::ONE,
    };
    (
        body,
        app.world().get::<Interpolated>(entity).unwrap().current,
    )
}

#[test]
fn a_door_leaf_and_a_mover_are_kinematic_bodies_that_follow_their_pose() {
    let mut app = room_app();
    let (door, lift) = (named(&app, "door"), named(&app, "lift"));
    // The leaf: the box of its mesh, 1 x 2 m, from the hinge along +X.
    let leaf = *app.world().get::<Kinematic>(door).unwrap();
    assert_eq!(leaf.center, Vec3::new(0.5, 1.0, 0.0));
    let half = Vec3::new(0.5, 1.0, 0.03);
    assert!(matches!(leaf.shape, Shape::Box { half: h } if close(h, half)));
    let (closed, _) = body_and_pose(&app, door);
    go_to_b(&mut app, [door, lift]);
    for at in 0..15 {
        step(&mut app);
        for entity in [door, lift] {
            let (body, pose) = body_and_pose(&app, entity);
            assert!(close(body.translation, pose.translation), "step {at}");
            assert!(
                body.rotation.angle_between(pose.rotation) < 1e-3,
                "step {at}"
            );
        }
    }
    // Open: the leaf turned, the lift is at B, and the blocker of the door is off.
    let (open, _) = body_and_pose(&app, door);
    assert!(open.rotation.angle_between(closed.rotation) > 1.0);
    let lift_at = body_and_pose(&app, lift).0.translation;
    assert!(close(lift_at, Vec3::new(-2.0, 0.0, 0.0)), "{lift_at}");
    let physics = physics(&app);
    assert!(app.world().get::<Collider>(door).is_none());
    let blocker = physics.links[&door].fixed.unwrap();
    assert!(!physics.world.colliders[blocker].is_enabled());
    // The open leaf blocks: a ray along the wall, 0.5 m in the room, hits it.
    let from = closed.translation + Vec3::new(-1.0, 1.0, 0.5);
    let hit = physics.ray(from, Vec3::X, 2.0, QueryFilter::default());
    assert_eq!(hit.map(|hit| hit.entity), Some(door));
}

#[test]
fn a_hidden_object_has_its_collider_off_and_on_again_with_the_same_handle() {
    let mut app = room_app();
    let crate_ = named(&app, "crate");
    let ray = |app: &App| {
        let filter = QueryFilter::default();
        let hit = physics(app).ray(Vec3::new(0.0, 0.5, 2.0), Vec3::X, 10.0, filter);
        hit.unwrap().entity
    };
    let link = physics(&app).links[&crate_];
    assert_eq!(ray(&app), crate_);
    change(&mut app, move |world| {
        world.entity_mut(crate_).insert(Hidden {});
    });
    // The ray goes on to the wall behind the crate.
    assert_ne!(ray(&app), crate_);
    assert_eq!(physics(&app).links[&crate_], link);
    change(&mut app, move |world| {
        world.entity_mut(crate_).remove::<Hidden>();
    });
    assert_eq!(ray(&app), crate_);
    assert_eq!(physics(&app).links[&crate_], link);
}

#[test]
fn two_runs_give_the_same_steps() {
    // The door opens and the lift moves while a character walks into the door.
    let run = || {
        let mut app = room_app();
        let (door, lift) = (named(&app, "door"), named(&app, "lift"));
        go_to_b(&mut app, [door, lift]);
        let shape = Shape::Capsule {
            radius: 0.3,
            half_height: 0.5,
        };
        let settings = CharacterSettings::default();
        let mut at = Vec3::new(0.3, 0.81, -2.5);
        let mut steps = Vec::new();
        for _ in 0..20 {
            step(&mut app);
            let wanted = Vec3::new(0.05, 0.0, -0.1);
            let filter = QueryFilter::default();
            at += physics(&app)
                .move_character(shape, at, wanted, &settings, filter)
                .step;
            let (door, _) = body_and_pose(&app, door);
            let (lift, _) = body_and_pose(&app, lift);
            steps.push((at, door.rotation, lift.translation));
        }
        steps
    };
    assert_eq!(run(), run());
}

#[test]
fn a_character_slides_along_a_wall_and_touches_it() {
    let app = room_app();
    let physics = physics(&app);
    let shape = Shape::Capsule {
        radius: 0.3,
        half_height: 0.5,
    };
    let (settings, filter) = (CharacterSettings::default(), QueryFilter::default());
    // At an angle into the wall at +X: the part along the wall stays.
    let at = Vec3::new(3.0, 0.81, 0.0);
    let moved = physics.move_character(shape, at, Vec3::new(1.0, 0.0, 0.5), &settings, filter);
    let end = at + moved.step;
    assert!(end.x < 3.7 && end.x > 3.6, "{end}");
    assert!(moved.step.z > 0.4, "{moved:?}");
    let wall = physics.ray(Vec3::Y, Vec3::X, 9.0, filter).unwrap().entity;
    assert_eq!(moved.touched, [wall]);
}

#[test]
fn an_overlap_finds_the_crate_and_a_sensor_tells_who_enters_and_leaves() {
    let mut app = room_app();
    let crate_ = named(&app, "crate");
    let ball = Shape::Sphere { radius: 0.2 };
    let filter = QueryFilter::default();
    let in_crate = Vec3::new(2.0, 0.5, 2.0);
    let found = physics(&app).overlap(ball, in_crate, Quat::IDENTITY, filter);
    assert_eq!(found, [crate_]);
    let excluded = QueryFilter {
        exclude: Some(crate_),
        ..filter
    };
    let found = physics(&app).overlap(ball, in_crate, Quat::IDENTITY, excluded);
    assert!(found.is_empty());

    // A sensor of 1 m at (0, 1, 0), and a ball that moves into it and out again.
    change(&mut app, move |world| {
        world.spawn((
            Name("sensor".to_owned()),
            Transform::from_translation(Vec3::Y),
            Kinematic {
                shape: Shape::Box {
                    half: Vec3::splat(0.5),
                },
                center: Vec3::ZERO,
                sensor: true,
            },
        ));
        world.spawn((
            Name("walker".to_owned()),
            Transform::from_translation(Vec3::new(-2.0, 1.0, 0.0)),
            Kinematic {
                shape: ball,
                center: Vec3::ZERO,
                sensor: false,
            },
        ));
    });
    let (sensor, walker) = (named(&app, "sensor"), named(&app, "walker"));
    let mut events = Vec::new();
    for x in [-1.0, 0.0, 0.0, 1.0, 2.0] {
        change(&mut app, move |world| {
            world.get_mut::<Transform>(walker).unwrap().translation.x = x;
        });
        events.extend_from_slice(physics(&app).sensor_events());
    }
    let entered = SensorEvent {
        sensor,
        other: walker,
        entered: true,
    };
    let left = SensorEvent {
        entered: false,
        ..entered
    };
    assert_eq!(events, [entered, left]);
    // A sensor is found only with `sensors`.
    let from = Vec3::new(0.0, 1.0, -0.9);
    assert_eq!(physics(&app).ray(from, Vec3::Z, 0.5, filter), None);
    let with_sensors = QueryFilter {
        sensors: true,
        ..filter
    };
    let hit = physics(&app).ray(from, Vec3::Z, 0.5, with_sensors);
    assert_eq!(hit.map(|hit| hit.entity), Some(sensor));
}
