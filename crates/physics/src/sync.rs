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

use crate::{Kinematic, Link, Physics, SensorEvent, Shape, pose, shared_shape, vector};

/// An entity with a [`Kinematic`], the poses that it can follow, and whether it is hidden.
type Followed = (
    Entity,
    Ref<'static, Kinematic>,
    Option<&'static Interpolated>,
    Option<&'static Transform>,
    Has<Hidden>,
);

/// Gives each mover (a door leaf too) a [`Kinematic`] box: the box of its mesh, at the
/// scale of its pose `a`.
pub(crate) fn add_mover_shapes(
    mut commands: Commands,
    meshes: Res<Assets<Mesh>>,
    movers: Query<(Entity, &MeshInstance, &Mover), Without<Kinematic>>,
) {
    for (entity, mesh, mover) in &movers {
        let Some(mesh) = meshes.get(mesh.0) else {
            continue;
        };
        let (min, max) = mesh.positions().iter().fold(
            (glam::Vec3::splat(f32::MAX), glam::Vec3::splat(f32::MIN)),
            |(min, max), position| (min.min(*position), max.max(*position)),
        );
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

/// Makes the world of the physics like the entities: a new box or body for a new
/// entity, a changed one where its component changed, and a removed one where its
/// entity is gone. A `Hidden` entity, and an entity whose `Collider` or `Kinematic`
/// went away, keep their handles with the colliders off.
pub(crate) fn sync_world(
    mut physics: ResMut<Physics>,
    entities: &Entities,
    boxes: Query<(Entity, Ref<Collider>, Has<Hidden>)>,
    moving: Query<Followed>,
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
        *link != Link::default()
    });

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
