//! The systems that copy the entities of `scene` into the world of the physics, and
//! the step.

use std::sync::Mutex;

use bevy_ecs::change_detection::DetectChanges;
use bevy_ecs::entity::{Entities, Entity};
use bevy_ecs::query::{Has, Without};
use bevy_ecs::system::{Commands, Query, Res, ResMut};
use bevy_ecs::world::Ref;
use maki_assets::{Assets, Mesh};
use maki_core::{Interpolated, Time, Transform};
use maki_scene::{Collider, Hidden, MeshInstance, Mover};
use rapier3d::prelude as rp;

use crate::{
    Body, ColliderShape, Kinematic, Link, ObjectCollider, Physics, SensorEvent, Shape, Solid, pose,
    shared_shape, vec3, vector,
};

/// An entity with a [`Kinematic`], the poses that it can follow, and whether it is hidden.
type Followed = (
    Entity,
    Ref<'static, Kinematic>,
    Option<&'static Interpolated>,
    Option<&'static Transform>,
    Has<Hidden>,
);

/// An entity with an [`ObjectCollider`], its body, its poses, its mesh, and whether it
/// is hidden.
type Solids = (
    Entity,
    Ref<'static, ObjectCollider>,
    Option<Ref<'static, Body>>,
    Option<&'static Interpolated>,
    Option<&'static Transform>,
    Option<&'static MeshInstance>,
    Has<Hidden>,
);

/// A mover with no shape yet.
type Unshaped = (Without<Kinematic>, Without<ObjectCollider>);

/// The box around a mesh: its smallest and its largest corner.
fn bounds(mesh: &Mesh) -> (glam::Vec3, glam::Vec3) {
    mesh.positions().iter().fold(
        (glam::Vec3::splat(f32::MAX), glam::Vec3::splat(f32::MIN)),
        |(min, max), position| (min.min(*position), max.max(*position)),
    )
}

/// Gives each mover (a door leaf too) a [`Kinematic`] box: the box of its mesh, at the
/// scale of its pose `a`. A mover with an [`ObjectCollider`] has that one.
pub(crate) fn add_mover_shapes(
    mut commands: Commands,
    meshes: Res<Assets<Mesh>>,
    movers: Query<(Entity, &MeshInstance, &Mover), Unshaped>,
) {
    for (entity, mesh, mover) in &movers {
        let Some(mesh) = meshes.get(mesh.0) else {
            continue;
        };
        let (min, max) = bounds(mesh);
        let scale = mover.a.scale;
        commands.entity(entity).insert(Kinematic {
            shape: Shape::Box {
                half: (max - min) * 0.5 * scale,
            },
            center: (min + max) * 0.5 * scale,
            sensor: false,
        });
    }
}

/// The shape and its center in the space of the pose. `None`: [`ColliderShape::FromMesh`]
/// of an entity with no mesh, or with a mesh that is not loaded.
fn solid_shape(
    shape: ColliderShape,
    mesh: Option<&MeshInstance>,
    meshes: &Assets<Mesh>,
    scale: glam::Vec3,
) -> Option<(Shape, glam::Vec3)> {
    let shape = match shape {
        ColliderShape::FromMesh => {
            let (min, max) = bounds(meshes.get(mesh?.0)?);
            let half = (max - min) * 0.5 * scale;
            return Some((Shape::Box { half }, (min + max) * 0.5 * scale));
        }
        ColliderShape::Box { half } => Shape::Box { half: half.into() },
        ColliderShape::Sphere { radius } => Shape::Sphere { radius },
        ColliderShape::Capsule {
            radius,
            half_height,
        } => Shape::Capsule {
            radius,
            half_height,
        },
    };
    Some((shape, glam::Vec3::ZERO))
}

