//! The area of an object as a sensor of the physics: the physics tells when the player
//! is in it, and a query tells which areas the player can use.

use bevy_ecs::change_detection::DetectChanges;
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::query::With;
use bevy_ecs::system::{Commands, Query, Res};
use bevy_ecs::world::{Ref, World};
use glam::{Quat, Vec3};
use maki_physics::{Kinematic, Physics, QueryFilter, Shape};
use maki_scene::Player;

use crate::script::TriggerArea;

/// The height of the sensor of an area, in meters: from the floor of the object up.
const AREA_HEIGHT: f32 = 2.0;
/// An area is usable if it is not farther from the player than this, in meters.
const INTERACT_REACH: f32 = 0.9;

/// On an area: the player is in its sensor, as the last physics step saw it.
#[derive(Component)]
pub(crate) struct PlayerInside;

/// Gives the area of an object its sensor, and a change of the size changes it. Runs
/// before the step of the physics. An object with a body of its own (a mover) keeps it.
pub(crate) fn add_sensors(
    mut commands: Commands,
    areas: Query<(Entity, Ref<TriggerArea>, Option<&Kinematic>)>,
) {
    for (entity, area, kinematic) in &areas {
        let own_body = kinematic.is_some_and(|kinematic| !kinematic.sensor);
        if own_body || (kinematic.is_some() && !area.is_changed()) {
            continue;
        }
        commands.entity(entity).insert(Kinematic {
            shape: Shape::Box {
                half: Vec3::new(area.width / 2.0, AREA_HEIGHT / 2.0, area.depth / 2.0),
            },
            center: Vec3::Y * (AREA_HEIGHT / 2.0),
            sensor: true,
        });
    }
}

/// Marks the areas that the player entered, and unmarks those it left. Runs after the
/// step of the physics; the events see it in their next step.
pub(crate) fn track_player(
    mut commands: Commands,
    physics: Res<Physics>,
    players: Query<(), With<Player>>,
    areas: Query<(), With<TriggerArea>>,
) {
    for event in physics.sensor_events() {
        if players.contains(event.other) && areas.contains(event.sensor) {
            let mut area = commands.entity(event.sensor);
            if event.entered {
                area.insert(PlayerInside);
            } else {
                area.remove::<PlayerInside>();
            }
        }
    }
}

/// The entities with a collider (a sensor too) that the player can reach: a sphere at
/// the player. `player`: its position and view direction.
pub(crate) fn in_reach(world: &World, player: Option<(Vec3, Vec3)>) -> Vec<Entity> {
    let (Some((at, _)), Some(physics)) = (player, world.get_resource::<Physics>()) else {
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
