//! Collision and bodies, on the library Rapier. The world of the physics is a copy of
//! the entities of `scene`: each `Collider` box is a fixed collider, each entity
//! with [`Kinematic`] (a door leaf, a mover) is a body that follows its pose, and each
//! object with the components `collider` and `body` ([`ObjectCollider`], [`Body`]) is
//! a body of its kind. It steps in `FixedUpdate`, in [`PhysicsSet::Step`].
//!
//! The floor is a fixed box under y = 0 for the dynamic bodies only: the queries do
//! not see it (the walk has its floor at y = 0 in code).
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
use maki_core::{App, Fields, FixedUpdate, Module, ResetPlay, Transform};
use maki_scene::add_object_component;
use rapier3d::control::{CharacterAutostep, CharacterLength, KinematicCharacterController};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude as rp;
use serde::{Deserialize, Serialize};

/// Adds the world of the physics and its step, and the components `collider` and
/// `body` of an object.
pub struct PhysicsModule;

impl Module for PhysicsModule {
    fn build(&self, app: &mut App) {
        add_object_component::<ObjectCollider>(app, "collider");
        add_object_component::<Body>(app, "body");
        app.init_resource::<Physics>()
            .add_systems(
                FixedUpdate,
                (
                    sync::add_mover_shapes,
                    sync::sync_world,
                    sync::step,
                    sync::write_poses,
                )
                    .chain()
                    .in_set(PhysicsSet::Step)
                    // The movers set their poses first.
                    .after(maki_scene::move_movers),
            )
            // Only the poses of the dynamic bodies change: the next step takes them.
            .add_systems(ResetPlay, sync::restart_bodies.ambiguous_with_all());
    }
}

/// The component `collider` of an object: its entity collides. With no [`Body`], the
/// collider is fixed at the pose of the object. On a prop it takes the place of the
/// box of the prop (`collider: true`).
#[derive(Component, Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
#[fields(default)]
pub struct ObjectCollider {
    pub shape: ColliderShape,
    /// 0.0: the collider slides on all; 1.0 and more: it grips.
    #[field(range = 0.0..=2.0, step = 0.01)]
    pub friction: f32,
    /// True: it blocks nothing, and tells what enters and leaves it.
    pub sensor: bool,
}

impl Default for ObjectCollider {
    fn default() -> Self {
        Self {
            shape: ColliderShape::FromMesh,
            friction: 0.5,
            sensor: false,
        }
    }
}

/// The form of an [`ObjectCollider`]. The sizes are meters, around the origin of the
/// object, and the scale of the object does not change them.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, Fields)]
pub enum ColliderShape {
    /// The box around the mesh of the object, at the scale of the object. An object
    /// with no mesh of its own (a model file) has no collider.
    #[default]
    FromMesh,
    /// `half`: half the size along each axis.
    #[field(default = ColliderShape::Box { half: [0.5; 3] })]
    Box { half: [f32; 3] },
    #[field(default = ColliderShape::Sphere { radius: 0.5 })]
    Sphere {
        #[field(range = 0.01..=100.0)]
        radius: f32,
    },
    /// Upright: see [`Shape::Capsule`].
    #[field(default = ColliderShape::Capsule { radius: 0.3, half_height: 0.5 })]
    Capsule {
        #[field(range = 0.01..=10.0)]
        radius: f32,
        #[field(label = "half height", range = 0.0..=10.0)]
        half_height: f32,
    },
}

/// The component `body` of an object: how its [`ObjectCollider`] moves.
#[derive(Component, Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, Fields)]
pub enum Body {
    /// At the pose of the object; a new pose puts it there.
    #[default]
    Fixed,
    /// It falls, hits, and is pushed; its pose goes into the pose of the object after
    /// each step. Not saved: a room load starts it at its place in the file.
    #[field(default = Body::Dynamic { mass: 10.0, damping: 0.0 })]
    Dynamic {
        /// Kilograms.
        #[field(range = 0.01..=10000.0)]
        mass: f32,
        /// How fast it slows down when nothing pushes it: 0.0 not at all.
        #[field(range = 0.0..=10.0, step = 0.01)]
        damping: f32,
    },
    /// Follows the pose of the object (a command, a script), and pushes dynamic bodies.
    Kinematic,
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
#[derive(Resource)]
pub struct Physics {
    world: rp::PhysicsWorld,
    // A `BTreeMap`: the same order in each run, so the same handles and steps.
    links: BTreeMap<Entity, Link>,
    sensor_events: Vec<SensorEvent>,
    /// The collider of the floor, of no entity.
    floor: rp::ColliderHandle,
}

impl Default for Physics {
    fn default() -> Self {
        let mut world = rp::PhysicsWorld::default();
        // Wider than a room: a body that falls out of a doorway lands on it too.
        let floor = rp::ColliderBuilder::cuboid(500.0, 1.0, 500.0).translation(Vector::NEG_Y);
        let floor = world.insert_collider(floor, None);
        Self {
            world,
            links: BTreeMap::new(),
            sensor_events: Vec::new(),
            floor,
        }
    }
}

/// The Rapier handles of an entity.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Link {
    /// Of its `Collider` box.
    fixed: Option<rp::ColliderHandle>,
    /// Of its [`Kinematic`].
    moving: Option<(rp::RigidBodyHandle, rp::ColliderHandle)>,
    /// Of its [`ObjectCollider`] and [`Body`].
    solid: Option<Solid>,
}

/// The body of an [`ObjectCollider`].
#[derive(Debug, Clone, Copy, PartialEq)]
struct Solid {
    body: rp::RigidBodyHandle,
    collider: rp::ColliderHandle,
    dynamic: bool,
    /// The pose when the body was made: `ResetPlay` puts a dynamic body back there.
    start: Transform,
}

impl Physics {
    /// Gives the dynamic body of `entity` an impulse (newton seconds) at its center. An
    /// entity with no dynamic body: no change.
    pub fn push(&mut self, entity: Entity, impulse: Vec3) {
        let Some(solid) = self.links.get(&entity).and_then(|link| link.solid) else {
            return;
        };
        if solid.dynamic {
            self.world.bodies[solid.body].apply_impulse(vector(impulse), true);
        }
    }

    /// The colliders that a query sees: not the floor, not those of `filter.exclude`.
    fn sees(&self, filter: &QueryFilter) -> impl Fn(rp::ColliderHandle, &rp::Collider) -> bool {
        let (floor, exclude) = (self.floor, filter.exclude);
        move |handle, collider| handle != floor && Some(owner(collider)) != exclude
    }

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
        let predicate = self.sees(&filter);
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
        let predicate = self.sees(&filter);
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
        let predicate = self.sees(&filter);
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

/// The Rapier filter of a [`QueryFilter`]. `predicate`: [`Physics::sees`].
fn rapier_filter<'a>(
    filter: &QueryFilter,
    predicate: &'a dyn Fn(rp::ColliderHandle, &rp::Collider) -> bool,
) -> rp::QueryFilter<'a> {
    let mut rapier = rp::QueryFilter::default();
    if !filter.sensors {
        rapier = rapier.exclude_sensors();
    }
    rapier.predicate = Some(predicate);
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
