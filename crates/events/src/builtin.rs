//! The commands of this crate. They are added to the registry as the commands of
//! any other module are: an enum and a handler.
//!
//! Each command writes a component or a resource of the scene crate or of this
//! crate. No command calls the renderer or the user interface: they read what the
//! commands wrote.

use bevy_ecs::world::World;
use maki_core::Time;
use maki_platform::FixedActionState;
use maki_scene::{
    CurrentRoom, Light, Mover, MoverState, Name, RoomRequest, RoomTarget, ScreenFade,
};
use serde::{Deserialize, Serialize};

use crate::command::{Command, Step};
use crate::{GameState, INTERACT, MessageBox};

/// Text arguments are names: of a flag, a variable, or an entity of the room file
/// (`name:` of a door, a mover, a light).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Builtin {
    SetFlag(String, bool),
    SetVariable(String, i32),
    AddVariable(String, i32),
    /// Seconds.
    Wait(f32),
    /// The first list if the flag is set, else the second one.
    If(String, Vec<Command>, Vec<Command>),
    /// Sends a door or a mover to its pose B (a door: open) and waits until it is there.
    OpenDoor(String),
    /// To pose A (a door: closed).
    CloseDoor(String),
    LightOn(String),
    LightOff(String),
    LightIntensity(String, f32),
    /// Shows the text until the player presses interact.
    Message(String),
    /// To black, in this many seconds.
    FadeOut(f32),
    /// Back to the picture.
    FadeIn(f32),
    /// To a spawn point of another room. The first text is the room file, from the
    /// directory of the current room file. The event goes on in the new room.
    Transfer(String, String),
}

/// One step of a built-in command. False: the command waits.
pub(crate) fn run(command: &Builtin, step: &mut Step, world: &mut World) -> bool {
    match command {
        Builtin::SetFlag(name, value) => world.resource_mut::<GameState>().set_flag(name, *value),
        Builtin::SetVariable(name, value) => {
            world.resource_mut::<GameState>().set_variable(name, *value);
        }
        Builtin::AddVariable(name, value) => {
            let mut state = world.resource_mut::<GameState>();
            let sum = state.variable(name).saturating_add(*value);
            state.set_variable(name, sum);
        }
        Builtin::Wait(seconds) => {
            let passed = world.resource::<Time>().fixed_delta.as_secs_f32();
            let start = step.start;
            let left = step.memory::<f32>();
            if start {
                *left = *seconds;
            }
            *left -= passed;
            // The small number: the sum of 60 steps of 1/60 s is not exactly 1 s.
            return *left <= 1e-5;
        }
        Builtin::If(flag, then, otherwise) => {
            let branch = if world.resource::<GameState>().flag(flag) {
                then
            } else {
                otherwise
            };
            step.open(branch.clone());
        }
        Builtin::OpenDoor(name) => return send_mover(world, name, true, step.start),
        Builtin::CloseDoor(name) => return send_mover(world, name, false, step.start),
        Builtin::LightOn(name) => set_light(world, name, |light| Light { on: true, ..*light }),
        Builtin::LightOff(name) => set_light(world, name, |light| Light {
            on: false,
            ..*light
        }),
        Builtin::LightIntensity(name, intensity) => set_light(world, name, |light| Light {
            intensity: *intensity,
            ..*light
        }),
        Builtin::Message(text) => {
            if step.start {
                tracing::debug!(event = step.event, text, "message");
                world.resource_mut::<MessageBox>().0 = Some(text.clone());
                return false;
            }
            if !world.resource::<FixedActionState>().just_pressed(INTERACT) {
                return false;
            }
            world.resource_mut::<MessageBox>().0 = None;
        }
        Builtin::FadeOut(seconds) | Builtin::FadeIn(seconds) => {
            let passed = world.resource::<Time>().fixed_delta.as_secs_f32();
            let part = if *seconds > 0.0 {
                passed / seconds
            } else {
                1.0
            };
            let out = matches!(command, Builtin::FadeOut(_));
            let mut fade = world.resource_mut::<ScreenFade>();
            fade.0 = (fade.0 + if out { part } else { -part }).clamp(0.0, 1.0);
            return fade.0 == if out { 1.0 } else { 0.0 };
        }
        Builtin::Transfer(room, spawn) => {
            let current = world.resource::<CurrentRoom>();
            let loads = current.loads;
            if step.start {
                let directory = current.path.parent().unwrap_or(std::path::Path::new(""));
                let path = directory.join(room);
                world.resource_mut::<RoomRequest>().0 = Some(RoomTarget {
                    path,
                    spawn: Some(spawn.clone()),
                });
                // `CurrentRoom::loads` when the room was asked for.
                *step.memory::<u64>() = loads;
                return false;
            }
            if loads == *step.memory::<u64>() {
                // The loader has not run yet (two fixed steps in one frame).
                if world.resource::<RoomRequest>().0.is_some() {
                    return false;
                }
                // The loader logged why. The event goes on in the old room.
                tracing::error!(event = step.event, room, "the transfer did not happen");
            }
        }
    }
    true
}

/// Sends each mover of this name to pose B or A (only `start`), and tells if all of
/// them are there.
fn send_mover(world: &mut World, name: &str, to_b: bool, start: bool) -> bool {
    let (mut found, mut arrived) = (false, true);
    let mut movers = world.query::<(&Name, &mut Mover)>();
    for (mover_name, mut mover) in movers.iter_mut(world) {
        if mover_name.0 != name {
            continue;
        }
        found = true;
        let target = if to_b {
            MoverState::AtB
        } else {
            MoverState::AtA
        };
        if mover.state == target {
            continue;
        }
        // Only a mover that is not there is written: a write is a change.
        if start {
            if to_b {
                mover.go_to_b();
            } else {
                mover.go_to_a();
            }
        }
        arrived = false;
    }
    if !found && start {
        tracing::warn!(
            name,
            "an event names a door or mover that the room does not have"
        );
    }
    arrived
}

fn set_light(world: &mut World, name: &str, change: impl Fn(&Light) -> Light) {
    let mut found = false;
    let mut lights = world.query::<(&Name, &mut Light)>();
    for (light_name, mut light) in lights.iter_mut(world) {
        if light_name.0 == name {
            found = true;
            let new = change(&light);
            // A write is a change of the lighting for the renderer.
            if *light != new {
                *light = new;
            }
        }
    }
    if !found {
        tracing::warn!(name, "an event names a light that the room does not have");
    }
}