/// Makes the world of the physics like the entities: a new box or body for a new
/// entity, a changed one where its component changed, and a removed one where its
/// entity is gone. A `Hidden` entity, and an entity whose `Collider` or `Kinematic`
/// went away, keep their handles with the colliders off. An entity with an
/// [`ObjectCollider`] has no box: the object collider takes its place.
pub(crate) fn sync_world(
    mut physics: ResMut<Physics>,
    entities: &Entities,
    meshes: Res<Assets<Mesh>>,
    boxes: Query<(Entity, Ref<Collider>, Has<Hidden>), Without<ObjectCollider>>,
    moving: Query<Followed>,
    solids: Query<Solids>,
) {
    let Physics { world, links, .. } = &mut *physics;

    // Gone entities and gone components first: their handles are free for the new ones.
    links.retain(|&entity, link| {
        let alive = entities.contains(entity);
        if let Some(handle) = link.fixed
            && !boxes.contains(entity)
        {
            if alive {
                world.colliders[handle].set_enabled(false);
            } else {
                world.remove_collider(handle);
                link.fixed = None;
            }
        }
        if let Some((body, _)) = link.moving
            && !moving.contains(entity)
        {
            if alive {
                world.bodies[body].set_enabled(false);
            } else {
                world.remove_body_with_colliders(body, true);
                link.moving = None;
            }
        }
        if let Some(solid) = link.solid
            && !solids.contains(entity)
        {
            if alive {
                world.bodies[solid.body].set_enabled(false);
            } else {
                world.remove_body_with_colliders(solid.body, true);
                link.solid = None;
            }
        }
        *link != Link::default()
    });

    for (entity, collider, body, interpolated, transform, mesh, hidden) in &solids {
        let link = links.entry(entity).or_default();
        let at = interpolated
            .map(|pose| pose.current)
            .or(transform.copied())
            .unwrap_or(Transform::IDENTITY);
        let changed = collider.is_changed() || body.as_ref().is_some_and(|body| body.is_changed());
        if let Some(solid) = link.solid
            && changed
        {
            // A new shape or kind: a new body, at the pose of now.
            world.remove_body_with_colliders(solid.body, true);
            link.solid = None;
        }
        let kind = body.map_or(Body::Fixed, |body| *body);
        let Some(solid) = link.solid else {
            let scale = at.scale;
            let Some((shape, center)) = solid_shape(collider.shape, mesh, &meshes, scale) else {
                continue;
            };
            let builder = match kind {
                Body::Fixed => rp::RigidBodyBuilder::fixed(),
                Body::Kinematic => rp::RigidBodyBuilder::kinematic_position_based(),
                Body::Dynamic { damping, .. } => rp::RigidBodyBuilder::dynamic()
                    .linear_damping(damping)
                    .angular_damping(damping),
            };
            let user_data = entity.to_bits().into();
            let body = builder
                .pose(pose(at.translation, at.rotation))
                .user_data(user_data)
                .enabled(!hidden);
            let mut new = rp::ColliderBuilder::new(shared_shape(shape))
                .translation(vector(center))
                .friction(collider.friction)
                .sensor(collider.sensor)
                .user_data(user_data)
                .enabled(!hidden);
            if let Body::Dynamic { mass, .. } = kind {
                // Rapier needs a mass above zero.
                new = new.mass(mass.max(0.01));
            }
            let (body, collider) = world.insert(body, new);
            link.solid = Some(Solid {
                body,
                collider,
                dynamic: matches!(kind, Body::Dynamic { .. }),
                start: at,
            });
            continue;
        };
        world.colliders[solid.collider].set_enabled(!hidden);
        let rigid = &mut world.bodies[solid.body];
        rigid.set_enabled(!hidden);
        let now = rigid.position();
        let moved = vec3(now.translation) != at.translation
            || glam::Quat::from_array(now.rotation.to_array()) != at.rotation;
        match kind {
            Body::Kinematic => rigid.set_next_kinematic_position(pose(at.translation, at.rotation)),
            // A pose other than that of the last step: a command, a script, or `ResetPlay` moved it.
            _ if moved => {
                rigid.set_position(pose(at.translation, at.rotation), true);
                rigid.set_linvel(rp::Vector::ZERO, true);
                rigid.set_angvel(rp::Vector::ZERO, true);
            }
            _ => {}
        }
    }

    for (entity, collider, hidden) in &boxes {
        let link = links.entry(entity).or_default();
        let center = vector((collider.min + collider.max) * 0.5);
        let half = (collider.max - collider.min) * 0.5;
        match link.fixed {
            None => {
                let new = rp::ColliderBuilder::cuboid(half.x, half.y, half.z)
                    .translation(center)
                    .user_data(entity.to_bits().into())
                    .enabled(!hidden);
                link.fixed = Some(world.insert_collider(new, None));
            }
            Some(handle) => {
                let fixed = &mut world.colliders[handle];
                if collider.is_changed() {
                    fixed.set_shape(rp::SharedShape::cuboid(half.x, half.y, half.z));
                    fixed.set_translation(center);
                }
                fixed.set_enabled(!hidden);
            }
        }
    }

    for (entity, kinematic, interpolated, transform, hidden) in &moving {
        let link = links.entry(entity).or_default();
        let at = interpolated
            .map(|pose| pose.current)
            .or(transform.copied())
            .unwrap_or(Transform::IDENTITY);
        let at = pose(at.translation, at.rotation);
        match link.moving {
            None => {
                let user_data = entity.to_bits().into();
                let body = rp::RigidBodyBuilder::kinematic_position_based()
                    .pose(at)
                    .user_data(user_data);
                let mut collider = rp::ColliderBuilder::new(shared_shape(kinematic.shape))
                    .translation(vector(kinematic.center))
                    .user_data(user_data)
                    .sensor(kinematic.sensor)
                    .enabled(!hidden);
                if kinematic.sensor {
                    // A sensor also sees fixed and kinematic colliders (the player).
                    collider = collider
                        .active_events(rp::ActiveEvents::COLLISION_EVENTS)
                        .active_collision_types(rp::ActiveCollisionTypes::all());
                }
                link.moving = Some(world.insert(body, collider));
            }
            Some((body, handle)) => {
                world.bodies[body].set_enabled(true);
                world.bodies[body].set_next_kinematic_position(at);
                let collider = &mut world.colliders[handle];
                if kinematic.is_changed() {
                    collider.set_shape(shared_shape(kinematic.shape));
                    collider.set_translation_wrt_parent(vector(kinematic.center));
                    collider.set_sensor(kinematic.sensor);
                }
                collider.set_enabled(!hidden);
            }
        }
    }
}

