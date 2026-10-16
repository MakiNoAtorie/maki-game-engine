//! The argument widgets of the commands of the audio module (`maki_audio`). They use
//! what a game has for its own commands: `add_argument_editor` and the public
//! widgets. The "Add" list makes a new audio command with the defaults that the
//! audio module gave to the command registry.

use maki_audio::{AudioCommand, sound_names};
use maki_core::App;

use crate::add_argument_editor;
use crate::widgets::{SOUNDS, file, number, object_text, pick};

pub(crate) fn add_argument_editors(app: &mut App) {
    add_argument_editor(app, |ui, command: &mut AudioCommand, room| match command {
        AudioCommand::PlaySound(path, volume) | AudioCommand::PlayMusic(path, volume) => {
            file(ui, "sound file", path, &SOUNDS) | number(ui, "volume", volume, 0.01, 0.0..=4.0)
        }
        AudioCommand::StopMusic => false,
        AudioCommand::SoundOn(name) | AudioCommand::SoundOff(name) => object_text(name, |name| {
            pick(ui, "light with a sound", name, &sound_names(room))
        }),
    });
}
