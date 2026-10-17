//! Collision and bodies, on the library Rapier. The world of the physics is a copy of
//! the entities of `scene`: each `Collider` box is a fixed collider, and each entity
//! with [`Kinematic`] (a door leaf, a mover) is a body that follows its pose. It steps
//! in `FixedUpdate`, in [`PhysicsSet::Step`].
//!
//! No type of Rapier or of nalgebra is in the API of this crate: the conversions to
//! `glam` are inside. This crate must not depend on `gpu`, `render`, or `ui`.

mod sync;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::{IntoScheduleConfigs, SystemSet};
use glam::{Quat, Vec3};
use maki_core::{App, FixedUpdate, Module};
use rapier3d::control::{CharacterAutostep, CharacterLength, KinematicCharacterController};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude as rp;

/// Adds the world of the physics and its step.
pub struct PhysicsModule;

impl Module for PhysicsModule {
    fn build(&self, app: &mut App) {
        app.init_resource::<Physics>().add_systems(
            FixedUpdate,
            (sync::add_mover_shapes, sync::sync_world, sync::step)
                .chain()
                .in_set(PhysicsSet::Step)
                // The movers set their poses first.
                .after(maki_scene::move_movers),
        );
    }
}

/// The systems of the physics in `FixedUpdate`. A system that reads what a step did
/// (the sensor events, the new poses) runs `.after(PhysicsSet::Step)`; one that moves
/// a body for this step runs `.before(PhysicsSet::Step)`.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PhysicsSet {
    Step,
}

/// The form of a collider, around its center. It has no scale: the code that makes it
/// gives the sizes in meters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    /// A box with sides along the axes of its pose. `half`: half its size.
    Box {
        half: Vec3,
    },
    Sphere {
        radius: f32,
    },
    /// A cylinder along Y with a half sphere at each end. `half_height`: half the length
    /// of the cylinder part; the full height is `2 * (half_height + radius)`.
    Capsule {
        radius: f32,
        half_height: f32,
    },
}

/// A collider that follows the pose of its entity: a kinematic body of Rapier. The pose
/// is `Interpolated::current` if the entity has it, else its `Transform` (with no
/// scale). A door leaf and a mover get one from the box of their mesh.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Kinematic {
    pub shape: Shape,
    /// The center of the shape, in the space of the pose.
    pub center: Vec3,
    /// True: the collider blocks nothing, and tells what enters and leaves it
    /// ([`Physics::sensor_events`]).
    pub sensor: bool,
}

/// What a query takes into account.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct QueryFilter {
    /// The colliders of this entity are not found (the entity that asks).
    pub exclude: Option<Entity>,
    /// True: sensors are found too.
    pub sensors: bool,
}

/// Where a ray hit a collider.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub entity: Entity,
    pub point: Vec3,
    /// Length 1, out of the collider.
    pub normal: Vec3,
    /// From the start of the ray, in meters.
    pub distance: f32,
}

/// How a character moves along the colliders ([`Physics::move_character`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CharacterSettings {
    /// The highest step that the character goes up, in meters. 0.0: no step.
    pub step_height: f32,
    /// The steepest slope that the character goes up, in degrees.
    pub max_slope_degrees: f32,
    /// The gap that the character keeps to the colliders, in meters.
    pub gap: f32,
}

/// Goes up no step, and up a slope to 45 degrees.
impl Default for CharacterSettings {
    fn default() -> Self {
        Self {
            step_height: 0.0,
            max_slope_degrees: 45.0,
            gap: 0.01,
        }
    }
}

/// What a move of a character did.
#[derive(Debug, Clone, PartialEq)]
pub struct CharacterMove {
    /// The step that the character can make: the wanted step, less the parts that go
    /// into colliders.
    pub step: Vec3,
    /// True: the character stands on a collider at the end of the step.
    pub grounded: bool,
    /// The entities whose colliders the move touched, each one time, in the order of
    /// the touches.
    pub touched: Vec<Entity>,
}

/// A sensor and an entity that entered or left it in the last step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SensorEvent {
    pub sensor: Entity,
    pub other: Entity,
    /// True: entered; false: left.
    pub entered: bool,
}

/// The world of the physics: the Rapier sets and pipeline, and which Rapier handles
/// each entity has.
#[derive(Resource, Default)]
pub struct Physics {
    world: rp::PhysicsWorld,
    // A `BTreeMap`: the same order in each run, so the same handles and steps.
    links: BTreeMap<Entity, Link>,
    sensor_events: Vec<SensorEvent>,
}

/// The Rapier handles of an entity.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Link {
    /// Of its `Collider` box.
    fixed: Option<rp::ColliderHandle>,
    /// Of its [`Kinematic`].
    moving: Option<(rp::RigidBodyHandle, rp::ColliderHandle)>,
}

