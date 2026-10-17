//! The walk on the character controller of the physics, in a room of 8 x 8 x 3 m.

use std::time::Duration;

use bevy_ecs::resource::Resource;
use bevy_ecs::world::World;
use maki_core::ConfigFile;
use maki_physics::PhysicsModule;
use maki_scene::{Collider, Hidden, Mover, Name, RoomRequest, RoomTarget, SceneModule, room_text};

use super::*;

/// A crate of 1 m at (2, 0, 2), and a closed door of 1 x 2 m in the wall at -Z (a passage
/// of 1 m behind it) that opens in 0.2 s.
const OBJECTS: [&str; 2] = [
    r#"(name: "crate", pose: (position: (2, 0, 2)), components: {"prop": (mesh: Cube(1.0))})"#,
    r#"(name: "door", components: {"door": (seconds: 0.2), "doorway": (wall: NegZ, offset: 0.0, width: 1.0, height: 2.0, depth: 1.0)})"#,
];

/// Meters in one fixed step of 20 ms, at the speed of the test walker.
const STEP: f32 = 2.0 * 0.02;

/// Changes of a test to the world, made in the next fixed step before the walk.
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
    steps(app, 1);
}

fn steps(app: &mut App, count: u32) {
    for _ in 0..count {
        app.advance(Duration::from_millis(20));
    }
}

/// The room loaded, the walk as `PlayerModule` orders it, and a walker named "walker"
/// at `at`.
fn room_app(at: Vec3) -> App {
    room_app_with(at, |_| {})
}

/// As [`room_app`], with a change of the world before the walker comes.
fn room_app_with(at: Vec3, before: impl FnOnce(&mut World) + Send + Sync + 'static) -> App {
    let mut app = App::new(ConfigFile::from_text("[time]\nfixed_hz = 50.0")).unwrap();
    app.add_module(SceneModule)
        .add_module(PhysicsModule)
        .init_resource::<Changes>()
        .add_systems(
            FixedUpdate,
            (apply_changes, fit_walker_bodies, walk)
                .chain()
                .before(maki_scene::move_movers)
                .before(PhysicsSet::Step),
        );
    app.resource_mut::<RoomRequest>().0 = Some(RoomTarget {
        path: "walk-test.ron".into(),
        spawn: None,
        text: Some(room_text(8.0, 8.0, 3.0, &OBJECTS)),
    });
    steps(&mut app, 3);
    // One step for the physics to see the change.
    change(&mut app, before);
    spawn_walker(&mut app, "walker", at);
    app
}

/// A walker of the default player, at 2 m/s, standing at `at`.
fn spawn_walker(app: &mut App, name: &'static str, at: Vec3) {
    change(app, move |world| {
        let walker = Walker {
            speed: 2.0,
            ..PlayerDef::default().walker(&CAPSULE)
        };
        let pose = Interpolated::at(Transform::from_translation(at));
        world.spawn((Name(name.to_owned()), walker, pose));
    });
}

fn named(app: &App, name: &str) -> Entity {
    let world = app.world();
    let mut query = world.try_query::<(Entity, &Name)>().unwrap();
    let found = query.iter(world).find(|(_, named)| named.0 == name);
    found.expect("in the room").0
}

fn place(app: &App, entity: Entity) -> Vec3 {
    let pose = app.world().get::<Interpolated>(entity).unwrap();
    pose.current.translation
}

/// The walker walks along `direction` for `count` steps, then stands. Returns its place.
fn walk_for(app: &mut App, name: &str, direction: Vec3, count: u32) -> Vec3 {
    let walker = named(app, name);
    let direction = direction.normalize_or_zero();
    change(app, move |world| {
        world.get_mut::<Walker>(walker).unwrap().direction = direction;
    });
    steps(app, count - 1);
    change(app, move |world| {
        world.get_mut::<Walker>(walker).unwrap().direction = Vec3::ZERO;
    });
    place(app, walker)
}

/// The place after a walk of `count` steps from `at` in a new room.
fn walk_from(at: Vec3, direction: Vec3, count: u32) -> Vec3 {
    walk_from_with(at, direction, count, |_| {})
}

/// As [`walk_from`], with a change of the world before the walk.
fn walk_from_with(
    at: Vec3,
    direction: Vec3,
    count: u32,
    before: impl FnOnce(&mut World) + Send + Sync + 'static,
) -> Vec3 {
    let mut app = room_app_with(at, before);
    walk_for(&mut app, "walker", direction, count)
}

/// The capsule stops at the radius and the gap of the controller from a face.
fn stops_at(value: f32, face: f32) -> bool {
    (value - face).abs() < 0.3 + 0.02 && (value - face).abs() > 0.3
}

