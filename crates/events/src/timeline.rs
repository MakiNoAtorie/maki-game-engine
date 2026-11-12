//! The commands that play and stop a timeline (`maki_scene::Timeline`) by the id of its object.

use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use maki_core::{App, Fields};
use maki_scene::{ObjectId, RoomDef, Timeline, play_timeline, stop_timeline};
use serde::{Deserialize, Serialize};

use crate::CommandRegistry;
use crate::check::Finding;
use crate::command::Step;

/// `Fields`: the editor draws their arguments. ObjectId 0: the object of the event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
pub enum TimelineCommand {
    /// Plays the timeline of an object from time 0. `wait`: the next row runs when it ends.
    PlayTimeline(ObjectId, #[field(label = "wait")] bool),
    /// Stops the timeline. `end`: first writes the values of its end.
    StopTimeline(ObjectId, #[field(label = "end")] bool),
}

pub(crate) fn add(_app: &mut App, registry: &CommandRegistry) {
    registry.add(run);
    registry.add_texts(row_text, help);
    registry.add_fields::<TimelineCommand>();
    registry.add_defaults(new_command);
    registry.add_check(|command: &TimelineCommand, context| {
        let (TimelineCommand::PlayTimeline(object, flag)
        | TimelineCommand::StopTimeline(object, flag)) = command;
        // 0 is the object of the event: the room check cannot know it.
        if *object == ObjectId::default() {
            return None;
        }
        let Some((_, found)) = context.room.find(object) else {
            return Some(Finding::error(format!("the room has no object `{object}`")));
        };
        let Some(text) = found.components.get(Timeline::NAME) else {
            return Some(Finding::error(format!("`{object}` has no timeline")));
        };
        let looping = text
            .read::<Timeline>()
            .is_ok_and(|timeline| timeline.looping);
        let waits = matches!(command, TimelineCommand::PlayTimeline(..)) && *flag && looping;
        waits.then(|| Finding::warning(format!("`{object}` loops: the wait never ends")))
    });
}

/// The entity of an object (0: of the event) that has a timeline.
// Not `object_entity`: that one needs `Interpolated`; this one needs `Timeline`.
fn timeline_entity(world: &mut World, step: &Step, object: ObjectId) -> Option<Entity> {
    if object == ObjectId::default() {
        return step
            .entity
            .filter(|entity| world.get::<Timeline>(*entity).is_some());
    }
    let mut query = world.query::<(Entity, &ObjectId, &Timeline)>();
    let found = query.iter(world).find(|(_, id, _)| **id == object);
    found.map(|(entity, ..)| entity)
}

fn run(command: &TimelineCommand, step: &mut Step, world: &mut World) -> bool {
    let (TimelineCommand::PlayTimeline(object, _) | TimelineCommand::StopTimeline(object, _)) =
        command;
    let Some(entity) = timeline_entity(world, step, *object) else {
        tracing::warn!(event = step.event, %object, "{command:?}: no timeline");
        return true;
    };
    match command {
        TimelineCommand::PlayTimeline(_, wait) => {
            if step.start {
                play_timeline(world, entity);
            }
            // A looping timeline never ends: no wait (the room check warns).
            let timeline = world.get::<Timeline>(entity).expect("found above");
            !*wait || timeline.looping || !timeline.playing
        }
        TimelineCommand::StopTimeline(_, end) => {
            stop_timeline(world, entity, *end);
            true
        }
    }
}

fn row_text(command: &TimelineCommand) -> Option<String> {
    Some(match command {
        TimelineCommand::PlayTimeline(object, wait) => {
            format!(
                "Play timeline {object}{}",
                if *wait { ", wait" } else { "" }
            )
        }
        TimelineCommand::StopTimeline(object, end) => {
            format!(
                "Stop timeline {object}{}",
                if *end { ", to the end" } else { "" }
            )
        }
    })
}

fn help(name: &str) -> Option<&'static str> {
    Some(match name {
        "PlayTimeline" => {
            "Plays the timeline of an object from time 0. With wait, waits until it ends."
        }
        "StopTimeline" => {
            "Stops the timeline of an object. With end, writes the values of its end first."
        }
        _ => return None,
    })
}

/// The new rows name the first object with a timeline, else the first with an id.
fn new_command(name: &str, room: &RoomDef) -> Option<TimelineCommand> {
    let mut with_id = room.objects.iter().filter(|object| object.id.is_some());
    let first = with_id.clone().next();
    let object = with_id.find(|object| object.components.contains_key(Timeline::NAME));
    let id = object
        .or(first)
        .and_then(|object| object.id)
        .unwrap_or_default();
    Some(match name {
        "PlayTimeline" => TimelineCommand::PlayTimeline(id, true),
        "StopTimeline" => TimelineCommand::StopTimeline(id, false),
        _ => return None,
    })
}
