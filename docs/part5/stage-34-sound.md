# Stage 34: sound

Plan: [README.md](README.md). Report: `docs/part5/stage-34.md`. Item: M7 of [`../part4/MISSING.md`](../part4/MISSING.md).

Goal: the game has sound. An event plays a sound and starts and stops music; a thing of the room hums at its place.
Gate: the switch of room B clicks; the lamp of room A hums and stops with the power; stop in the editor ends all sound.

Written after the report of stage 33, from the code at `26d0c5d`.

Decisions of the owner for this stage:
- The sound files are placeholders that a script in `tools/` makes (a short click, a hum that loops). The owner puts real sounds in their place later.
- A check with no ears is accepted: the files decode, the right sound starts and stops at the right event (log and audio state), and stop in the editor ends all sound. The report says that nobody listened.

## Context in the code

### The output crate

- `kira` 0.12.5 (checked in its source, not from memory):
  - `AudioManager<B: Backend>`; `AudioManager<CpalBackend>` and each handle are `Send + Sync`, so the manager can be in a resource.
  - `StaticSoundData::from_file` decodes a file into memory; a clone shares the samples. `StreamingSoundData::from_file` decodes while it plays.
  - `manager.add_listener(position, orientation)` and `manager.add_spatial_sub_track(listener, position, SpatialTrackBuilder)`: a track at a place, with `distances((min, max))` and `persist_until_sounds_finish`. With no turn, the right ear of a listener is at +X.
  - A volume is `Decibels`; -60 dB is silence. A `Tween` is a fade. The handle of a static sound has no volume setter: the volume is a setting of the data before `play`.
  - `backend::mock::MockBackend` connects to no device. `process()` renders one internal buffer; the state of a sound changes only then.
- Features: `cpal`, `wav` with `pcm`, `ogg` with `vorbis`. No `mp3`, no `flac`.

### Where a sound starts

- A module adds commands with `CommandRegistry::add`, a check with `add_check`, texts with `add_texts` (`game/src/shake.rs` is the example). The handler has the world.
- `CheckContext` (`crates/events/src/check.rs`) has the room and the name of its file, not its directory: the check of a file name needs it.
- A path of a room file starts at the directory of the room file (`MeshDef::File`, `TextureDef::path`, `Transfer`): `CurrentRoom::path`.
- `Light` and `LightDef` (`crates/scene`): a light has a name, events find it by the name (`set_light` in `builtin.rs`), and `patch_room` changes a light in place.
- Commands that do not wait run in one step (`RunningEvent::step`): a command with no wait in the `flicker` list of room A does not change the time of the list.
- An `Autorun` event blocks the player for one step. The trip script counts frames: a new `Autorun` event on its way changes the pictures.

### Play and stop

- `ResetPlay` (`maki_core`): the editor runs it on play and on stop, the game at its start.
- The editor sets `Time::paused` while it does not play (`crates/editor/src/lib.rs:753`): no fixed step runs, `Update` goes on. The entities of the room exist in the two states.

### The editor

- `widgets::file` (`crates/editor/src/widgets.rs`) lists the files of the directory of the room file and one level below it. It is `pub(crate)`; the directory comes from `set_room_directory`, which only the property panel calls.
- `add_argument_editor` (`event_panel.rs:196`) gives the widgets of a command of a module. The closure gets the command only: it cannot offer the names of the room.
- `new_command` (`event_panel.rs:483`) makes a new built-in command. A command of a module with arguments becomes a text field (`PlaySound(`).
- `Document::renamed` (`ops.rs:145`) gives the built-in commands the new name of a light.

### The benchmark

- `tools/bench.py` writes a config for each scenario (`run_scenario`). The game of a benchmark must be silent, and its work must not depend on a sound device.

## Prompt

```text
Stage 34: sound.
Read docs/part5/stage-34-sound.md and item 7 of docs/part4/MISSING.md first.

Crate audio
- crates/audio, after events and before gameplay in the crate order. No
  dependency on gpu, render, ui. No kira type in its public API.
- A sound file is an asset with a handle; a file that is asked for two
  times is loaded one time. Music is streamed, not loaded.
- A resource with the output: play a sound one time, start music as a
  loop (in place of the music that plays), stop the music, stop all.
  It tells what plays (for a test, a log, a tool).
- Settings [audio]: volume, and output (false: no device; the module
  does the same work with no sound). No device at the start: a
  warning, and the same as output = false.
- ResetPlay stops all sound and forgets the loaded files.

A sound at a place
- A component of the scene crate: a sound file that loops at the place
  of its entity, with a volume, a reach in meters, and on or off.
- The room file: a light can have a sound.
- The audio module plays each such entity while it is on and the game
  is not paused, louder near the player, and from the side where the
  picture shows it. A start and a stop are fades.

Commands, from the audio module
- PlaySound(file, volume), PlayMusic(file, volume), StopMusic,
  SoundOn(name), SoundOff(name). A file name starts at the directory
  of the room file.
- Checks of the loader: a file that does not exist is a warning; a
  name with no sound in the room is an error.
- Row texts and help.

Editor
- The property panel of a light: the sound, with a file picker.
- The event panel: the audio commands are added from the "Add" list
  with arguments that exist, and their arguments are a file picker, a
  number, and a list of the names with a sound. No text field.
- These use seams that a game can use for its own commands.
- A rename of a light changes SoundOn and SoundOff with it.

Content
- tools/gen_sounds.py makes the placeholder files.
- Room B: the switch clicks. Room A: the lamp hums, and the hum stops
  when the power is off.

Out of scope: a sound on a prop, a sound item of its own in the room,
reverb and other effects, volume groups, a sound that follows the
intensity of a light, footsteps, a settings panel.

Done when: a test runs the commands of a room file with no device and
the audio state shows each start and stop; a test turns the sound of a
light off by an event and the play signal ends all sound; the trip
script logs the click at the switch and the stop of the hum in room A;
trip_720p is byte-equal. Add the crate to CLAUDE.md (crate list, rule
1) and to README.md. Report.
```

## What to watch for

- **The picture.** Sound must not change it: `bench.py same trip_720p s33 s34`. The hum of room A stops with a command in the `flicker` list, not with a new `Autorun` event.
- **No device.** `cargo test` opens no sound device and makes no sound. The benchmark runs with `audio.output = false`.
- **A sound that ends.** A click that has ended is not kept: 200 clicks do not fill the 128 places of a track.
- **The hum on the way back.** Room A with the power off: the hum must not be heard for the frames between the load of the room and the command that stops it.
- **Stop in the editor.** After stop: no music, no click, no hum, also while the editor shows a room with a lamp that hums.
- **A write in each frame.** The audio systems write no component of the scene: `MAKI_LOG=info,maki_render=trace` shows no `a light changed` at rest.
- **A file that changed.** The owner puts a real sound in place of a placeholder while the editor is open: the next play has the new file.
- **A long frame.** A sound file is decoded on the calling thread. Note the time of the two files of the game.
