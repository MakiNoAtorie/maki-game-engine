//! The event commands of this crate. They are added to the registry as the commands
//! of a game are (`game/src/shake.rs`): an enum, a handler, a check, texts.

use std::path::{Path, PathBuf};

use bevy_ecs::world::World;
use maki_events::{CheckContext, CommandRegistry, Finding, Step};
use maki_scene::{CurrentRoom, Name, ObjectId, ObjectRef, RoomDef, SoundSource};
use serde::{Deserialize, Serialize};

use crate::Audio;

/// A file name starts at the directory of the room file. A volume of 1.0 is as loud
/// as the file is. No command waits: the event goes on while the sound plays.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AudioCommand {
    /// Plays a sound file one time: `PlaySound("sounds/click.wav", 1.0)`.
    PlaySound(PathBuf, f32),
    /// Starts a file as music, as a loop, in place of the music that plays. The
    /// music stays when the room changes. The file of the music that plays: no change.
    PlayMusic(PathBuf, f32),
    /// The music fades out.
    StopMusic,
    /// Turns the sound of a light of the room on.
    SoundOn(ObjectRef),
    SoundOff(ObjectRef),
}

/// The names of the things of a room that have a sound: what `SoundOn` and
/// `SoundOff` can name.
pub fn sound_names(room: &RoomDef) -> Vec<String> {
    let lights = room.each_light().into_iter();
    let with_sound = lights.filter(|light| light.sound.is_some());
    with_sound.filter_map(|light| light.name).collect()
}

pub(crate) fn add(registry: &CommandRegistry) {
    registry.add(run);
    registry.add_check(check);
    registry.add_texts(row_text, help);
    registry.add_defaults(new);
}

fn run(command: &AudioCommand, step: &mut Step, world: &mut World) -> bool {
    let in_room = |world: &World, file: &Path| {
        let room = &world.resource::<CurrentRoom>().path;
        room.parent().unwrap_or(Path::new("")).join(file)
    };
    match command {
        AudioCommand::PlaySound(file, volume) => {
            let path = in_room(world, file);
            tracing::debug!(event = step.event, file = %file.display(), "sound");
            let mut audio = world.resource_mut::<Audio>();
            let sound = audio.load(&path);
            audio.play(sound, *volume);
        }
        AudioCommand::PlayMusic(file, volume) => {
            let path = in_room(world, file);
            tracing::debug!(event = step.event, file = %file.display(), "music");
            world.resource_mut::<Audio>().play_music(&path, *volume);
        }
        AudioCommand::StopMusic => {
            tracing::debug!(event = step.event, "music stops");
            world.resource_mut::<Audio>().stop_music();
        }
        AudioCommand::SoundOn(name) => set_source(world, name, true, step.event),
        AudioCommand::SoundOff(name) => set_source(world, name, false, step.event),
    }
    true
}

/// Writes the component only: the audio module starts and stops the sound.
fn set_source(world: &mut World, name: &ObjectRef, on: bool, event: &str) {
    let mut found = false;
    let mut sources = world.query::<(Option<&ObjectId>, Option<&Name>, &mut SoundSource)>();
    for (id, source_name, mut source) in sources.iter_mut(world) {
        if !name.is(id, source_name.map(|name| name.0.as_str())) {
            continue;
        }
        found = true;
        // An event in a loop gives the command again and again: one write.
        if source.on != on {
            source.on = on;
            tracing::debug!(event, %name, on, "sound of the room");
        }
    }
    if !found {
        tracing::warn!(%name, "an event names a sound that the room does not have");
    }
}

fn check(command: &AudioCommand, context: &CheckContext) -> Option<Finding> {
    match command {
        // A warning: the game runs with no sound, and a room is made before its sounds.
        AudioCommand::PlaySound(file, _) | AudioCommand::PlayMusic(file, _) => {
            let missing = !context.directory.join(file).is_file();
            missing.then(|| Finding::warning(format!("no sound file `{}`", file.display())))
        }
        AudioCommand::SoundOn(name) | AudioCommand::SoundOff(name) => {
            let with_sound = |(_, object): (usize, &maki_scene::ObjectDef)| {
                object.light()?.ok()?.sound.is_some().then_some(())
            };
            let missing = context.room.find(name).and_then(with_sound).is_none();
            missing.then(|| Finding::error(format!("the room has no light `{name}` with a sound")))
        }
        AudioCommand::StopMusic => None,
    }
}

fn row_text(command: &AudioCommand) -> Option<String> {
    Some(match command {
        AudioCommand::PlaySound(file, _) => format!("Play the sound {}", file.display()),
        AudioCommand::PlayMusic(file, _) => format!("Music: {}", file.display()),
        AudioCommand::StopMusic => "Stop the music".to_owned(),
        AudioCommand::SoundOn(name) => format!("Sound of {name}: on"),
        AudioCommand::SoundOff(name) => format!("Sound of {name}: off"),
    })
}

fn help(name: &str) -> Option<&'static str> {
    Some(match name {
        "PlaySound" => "Plays a sound file one time. The event does not wait for its end.",
        "PlayMusic" => "Starts a file as music, in place of the music that plays.",
        "StopMusic" => "The music fades out.",
        "SoundOn" => "Turns the sound of a light of the room on.",
        "SoundOff" => "Turns the sound of a light of the room off.",
        _ => return None,
    })
}

/// A new command for a tool (`CommandRegistry::add_defaults`).
fn new(name: &str, room: &RoomDef) -> Option<AudioCommand> {
    // The first light with a sound, by its id if it has one.
    let first = || {
        let name = sound_names(room).into_iter().next().unwrap_or_default();
        let found = room
            .object_named(&name)
            .and_then(|(_, object)| object.reference());
        found.unwrap_or(ObjectRef::Name(name))
    };
    Some(match name {
        "PlaySound" => AudioCommand::PlaySound(PathBuf::new(), 1.0),
        "PlayMusic" => AudioCommand::PlayMusic(PathBuf::new(), 1.0),
        "StopMusic" => AudioCommand::StopMusic,
        "SoundOn" => AudioCommand::SoundOn(first()),
        "SoundOff" => AudioCommand::SoundOff(first()),
        _ => return None,
    })
}
