//! The component `hidden`: an object that is not in the picture, has no collider,
//! and whose area gives no output. The renderer, the walk, and the events skip each
//! entity with it.

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::query::{With, Without};
use bevy_ecs::system::{Commands, Query};
use maki_core::Fields;
use serde::{Deserialize, Serialize};

use crate::RoomItem;

/// Marks a hidden entity. All entities of an object have it, or none.
#[derive(Component, Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, Fields)]
#[serde(deny_unknown_fields)]
pub struct Hidden {}

/// Runs in `Update`: gives `Hidden` to each entity of an object of which one entity
/// has it. The room file gives it to the object entity only, and the meshes of a
/// model file come some frames after the load.
pub(crate) fn spread_hidden(
    mut commands: Commands,
    hidden: Query<&RoomItem, With<Hidden>>,
    shown: Query<(Entity, &RoomItem), Without<Hidden>>,
) {
    // Normally no object is hidden: one empty query and no more.
    let items: Vec<RoomItem> = hidden.iter().copied().collect();
    if items.is_empty() {
        return;
    }
    for (entity, item) in &shown {
        if items.contains(item) {
            commands.entity(entity).insert(Hidden {});
        }
    }
}
