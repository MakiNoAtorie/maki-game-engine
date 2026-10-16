//! The commands that show, hide, and move any object of the room by its id. They
//! are not saved: object state and an autorun page apply them again.

use std::collections::HashMap;

use bevy_ecs::entity::Entity;
use bevy_ecs::resource::Resource;
use bevy_ecs::world::World;
use glam::Vec3;
use maki_core::{App, Fields, Interpolated, Time, Transform};
use maki_scene::{Collider, CurrentRoom, Hidden, ObjectId, RoomDef, RoomItem};
use serde::{Deserialize, Serialize};

use crate::CommandRegistry;
use crate::check::Finding;
use crate::command::Step;

/// `Fields`: the editor draws their arguments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
pub enum ObjectCommand {
    /// Takes the component `hidden` away from the object and its children.
    Show(ObjectId),
    /// The object and its children are not in the picture, have no collider, and give no output.
    Hide(ObjectId),
    /// Moves a root object to a place in the room in some seconds (0: at once), and
    /// waits until it is there.
    MoveObject(
        ObjectId,
        #[field(label = "place")] [f32; 3],
        #[field(label = "seconds", range = 0.0..=60.0, step = 0.05)] f32,
    ),
}

/// How far each object of the current room moved from its place in the room file.
#[derive(Resource, Debug, Default)]
struct MovedObjects {
    /// `CurrentRoom::loads` of the offsets.
    loads: u64,
    offsets: HashMap<ObjectId, Vec3>,
}

pub(crate) fn add(app: &mut App, registry: &CommandRegistry) {
    app.init_resource::<MovedObjects>();
    registry.add(run);
    registry.add_texts(row_text, help);
    registry.add_fields::<ObjectCommand>();
    registry.add_defaults(new_command);
    registry.add_check(|command: &ObjectCommand, context| {
        let (ObjectCommand::Show(object)
        | ObjectCommand::Hide(object)
        | ObjectCommand::MoveObject(object, ..)) = command;
        let Some((_, found)) = context.room.find(object) else {
            return Some(Finding::error(format!("the room has no object `{object}`")));
        };
        let child = matches!(command, ObjectCommand::MoveObject(..)) && found.parent.is_some();
        child.then(|| Finding::error(format!("`{object}` is not a root object: no move")))
    });
}

fn run(command: &ObjectCommand, step: &mut Step, world: &mut World) -> bool {
    match command {
        ObjectCommand::Show(object) => {
            for entity in tree_entities(world, object) {
                world.entity_mut(entity).remove::<Hidden>();
            }
        }
        ObjectCommand::Hide(object) => {
            for entity in tree_entities(world, object) {
                // Only an entity that is not hidden: a write is a change.
                if !world.entity(entity).contains::<Hidden>() {
                    world.entity_mut(entity).insert(Hidden {});
                }
            }
        }
        ObjectCommand::MoveObject(object, place, seconds) => {
            return move_object(world, step, object, Vec3::from(*place), *seconds);
        }
    }
    true
}

/// The entities of an object: by its id, and by its room item (a model mesh has no id).
pub(crate) fn entities_of(world: &mut World, object: &ObjectId) -> Vec<Entity> {
    let def = &world.resource::<CurrentRoom>().def;
    let items: Vec<usize> = def
        .find(object)
        .map(|(index, _)| index)
        .into_iter()
        .collect();
    entities_of_items(world, &[*object], &items)
}

/// The entities of an object and of its children: a pose is in room space, so a child moves by the same offset.
fn tree_entities(world: &mut World, object: &ObjectId) -> Vec<Entity> {
    let def = &world.resource::<CurrentRoom>().def;
    let tree: Vec<(usize, Option<ObjectId>)> = def
        .objects
        .iter()
        .enumerate()
        .filter(|(_, at)| leads_to(def, at, object))
        .map(|(index, at)| (index, at.id))
        .collect();
    let ids: Vec<ObjectId> = tree.iter().filter_map(|(_, id)| *id).collect();
    let items: Vec<usize> = tree.iter().map(|(index, _)| *index).collect();
    entities_of_items(world, &ids, &items)
}

/// True if the object is `object` or its parent chain leads to it.
fn leads_to<'a>(def: &'a RoomDef, mut at: &'a maki_scene::ObjectDef, object: &ObjectId) -> bool {
    // The chain is cut at the count of objects: a cycle of parents has no end.
    for _ in 0..def.objects.len() {
        if at.id.as_ref() == Some(object) {
            return true;
        }
        let Some((_, parent)) = at.parent.as_ref().and_then(|parent| def.find(parent)) else {
            return false;
        };
        at = parent;
    }
    false
}

