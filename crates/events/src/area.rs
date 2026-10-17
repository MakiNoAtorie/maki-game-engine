//! The area of an object as a sensor of the physics: the physics tells when the player
//! is in it, and a query tells which areas the player can use.

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::query::{Changed, Has, With};
use bevy_ecs::system::{Commands, Query, Res};
use bevy_ecs::world::World;
use glam::{Quat, Vec3};
use maki_physics::{Physics, QueryFilter, Sensor, Shape};
use maki_scene::Player;

use crate::script::TriggerArea;

/// The height of the sensor of an area, in meters: from the floor of the object up.
const AREA_HEIGHT: f32 = 2.0;
/// An area is usable if it is not farther from the player than this, in meters.
const INTERACT_REACH: f32 = 0.9;

/// On an area: the player is in its sensor, as the last physics step saw it.
#[derive(Component)]
pub(crate) struct PlayerInside;

/// Gives the area of an object its [`Sensor`], and a change of the size changes it. Runs
/// before the step of the physics. The sensor is a collider of its own: an object with
/// a body (a door, a mover) keeps it. It does not turn with the object, as the
/// rectangle of `faces` (`EventDef::area_at`).
pub(crate) fn add_sensors(
    mut commands: Commands,
    areas: Query<(Entity, &TriggerArea), Changed<TriggerArea>>,
) {
    for (entity, area) in &areas {
        commands.entity(entity).insert(Sensor {
            shape: Shape::Box {
                half: Vec3::new(area.width / 2.0, AREA_HEIGHT / 2.0, area.depth / 2.0),
            },
            center: Vec3::Y * (AREA_HEIGHT / 2.0),
        });
    }
}

/// Marks the areas that the player is in, and unmarks the others: a test of the shapes
/// after the step of the physics. The events see it in their next step.
pub(crate) fn track_player(
    mut commands: Commands,
    physics: Res<Physics>,
    players: Query<Entity, With<Player>>,
    areas: Query<(Entity, Has<PlayerInside>), With<TriggerArea>>,
) {
    for (area, marked) in &areas {
        let inside = players.iter().any(|player| physics.in_sensor(area, player));
        if inside && !marked {
            commands.entity(area).insert(PlayerInside);
        } else if !inside && marked {
            commands.entity(area).remove::<PlayerInside>();
        }
    }
}

/// The entities with a collider (a sensor too) that the player at `at` can reach: a
/// sphere at the player.
pub(crate) fn in_reach(world: &World, at: Option<Vec3>) -> Vec<Entity> {
    let (Some(at), Some(physics)) = (at, world.get_resource::<Physics>()) else {
        return Vec::new();
    };
    let shape = Shape::Sphere {
        radius: INTERACT_REACH,
    };
    let filter = QueryFilter {
        exclude: None,
        sensors: true,
    };
    physics.overlap(shape, at, Quat::IDENTITY, filter)
}
