//! The sound output. The only file of the engine that names `kira`: a change of the
//! output library changes this file and no other.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy_ecs::entity::Entity;
use bevy_ecs::resource::Resource;
use glam::{Quat, Vec3};
use kira::backend::mock::{MockBackend, MockBackendSettings};
use kira::listener::ListenerHandle;
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::sound::streaming::{StreamingSoundData, StreamingSoundHandle};
use kira::sound::{FromFileError, PlaybackState};
use kira::track::{SpatialTrackBuilder, SpatialTrackHandle};
use kira::{AudioManager, AudioManagerSettings, Decibels, DefaultBackend, Tween};
use maki_assets::{Assets, Handle};
use maki_scene::SoundSource;

/// A sound at a place starts and stops with a fade of this length.
const SOURCE_FADE: Duration = Duration::from_millis(500);
/// Music that stops, or that other music takes the place of, fades out in this time.
// ponytail: one length for all music. An argument of `StopMusic` if an event needs
// a hard stop or a long fade.
const MUSIC_FADE: Duration = Duration::from_secs(1);
/// With no device, the output renders this much time in one call: one buffer of 128
/// samples at `SILENT_RATE`.
const SILENT_STEP: Duration = Duration::from_nanos(1_000_000_000 / 60);
const SILENT_RATE: u32 = 128 * 60;

fn fade(duration: Duration) -> Tween {
    Tween {
        duration,
        ..Tween::default()
    }
}

/// A volume as a factor (1.0: as loud as the file) in the unit of the output.
fn decibels(volume: f32) -> Decibels {
    if volume <= 0.001 {
        Decibels::SILENCE
    } else {
        Decibels(20.0 * volume.log10())
    }
}

/// A sound file in memory: an asset. Get one with [`Audio::load`].
pub struct Sound {
    /// The file.
    pub path: PathBuf,
    data: StaticSoundData,
}

impl Sound {
    pub fn seconds(&self) -> f32 {
        self.data.duration().as_secs_f32()
    }
}

/// The output with a device, or with none: the same code runs for the two. Each is
/// some hundred bytes and more: in a box.
enum Manager {
    Device(Box<AudioManager<DefaultBackend>>),
    /// No device. `Audio::update` renders the sound into nothing, at the rate of the
    /// time of the game: a sound starts, fades, and ends as it does with a device.
    Silent(Box<AudioManager<MockBackend>>),
}

/// Calls a method that the manager has with each backend.
macro_rules! with_manager {
    ($manager:expr, $name:ident => $call:expr) => {
        match &mut $manager {
            Manager::Device($name) => $call,
            Manager::Silent($name) => $call,
        }
    };
}

/// A sound at a place that plays.
struct Voice {
    /// What plays: a change of the component starts the sound again.
    source: SoundSource,
    position: Vec3,
    track: SpatialTrackHandle,
    sound: StaticSoundHandle,
}

/// The sound output of the game: play a sound, start and stop music. A resource; it
/// stays when the room changes, so music goes on in the next room.
///
/// The sounds at a place (`maki_scene::SoundSource`) are not started here: the audio
/// module plays each entity that has the component.
#[derive(Resource)]
pub struct Audio {
    manager: Manager,
    listener: ListenerHandle,
    /// Where the listener is, as the output has it.
    heard: (Vec3, Quat),
    sounds: Assets<Sound>,
    /// The sound of each file that was asked for.
    files: HashMap<PathBuf, Handle<Sound>>,
    /// The sounds of `play` that have not ended.
    effects: Vec<(PathBuf, StaticSoundHandle)>,
    music: Option<(PathBuf, StreamingSoundHandle<FromFileError>)>,
    /// The volume that the music was started with: for a start of it again.
    music_volume: f32,
    sources: HashMap<Entity, Voice>,
    /// Sounds and music that fade out. Kept so that `stop_all` can end them at once.
    ending: Vec<StaticSoundHandle>,
    ending_music: Vec<StreamingSoundHandle<FromFileError>>,
    /// With no device: the time that is not rendered yet.
    owed: Duration,
}

