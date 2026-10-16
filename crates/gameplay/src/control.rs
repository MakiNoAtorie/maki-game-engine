//! The controller: what the move actions steer. As in Unreal, three things:
//!
//! - the pawn: the entity with `maki_scene::Player`. The events, the scripts, the
//!   sound, the camera, and the save find the player by this marker;
//! - the controller: [`Controller`] and `steer_player`. The control can move to
//!   another object ([`give_control`]);
//! - the rules of the game: `GameModule` (`game.rs`). They say when the control is on.

use bevy_ecs::entity::Entity;
use bevy_ecs::query::With;
use bevy_ecs::resource::Resource;
use bevy_ecs::world::World;
use glam::Vec3;
use maki_scene::Player;

use crate::player::{PlayerDef, Walker};

/// What the move actions steer. `PlayerModule` adds it.
#[derive(Resource, Debug, Default)]
pub struct Controller {
    /// The body that `PlayerModule` makes from the player prefab. The control goes
    /// back to it at `ResetPlay`, and when a room change removes the pawn.
    pub home: Option<Entity>,
    /// True: the move actions steer the pawn. The rules of the game write it
    /// (`GameModule`); with no rules, the pawn does not move.
    pub active: bool,
}

/// The control goes to `target`: it becomes the pawn, the one entity with `Player`.
/// A target with no `Walker` or `PlayerDef` gets those of the pawn before: what
/// walking needs. The pawn before stands. False: `target` is not an entity.
pub fn give_control(world: &mut World, target: Entity) -> bool {
    if world.get_entity(target).is_err() {
        return false;
    }
    let mut pawns = world.query_filtered::<Entity, With<Player>>();
    let before: Vec<Entity> = pawns.iter(world).collect();
    if before == [target] {
        return true;
    }
    // The pawn before, or the home body if a room removed the pawn.
    let home = world.resource::<Controller>().home;
    let source = before.first().copied().or(home);
    let walker = source.and_then(|source| world.get::<Walker>(source).copied());
    let def = source.and_then(|source| world.get::<PlayerDef>(source).cloned());
    for pawn in before {
        let mut pawn = world.entity_mut(pawn);
        pawn.remove::<Player>();
        if let Some(mut walker) = pawn.get_mut::<Walker>() {
            walker.direction = Vec3::ZERO;
        }
    }
    let mut target = world.entity_mut(target);
    target.insert(Player);
    if let (false, Some(walker)) = (target.contains::<Walker>(), walker) {
        target.insert(Walker {
            direction: Vec3::ZERO,
            ..walker
        });
    }
    if let (false, Some(def)) = (target.contains::<PlayerDef>(), def) {
        target.insert(def);
    }
    true
}

/// Runs in `ResetPlay`: the control goes back to the home body.
pub(crate) fn control_home(world: &mut World) {
    if let Some(home) = world.resource::<Controller>().home {
        give_control(world, home);
    }
}

/// Runs in `Update` after the room loader: a room change removed the pawn (an object
/// of the old room). The control goes back to the home body.
pub(crate) fn control_home_if_lost(world: &mut World) {
    let mut pawns = world.query_filtered::<(), With<Player>>();
    if pawns.iter(world).next().is_none() {
        control_home(world);
    }
}