#[test]
fn the_walk_stops_at_the_walls_and_a_prop_and_slides_along_a_wall_at_an_angle() {
    // The walls at x = 4, x = -4, z = 4. The walk is on the floor.
    let end = walk_from(Vec3::ZERO, Vec3::X, 120);
    assert!(
        stops_at(end.x, 4.0) && end.y == 0.0 && end.z.abs() < 1e-3,
        "{end}"
    );
    assert!(stops_at(walk_from(Vec3::ZERO, Vec3::NEG_X, 120).x, -4.0));
    assert!(stops_at(walk_from(Vec3::ZERO, Vec3::Z, 120).z, 4.0));
    // At 45 degrees into the wall at +X: on along the wall, at the speed along it.
    let end = walk_from(Vec3::new(3.0, 0.0, -2.0), Vec3::new(1.0, 0.0, 1.0), 50);
    assert!(stops_at(end.x, 4.0) && end.y.abs() < 1e-4, "{end}");
    let along = 50.0 * STEP / 2f32.sqrt();
    assert!(
        end.z > -2.0 + along - 0.05 && end.z <= -2.0 + along + 1e-4,
        "{end}"
    );
    // The crate from x = 1.5 to 2.5.
    let end = walk_from(Vec3::new(0.0, 0.0, 2.0), Vec3::X, 60);
    assert!(stops_at(end.x, 1.5) && end.y == 0.0, "{end}");
}

#[test]
fn the_walk_goes_up_a_step_below_the_step_height_and_not_up_one_above_it() {
    // A step of 1 m from x = 1 to 2: the walker stands on it, then comes down behind it.
    let step = |height: f32| {
        move |world: &mut World| {
            let a = Vec3::new(1.0, 0.0, -1.0);
            world.spawn(Collider::new(a, Vec3::new(2.0, height, 1.0)));
        }
    };
    let on = walk_from_with(Vec3::ZERO, Vec3::X, 38, step(0.2));
    assert!(on.x > 1.2 && (on.y - 0.2).abs() < 0.02, "{on}");
    let behind = walk_from_with(Vec3::ZERO, Vec3::X, 75, step(0.2));
    assert!(behind.x > 2.5 && behind.y == 0.0, "{behind}");
    // Above the step height of 0.25 m: a wall.
    let stopped = walk_from_with(Vec3::ZERO, Vec3::X, 38, step(0.3));
    assert!(stopped.x < 0.71 && stopped.y.abs() < 1e-3, "{stopped}");
}

/// No test of a slope below the limit: the ramp of a test is kinematic (the one way to
/// make a slope through the API), and the controller of Rapier 0.36 takes kinematic ground
/// as a platform and stops on it after some cm. A fixed ramp works. A room has no slope
/// now: add the test with a fixed slope collider.
#[test]
fn the_walk_does_not_go_up_a_slope_above_the_max_slope() {
    // A ramp that rises along +X from (1, 0, 0): a thick box, turned around Z.
    let ramp = |degrees: f32| {
        move |world: &mut World| {
            let angle = degrees.to_radians();
            let rotation = Quat::from_rotation_z(angle);
            let half = 3.0;
            let top = rotation * Vec3::Y;
            world.spawn((
                Transform {
                    translation: Vec3::X - top * half,
                    rotation,
                    scale: Vec3::ONE,
                },
                Kinematic {
                    shape: Shape::Box {
                        half: Vec3::new(half, half, 1.0),
                    },
                    center: Vec3::ZERO,
                    sensor: false,
                },
            ));
        }
    };
    // At 60 degrees, from the floor: a wall, also for the step of 0.25 m.
    let stopped = walk_from_with(Vec3::new(-0.5, 0.0, 0.0), Vec3::X, 50, ramp(60.0));
    assert!(stopped.y < 0.01 && stopped.x < 1.0, "{stopped}");
}

#[test]
fn the_walk_goes_through_a_door_only_when_it_is_open_and_the_open_leaf_blocks() {
    let mut app = room_app(Vec3::new(0.0, 0.0, -2.0));
    let door = named(&app, "door");
    let hinge = place(&app, door);
    // Closed: the leaf in the wall at z = -4 stops the walker.
    let closed = walk_for(&mut app, "walker", Vec3::NEG_Z, 60);
    assert!(stops_at(closed.z, -4.0 + 0.03), "{closed}");
    // The leaf opens into the room, through the walker: the walker is not pushed.
    change(&mut app, move |world| {
        world.get_mut::<Mover>(door).unwrap().go_to_b();
    });
    steps(&mut app, 15);
    let walker = named(&app, "walker");
    assert_eq!(place(&app, walker), closed);
    // Open: through the doorway to the end of the passage of 1 m.
    let through = walk_for(&mut app, "walker", Vec3::NEG_Z, 60);
    assert!(
        stops_at(through.z, -5.0) && through.x.abs() < 1e-3,
        "{through}"
    );

    // The open leaf, 0.5 m into the room: a walker along the wall stops at it.
    let from = Vec3::new(hinge.x - 2.0, 1.0, -3.5);
    let filter = QueryFilter::default();
    let physics = app.world().resource::<Physics>();
    let leaf = physics.ray(from, Vec3::X, 4.0, filter).unwrap();
    assert_eq!(leaf.entity, door);
    spawn_walker(&mut app, "second", from.with_y(0.0));
    let end = walk_for(&mut app, "second", Vec3::X, 60);
    assert!(stops_at(end.x, leaf.point.x), "{end} {leaf:?}");
}