impl Audio {
    /// `output` false: no device is opened. No device that can be opened: a warning,
    /// and the same.
    pub(crate) fn new(output: bool) -> Self {
        let device =
            output.then(|| AudioManager::<DefaultBackend>::new(AudioManagerSettings::default()));
        let mut manager = match device {
            Some(Ok(manager)) => Manager::Device(Box::new(manager)),
            other => {
                if let Some(Err(error)) = other {
                    tracing::warn!("no sound device ({error}): the game is silent");
                }
                let settings = AudioManagerSettings {
                    backend_settings: MockBackendSettings {
                        sample_rate: SILENT_RATE,
                    },
                    ..AudioManagerSettings::default()
                };
                Manager::Silent(Box::new(
                    AudioManager::new(settings).expect("needs no device"),
                ))
            }
        };
        let heard = (Vec3::ZERO, Quat::IDENTITY);
        let listener = with_manager!(manager, manager => manager.add_listener(heard.0.to_array(), heard.1.to_array()));
        Self {
            manager,
            listener: listener.expect("the first listener of a new manager"),
            heard,
            sounds: Assets::default(),
            files: HashMap::new(),
            effects: Vec::new(),
            music: None,
            music_volume: 1.0,
            sources: HashMap::new(),
            ending: Vec::new(),
            ending_music: Vec::new(),
            owed: Duration::ZERO,
        }
    }

    /// False: the game makes no sound (`output = false`, or no device).
    pub fn has_device(&self) -> bool {
        matches!(self.manager, Manager::Device(_))
    }

    /// The sound of a file (WAV or Ogg Vorbis). A file that is asked for two times
    /// is loaded one time. A failure is logged, and the sound never becomes ready:
    /// `play` then does nothing.
    // ponytail: the file is read and decoded on the calling thread, as a texture is.
    pub fn load(&mut self, path: &Path) -> Handle<Sound> {
        if let Some(handle) = self.files.get(path) {
            return *handle;
        }
        let start = std::time::Instant::now();
        let sound = match StaticSoundData::from_file(path) {
            Ok(data) => Some(Sound {
                path: path.to_owned(),
                data,
            }),
            Err(error) => {
                tracing::error!("cannot load sound {}: {error}", path.display());
                None
            }
        };
        if let Some(sound) = &sound {
            let (seconds, time) = (sound.seconds(), start.elapsed());
            tracing::debug!(path = %path.display(), seconds, ?time, "sound loaded");
        }
        let handle = self.sounds.reserve();
        self.sounds.finish(handle, sound);
        self.files.insert(path.to_owned(), handle);
        handle
    }

    /// The files that were loaded, and the file of the music: for the watch of the
    /// asset files.
    pub fn files(&self) -> Vec<PathBuf> {
        let music = self.music.iter().map(|(path, _)| path);
        self.files.keys().chain(music).cloned().collect()
    }

    /// Forgets a file that changed on the disk: the next load of it reads it again.
    /// A sound at a place that plays the file stops, and `set_sources` starts it
    /// with the new file. Music of the file starts again from its start. A sound
    /// of `play` goes on to its end.
    pub fn forget(&mut self, path: &Path) {
        if let Some(handle) = self.files.remove(path) {
            self.sounds.remove(handle);
        }
        let of_file = |voice: &Voice| voice.source.path == path;
        let voices = self.sources.iter().filter(|(_, voice)| of_file(voice));
        let playing: Vec<Entity> = voices.map(|(entity, _)| *entity).collect();
        for entity in playing {
            let mut voice = self.sources.remove(&entity).expect("found above");
            voice.sound.stop(fade(SOURCE_FADE));
            self.ending.push(voice.sound);
        }
        if self.music().is_some_and(|music| music == path) {
            self.stop_music();
            self.play_music(path, self.music_volume);
        }
    }