/// The entities with one of the ids, then those of one of the room items.
fn entities_of_items(world: &mut World, ids: &[ObjectId], items: &[usize]) -> Vec<Entity> {
    let mut entities = world.query::<(Entity, Option<&ObjectId>, Option<&RoomItem>)>();
    let (mut named, mut of_item) = (Vec::new(), Vec::new());
    for (entity, id, room_item) in entities.iter(world) {
        if id.is_some_and(|id| ids.contains(id)) {
            named.push(entity);
        } else if room_item.is_some_and(|item| items.contains(&item.0)) {
            of_item.push(entity);
        }
    }
    named.extend(of_item);
    named
}

/// The memory of a move: where the object was at the start, and the seconds since.
#[derive(Default)]
struct Move {
    from: Vec3,
    passed: f32,
}

/// One step of a move. True: the object is at its place.
fn move_object(
    world: &mut World,
    step: &mut Step,
    object: &ObjectId,
    place: Vec3,
    seconds: f32,
) -> bool {
    let current = world.resource::<CurrentRoom>();
    let loads = current.loads;
    let Some(start) = current
        .def
        .find(object)
        .filter(|(_, def)| def.parent.is_none())
        .map(|(_, def)| Vec3::from(def.pose.position))
    else {
        tracing::warn!(event = step.event, %object, "MoveObject: no such root object");
        return true;
    };
    let mut moved = world.resource_mut::<MovedObjects>();
    if moved.loads != loads {
        *moved = MovedObjects {
            loads,
            offsets: HashMap::new(),
        };
    }
    let offset = moved.offsets.get(object).copied().unwrap_or(Vec3::ZERO);
    let passed = world.resource::<Time>().fixed_delta.as_secs_f32();
    let first = step.start;
    let memory = step.memory::<Move>();
    if first {
        *memory = Move {
            from: start + offset,
            passed: 0.0,
        };
    }
    memory.passed += passed;
    // The small number: the sum of the fixed steps is not exactly the seconds.
    let part = if seconds > 0.0 && memory.passed < seconds - 1e-5 {
        memory.passed / seconds
    } else {
        1.0
    };
    let target = memory.from.lerp(place, part);
    let delta = target - (start + offset);
    world
        .resource_mut::<MovedObjects>()
        .offsets
        .insert(*object, target - start);
    // Only a different pose is a write: a write is a change for the renderer.
    if delta != Vec3::ZERO {
        for entity in tree_entities(world, object) {
            shift(world, entity, delta);
        }
    }
    part >= 1.0
}

/// Moves the pose and the collider of an entity.
// ponytail: a door or a mover keeps its own poses; move them when a room needs it.
fn shift(world: &mut World, entity: Entity, delta: Vec3) {
    let mut entity = world.entity_mut(entity);
    if let Some(mut pose) = entity.get_mut::<Interpolated>() {
        pose.current.translation += delta;
    } else if let Some(mut transform) = entity.get_mut::<Transform>() {
        transform.translation += delta;
    }
    if let Some(mut collider) = entity.get_mut::<Collider>() {
        collider.min += delta;
        collider.max += delta;
    }
}

fn row_text(command: &ObjectCommand) -> Option<String> {
    Some(match command {
        ObjectCommand::Show(object) => format!("Show {object}"),
        ObjectCommand::Hide(object) => format!("Hide {object}"),
        ObjectCommand::MoveObject(object, [x, y, z], seconds) => {
            format!("Move {object} to ({x}, {y}, {z}), {seconds} s")
        }
    })
}

fn help(name: &str) -> Option<&'static str> {
    Some(match name {
        "Show" => "Shows a hidden object again.",
        "Hide" => "Hides an object: no picture, no collider, no output of its area.",
        "MoveObject" => "Moves an object to a place in the room in some seconds, and waits.",
        _ => return None,
    })
}

/// The new rows of the "Add" list name the first root object with an id.
fn new_command(name: &str, room: &RoomDef) -> Option<ObjectCommand> {
    let root = |object: &&maki_scene::ObjectDef| object.parent.is_none() && object.id.is_some();
    let object = room.objects.iter().find(root);
    let id = object.and_then(|object| object.id).unwrap_or_default();
    let place = object.map_or([0.0; 3], |object| object.pose.position);
    Some(match name {
        "Show" => ObjectCommand::Show(id),
        "Hide" => ObjectCommand::Hide(id),
        "MoveObject" => ObjectCommand::MoveObject(id, place, 1.0),
        _ => return None,
    })
}