/// One step of the world, of the fixed time step. Keeps the sensor events of the step.
pub(crate) fn step(mut physics: ResMut<Physics>, time: Res<Time>) {
    let physics = &mut *physics;
    physics.world.integration_parameters.dt = time.fixed_delta.as_secs_f32();
    let events = Collect::default();
    physics.world.step_with_events(&(), &events);
    let colliders = &physics.world.colliders;
    physics.sensor_events.clear();
    for event in events.0.into_inner().expect("one thread") {
        let (a, b, entered) = match event {
            rp::CollisionEvent::Started(a, b, _) => (a, b, true),
            rp::CollisionEvent::Stopped(a, b, _) => (a, b, false),
        };
        // The floor has no entity: a sensor on it tells nothing.
        if a == physics.floor || b == physics.floor {
            continue;
        }
        // A removed collider has no entity any more: its room is gone.
        let (Some(a), Some(b)) = (colliders.get(a), colliders.get(b)) else {
            continue;
        };
        let (sensor, other) = if a.is_sensor() { (a, b) } else { (b, a) };
        physics.sensor_events.push(SensorEvent {
            sensor: crate::owner(sensor),
            other: crate::owner(other),
            entered,
        });
    }
}

/// Gives the pose of each dynamic body that moved to its entity: to `Interpolated::current`
/// if the entity has it, else to its `Transform`. The scale stays.
pub(crate) fn write_poses(
    physics: Res<Physics>,
    mut poses: Query<(Option<&mut Interpolated>, Option<&mut Transform>)>,
) {
    for (entity, link) in &physics.links {
        let Some(solid) = link.solid.filter(|solid| solid.dynamic) else {
            continue;
        };
        let body = &physics.world.bodies[solid.body];
        if body.is_sleeping() || !body.is_enabled() {
            continue;
        }
        let Ok((interpolated, transform)) = poses.get_mut(*entity) else {
            continue;
        };
        let at = body.position();
        let place = |pose: &mut Transform| {
            pose.translation = vec3(at.translation);
            pose.rotation = glam::Quat::from_array(at.rotation.to_array());
        };
        match (interpolated, transform) {
            (Some(mut interpolated), _) => place(&mut interpolated.current),
            (None, Some(mut transform)) => place(&mut transform),
            (None, None) => {}
        }
    }
}

/// Runs in `ResetPlay`: each dynamic body goes back to the pose that it had when it was
/// made (its place in the room file). The next step moves the body there.
pub(crate) fn restart_bodies(physics: Res<Physics>, mut poses: Query<&mut Interpolated>) {
    for (entity, link) in &physics.links {
        if let Some(solid) = link.solid.filter(|solid| solid.dynamic)
            && let Ok(mut pose) = poses.get_mut(*entity)
        {
            pose.snap(solid.start);
        }
    }
}

/// Keeps the collision events of a step. Rapier asks for `Sync`: hence the `Mutex`.
#[derive(Default)]
struct Collect(Mutex<Vec<rp::CollisionEvent>>);

impl rp::EventHandler for Collect {
    fn handle_collision_event(
        &self,
        _bodies: &rp::RigidBodySet,
        _colliders: &rp::ColliderSet,
        event: rp::CollisionEvent,
        _contact_pair: Option<&rp::ContactPair>,
    ) {
        self.0.lock().expect("one thread").push(event);
    }

    fn handle_contact_force_event(
        &self,
        _dt: f32,
        _bodies: &rp::RigidBodySet,
        _colliders: &rp::ColliderSet,
        _contact_pair: &rp::ContactPair,
        _total_force_magnitude: f32,
    ) {
    }

    fn handle_soft_body_tear_event(
        &self,
        _soft_bodies: &rp::SoftBodySet,
        _event: &rp::SoftBodyTearEvent,
    ) {
    }
}