    /// `None` after a failed load, and after the play signal: it forgets all sounds.
    pub fn sound(&self, handle: Handle<Sound>) -> Option<&Sound> {
        self.sounds.get(handle)
    }

    /// Plays a sound one time. `volume` 1.0: as loud as the file is.
    pub fn play(&mut self, sound: Handle<Sound>, volume: f32) {
        let Some(sound) = self.sounds.get(sound) else {
            return;
        };
        let data = sound.data.volume(decibels(volume));
        match with_manager!(self.manager, manager => manager.play(data)) {
            Ok(handle) => self.effects.push((sound.path.clone(), handle)),
            Err(error) => tracing::warn!("sound {} not played: {error}", sound.path.display()),
        }
    }

    /// Starts a file as music: a loop, read from the file while it plays. It takes
    /// the place of the music that plays. The same file as the music that plays:
    /// nothing changes, the music goes on.
    pub fn play_music(&mut self, path: &Path, volume: f32) {
        if self.music().is_some_and(|playing| playing == path) {
            return;
        }
        self.stop_music();
        let data = match StreamingSoundData::from_file(path) {
            Ok(data) => data.loop_region(..).volume(decibels(volume)),
            Err(error) => {
                tracing::error!("cannot play music {}: {error}", path.display());
                return;
            }
        };
        match with_manager!(self.manager, manager => manager.play(data)) {
            Ok(handle) => {
                self.music = Some((path.to_owned(), handle));
                self.music_volume = volume;
            }
            Err(error) => tracing::warn!("music {} not played: {error}", path.display()),
        }
    }

    /// The music fades out.
    pub fn stop_music(&mut self) {
        if let Some((_, mut handle)) = self.music.take() {
            handle.stop(fade(MUSIC_FADE));
            self.ending_music.push(handle);
        }
    }

    /// The file of the music that plays.
    pub fn music(&self) -> Option<&Path> {
        self.music.as_ref().map(|(path, _)| path.as_path())
    }

    /// The files of the sounds of [`Audio::play`] that have not ended.
    pub fn effects(&self) -> Vec<&Path> {
        self.effects
            .iter()
            .map(|(path, _)| path.as_path())
            .collect()
    }

    /// The files of the sounds at a place that play, in the order of their names.
    pub fn sources(&self) -> Vec<&Path> {
        let voices = self.sources.values();
        let mut paths: Vec<&Path> = voices.map(|voice| voice.source.path.as_path()).collect();
        paths.sort();
        paths
    }

    /// The volume of all sound. 1.0: as loud as the files are.
    pub fn set_volume(&mut self, volume: f32) {
        let volume = decibels(volume);
        with_manager!(self.manager, manager => manager.main_track().set_volume(volume, Tween::default()));
    }

    /// Ends all sound now, and forgets the loaded files: a file that was changed on
    /// the disk is read again when it is asked for.
    pub fn stop_all(&mut self) {
        let (effects, sources) = (self.effects.len(), self.sources.len());
        if effects + sources > 0 || self.music.is_some() {
            let music = self.music.is_some();
            tracing::debug!(effects, sources, music, "all sound stops");
        }
        // Not a cut: a cut in the middle of a wave is a click.
        let quick = Tween::default();
        let effects = self.effects.drain(..).map(|(_, handle)| handle);
        let sources = self.sources.drain().map(|(_, voice)| voice.sound);
        for mut handle in effects.chain(sources).chain(self.ending.drain(..)) {
            handle.stop(quick);
        }
        let music = self.music.take().map(|(_, handle)| handle);
        for mut handle in music.into_iter().chain(self.ending_music.drain(..)) {
            handle.stop(quick);
        }
        for (_, handle) in self.files.drain() {
            self.sounds.remove(handle);
        }
    }

    /// Where the sounds at a place are heard from. With no turn, the right ear is
    /// at +X.
    pub(crate) fn set_listener(&mut self, position: Vec3, orientation: Quat) {
        if self.heard.0 != position {
            self.listener
                .set_position(position.to_array(), Tween::default());
        }
        if self.heard.1 != orientation {
            self.listener
                .set_orientation(orientation.to_array(), Tween::default());
        }
        self.heard = (position, orientation);
    }