#[test]
fn a_hidden_object_does_not_block_and_blocks_again_when_shown() {
    let mut app = room_app(Vec3::new(0.0, 0.0, 2.0));
    let crate_ = named(&app, "crate");
    assert!(stops_at(walk_for(&mut app, "walker", Vec3::X, 60).x, 1.5));
    change(&mut app, move |world| {
        world.entity_mut(crate_).insert(Hidden {});
    });
    // Through the place of the crate to the wall.
    assert!(stops_at(walk_for(&mut app, "walker", Vec3::X, 80).x, 4.0));
    change(&mut app, move |world| {
        world.entity_mut(crate_).remove::<Hidden>();
    });
    // Back: the crate stops the walker at its face at x = 2.5.
    assert!(stops_at(
        walk_for(&mut app, "walker", Vec3::NEG_X, 60).x,
        2.5
    ));
}

#[test]
fn the_walk_pushes_a_dynamic_box_with_the_push_force() {
    let push = |push_force: f32| {
        let mut app = room_app_with(Vec3::new(0.0, 0.0, -1.5), spawn_box);
        let walker = named(&app, "walker");
        change(&mut app, move |world| {
            world.get_mut::<Walker>(walker).unwrap().push_force = push_force;
        });
        let end = walk_for(&mut app, "walker", Vec3::Z, 80);
        (end, place(&app, named(&app, "box")))
    };
    let (walker, pushed) = push(PlayerDef::default().push_force);
    assert!(pushed.z > 1.5 && pushed.x.abs() < 0.05, "{pushed}");
    assert!(walker.z > 0.0, "{walker}");
    // With no force, the walker stops at the box.
    let (walker, still) = push(0.0);
    let at_box = (walker.z + 0.3).abs() < 0.02;
    assert!(at_box && (still.z - 0.5).abs() < 0.01, "{walker} {still}");
}

/// A box of 1 m and 10 kg on the floor, its face at -Z at z = 0.
fn spawn_box(world: &mut World) {
    use maki_physics::{Body, ColliderShape, ObjectCollider};

    let collider = ObjectCollider {
        shape: ColliderShape::Box { half: [0.5; 3] },
        ..ObjectCollider::default()
    };
    let body = Body::Dynamic {
        mass: 10.0,
        damping: 0.0,
    };
    let at = Interpolated::at(Transform::from_translation(Vec3::new(0.0, 0.5, 0.5)));
    world.spawn((Name("box".to_owned()), at, collider, body));
}

#[test]
fn a_jump_of_a_walker_does_not_throw_a_dynamic_box() {
    // At the face of the box, then a jump through it (a transfer, a spawn point).
    let mut app = room_app_with(Vec3::new(0.0, 0.0, -0.31), spawn_box);
    let (walker, box_) = (named(&app, "walker"), named(&app, "box"));
    steps(&mut app, 20);
    let rest = place(&app, box_);
    change(&mut app, move |world| {
        let at = Transform::from_translation(Vec3::new(0.0, 0.0, 2.0));
        world.get_mut::<Interpolated>(walker).unwrap().snap(at);
    });
    steps(&mut app, 10);
    let after = place(&app, box_);
    assert!(after.distance(rest) < 0.01, "{rest} {after}");
}

#[test]
fn a_walker_that_cannot_move_writes_no_pose() {
    #[derive(Resource, Default)]
    struct Writes(u32);

    let mut app = room_app(Vec3::new(3.5, 0.0, 0.0));
    app.init_resource::<Writes>().add_systems(
        FixedUpdate,
        (|mut writes: ResMut<Writes>, poses: Query<(), (Changed<Interpolated>, With<Walker>)>| {
            writes.0 += poses.iter().count() as u32;
        })
        .after(walk),
    );
    let walker = named(&app, "walker");
    // Into the wall at +X: after some steps it stands at the wall.
    change(&mut app, move |world| {
        world.get_mut::<Walker>(walker).unwrap().direction = Vec3::X;
    });
    steps(&mut app, 20);
    app.resource_mut::<Writes>().0 = 0;
    steps(&mut app, 10);
    assert_eq!(app.world().resource::<Writes>().0, 0);
}

#[test]
fn the_step_height_and_the_max_slope_of_a_player_are_checked() {
    let check = |def: PlayerDef| def.check(&CAPSULE);
    assert!(check(PlayerDef::default()).is_ok());
    let steep = PlayerDef {
        max_slope: 91.0,
        ..PlayerDef::default()
    };
    assert!(check(steep).is_err());
    let below = PlayerDef {
        step_height: -0.1,
        ..PlayerDef::default()
    };
    assert!(check(below).is_err());
}