impl Physics {
    /// The first collider that a ray from `from` along `direction` hits within `length`
    /// meters. A ray that starts in a collider hits it at `from`.
    pub fn ray(
        &self,
        from: Vec3,
        direction: Vec3,
        length: f32,
        filter: QueryFilter,
    ) -> Option<Hit> {
        let direction = direction.try_normalize()?;
        let ray = rp::Ray::new(vector(from), vector(direction));
        let predicate = |_, collider: &rp::Collider| Some(owner(collider)) != filter.exclude;
        let (handle, hit) = self.world.cast_ray_and_get_normal(
            &ray,
            length,
            true,
            rapier_filter(&filter, &predicate),
        )?;
        Some(Hit {
            entity: owner(&self.world.colliders[handle]),
            point: from + direction * hit.time_of_impact,
            normal: vec3(hit.normal),
            distance: hit.time_of_impact,
        })
    }

    /// The entities with a collider that overlaps `shape` at a pose, each one time, in
    /// the order of the entities.
    pub fn overlap(
        &self,
        shape: Shape,
        position: Vec3,
        rotation: Quat,
        filter: QueryFilter,
    ) -> Vec<Entity> {
        let shape = shared_shape(shape);
        let predicate = |_, collider: &rp::Collider| Some(owner(collider)) != filter.exclude;
        let mut found: Vec<Entity> = self
            .world
            .intersect_shape(
                pose(position, rotation),
                shape.as_ref(),
                rapier_filter(&filter, &predicate),
            )
            .map(|(_, collider)| owner(collider))
            .collect();
        found.sort();
        found.dedup();
        found
    }

    /// Moves a character of `shape` (upright, at `position`) by the `wanted` step along
    /// the colliders: it slides along walls, goes up steps and slopes as `settings`
    /// allow, and stops at the rest. Changes nothing in the world: the caller moves the
    /// character by the step that comes back. `filter.exclude`: the character itself.
    pub fn move_character(
        &self,
        shape: Shape,
        position: Vec3,
        wanted: Vec3,
        settings: &CharacterSettings,
        filter: QueryFilter,
    ) -> CharacterMove {
        let controller = KinematicCharacterController {
            offset: CharacterLength::Absolute(settings.gap),
            autostep: (settings.step_height > 0.0).then(|| CharacterAutostep {
                max_height: CharacterLength::Absolute(settings.step_height),
                ..CharacterAutostep::default()
            }),
            max_slope_climb_angle: settings.max_slope_degrees.to_radians(),
            ..KinematicCharacterController::default()
        };
        let shape = shared_shape(shape);
        let predicate = |_, collider: &rp::Collider| Some(owner(collider)) != filter.exclude;
        let queries = self
            .world
            .query_pipeline_with_filter(rapier_filter(&filter, &predicate));
        let mut touched = Vec::new();
        let moved = controller.move_shape(
            self.world.integration_parameters.dt,
            &queries,
            shape.as_ref(),
            &pose(position, Quat::IDENTITY),
            vector(wanted),
            |collision| {
                let entity = owner(&self.world.colliders[collision.handle]);
                if !touched.contains(&entity) {
                    touched.push(entity);
                }
            },
        );
        CharacterMove {
            step: vec3(moved.translation),
            grounded: moved.grounded,
            touched,
        }
    }

    /// The sensors that an entity entered or left in the last step, in the order of
    /// Rapier.
    pub fn sensor_events(&self) -> &[SensorEvent] {
        &self.sensor_events
    }
}

/// The Rapier filter of a [`QueryFilter`]. `predicate` leaves out the excluded entity.
fn rapier_filter<'a>(
    filter: &QueryFilter,
    predicate: &'a dyn Fn(rp::ColliderHandle, &rp::Collider) -> bool,
) -> rp::QueryFilter<'a> {
    let mut rapier = rp::QueryFilter::default();
    if !filter.sensors {
        rapier = rapier.exclude_sensors();
    }
    if filter.exclude.is_some() {
        rapier.predicate = Some(predicate);
    }
    rapier
}

/// The entity of a collider: its `user_data`.
fn owner(collider: &rp::Collider) -> Entity {
    Entity::from_bits(collider.user_data as u64)
}

fn shared_shape(shape: Shape) -> rp::SharedShape {
    match shape {
        Shape::Box { half } => rp::SharedShape::cuboid(half.x, half.y, half.z),
        Shape::Sphere { radius } => rp::SharedShape::ball(radius),
        Shape::Capsule {
            radius,
            half_height,
        } => rp::SharedShape::capsule_y(half_height, radius),
    }
}

// Rapier has its own version of `glam`: the conversions go through arrays.
fn vector(v: Vec3) -> Vector {
    Vector::from_array(v.to_array())
}

fn vec3(v: Vector) -> Vec3 {
    Vec3::from_array(v.to_array())
}

fn pose(position: Vec3, rotation: Quat) -> Pose {
    Pose::from_parts(vector(position), Rotation::from_array(rotation.to_array()))
}