    /// Makes the sounds at a place those of `wanted` (an entity, its component, its
    /// place): starts what is new, stops what is gone, moves the rest. A component
    /// that changed starts again.
    pub(crate) fn set_sources(&mut self, wanted: &[(Entity, &SoundSource, Vec3)]) {
        let stays = |entity: &Entity, voice: &Voice| {
            let same = |(wanted, source, _): &(Entity, &SoundSource, Vec3)| {
                wanted == entity && **source == voice.source
            };
            wanted.iter().any(same)
        };
        let gone: Vec<Entity> = self
            .sources
            .iter()
            .filter(|(entity, voice)| !stays(entity, voice))
            .map(|(entity, _)| *entity)
            .collect();
        for entity in gone {
            let mut voice = self.sources.remove(&entity).expect("found above");
            tracing::debug!(path = %voice.source.path.display(), "sound at a place stops");
            voice.sound.stop(fade(SOURCE_FADE));
            // The track of the voice stays until the fade has ended.
            self.ending.push(voice.sound);
        }
        for &(entity, source, position) in wanted {
            match self.sources.get_mut(&entity) {
                Some(voice) if voice.position != position => {
                    voice.position = position;
                    voice
                        .track
                        .set_position(position.to_array(), Tween::default());
                }
                Some(_) => {}
                None => self.start_source(entity, source, position),
            }
        }
    }

    fn start_source(&mut self, entity: Entity, source: &SoundSource, position: Vec3) {
        let handle = self.load(&source.path);
        let Some(sound) = self.sounds.get(handle) else {
            return;
        };
        // Full volume up to 1 m, silent at the reach.
        let reach = source.reach.max(0.01);
        let builder = SpatialTrackBuilder::new()
            .distances((reach.min(2.0) / 2.0, reach))
            // The fade of a stop is heard to its end.
            .persist_until_sounds_finish(true);
        let listener = self.listener.id();
        let track = with_manager!(self.manager, manager => manager.add_spatial_sub_track(listener, position.to_array(), builder));
        let data = sound
            .data
            .loop_region(..)
            .volume(decibels(source.volume))
            .fade_in_tween(fade(SOURCE_FADE));
        let played = track
            .map_err(|error| error.to_string())
            .and_then(|mut track| {
                let sound = track.play(data).map_err(|error| error.to_string())?;
                Ok((track, sound))
            });
        match played {
            Ok((track, sound)) => {
                tracing::debug!(path = %source.path.display(), "sound at a place starts");
                let source = source.clone();
                self.sources.insert(
                    entity,
                    Voice {
                        source,
                        position,
                        track,
                        sound,
                    },
                );
            }
            Err(error) => tracing::warn!("sound {} not played: {error}", source.path.display()),
        }
    }

    /// One frame: forgets the sounds that have ended. With no device, renders the
    /// time of the frame.
    pub(crate) fn update(&mut self, delta: Duration) {
        if let Manager::Silent(manager) = &mut self.manager {
            self.owed += delta;
            while self.owed >= SILENT_STEP {
                self.owed -= SILENT_STEP;
                let backend = manager.backend_mut();
                backend.on_start_processing();
                backend.process();
            }
        }
        let ended = |state| state == PlaybackState::Stopped;
        self.effects.retain(|(_, handle)| !ended(handle.state()));
        self.ending.retain(|handle| !ended(handle.state()));
        self.ending_music.retain(|handle| !ended(handle.state()));
        if let Some((path, handle)) = &mut self.music {
            // A file that breaks while it plays: the reader thread gives the error.
            if let Some(error) = handle.pop_error() {
                tracing::error!("music {}: {error}", path.display());
            }
            if ended(handle.state()) {
                self.music = None;
            }
        }
    }
}
