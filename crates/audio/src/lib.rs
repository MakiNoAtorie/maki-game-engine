//! Sound: effects, music, and sounds at a place in the room.
//!
//! - [`Audio`] is the output, a resource: `load` a sound file one time and `play` it,
//!   `play_music`, `stop_music`. It also tells what plays.
//! - A sound at a place is data of the scene crate (`maki_scene::SoundSource`, from
//!   the room file). This module plays each entity that has one, as the renderer
//!   draws each mesh: in `Extract`, from the state of the frame. A command or a
//!   gameplay system turns such a sound on and off with a write to the component.
//! - [`AudioCommand`]: the event commands (`PlaySound`, `PlayMusic`, `StopMusic`,
//!   `SoundOn`, `SoundOff`).
//!
//! This crate must not depend on `gpu`, `render`, or `ui`. No type of the output
//! library is in its public API.

mod commands;
mod output;

use bevy_ecs::change_detection::DetectChanges;
use bevy_ecs::entity::Entity;
use bevy_ecs::query::With;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Query, Res, ResMut};
use glam::{Mat3, Quat, Vec3};
use maki_core::{App, Extract, Module, ResetPlay, Settings, Time, Transform};
use maki_events::CommandRegistry;
use maki_scene::{ActiveCamera, Camera, Player, SoundSource};
use serde::{Deserialize, Serialize};

pub use commands::{AudioCommand, sound_names};
pub use output::{Audio, Sound};

/// The `[audio]` table of the config file.
#[derive(Resource, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AudioSettings {
    /// The volume of all sound. 1.0: as loud as the files are. 0.0: silent.
    pub volume: f32,
    /// False: the game opens no sound device. It does the same work with no sound:
    /// for a benchmark and for a test.
    pub output: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            volume: 1.0,
            output: true,
        }
    }
}

impl Settings for AudioSettings {
    const TABLE: &'static str = "audio";

    fn check(&self) -> Result<(), String> {
        if !(self.volume.is_finite() && self.volume >= 0.0) {
            return Err("volume must be zero or a positive number".to_owned());
        }
        Ok(())
    }
}

/// Adds the sound output. With `maki_events::EventsModule` before it, it adds the
/// commands of [`AudioCommand`].
pub struct AudioModule;

impl Module for AudioModule {
    fn build(&self, app: &mut App) {
        app.add_settings::<AudioSettings>();
        let output = app.world().resource::<AudioSettings>().output;
        app.insert_resource(Audio::new(output))
            // After `Update`: the sound of a frame is that of the state that the
            // frame shows, as its picture is.
            .add_systems(Extract, play_frame)
            // A game that starts, or that a tool stops, is silent.
            .add_systems(ResetPlay, |mut audio: ResMut<Audio>| audio.stop_all());
        if let Some(registry) = app.world().get_resource::<CommandRegistry>() {
            commands::add(registry);
        }
    }
}

/// Runs in `Extract`: the sounds at a place, the listener, the volume.
fn play_frame(
    mut audio: ResMut<Audio>,
    settings: Res<AudioSettings>,
    time: Res<Time>,
    sources: Query<(Entity, &SoundSource, &Transform)>,
    players: Query<&Transform, With<Player>>,
    cameras: Query<&Camera, With<ActiveCamera>>,
) {
    if settings.is_changed() {
        audio.set_volume(settings.volume);
    }
    if let Some(camera) = cameras.iter().next() {
        // The ears are at the player, and turned as the picture is: a sound at the
        // right of the player in the picture comes from the right.
        let player = players.iter().next();
        let position = player.map_or(camera.target, |player| player.translation);
        audio.set_listener(position, facing(camera));
    }
    // A game that stands still (a tool that does not play) is silent.
    let plays = |source: &SoundSource| source.on && !time.paused;
    let wanted: Vec<(Entity, &SoundSource, Vec3)> = sources
        .iter()
        .filter(|(_, source, _)| plays(source))
        .map(|(entity, source, transform)| (entity, source, transform.translation))
        .collect();
    audio.set_sources(&wanted);
    audio.update(time.delta);
}

/// The turn of a listener that looks as the camera does: its +X is the right side
/// of the picture.
fn facing(camera: &Camera) -> Quat {
    let forward = (camera.target - camera.position).normalize_or(Vec3::NEG_Z);
    // The top of the picture is up, or -Z for a camera that looks straight down.
    let up = if forward.y.abs() > 0.999 {
        Vec3::NEG_Z
    } else {
        Vec3::Y
    };
    let right = forward.cross(up).normalize();
    Quat::from_mat3(&Mat3::from_cols(right, right.cross(forward), -forward))
}

#[cfg(test)]
mod tests;
