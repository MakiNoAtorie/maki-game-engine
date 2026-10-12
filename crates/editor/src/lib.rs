//! The room editor: a room file is opened, inspected, changed, saved, and played, in
//! the window of the game.
//!
//! - `document`: what is edited. The content of one room file: the room, its events,
//!   its comments. The panels change the document and nothing else.
//! - `ron_text`: the text of a room file, with line breaks and comments as a person
//!   writes them.
//! - This file: the [`Editor`] resource and its commands (open, save, play, stop).
//!   In each frame the world is made to show the document: a small change is
//!   patched into the entities (`maki_scene::patch_room`), each other change loads
//!   the room again from the text of the document.
//! - `camera`: the free camera. `panels`, `widgets`: the outliner and the property
//!   panel.
//!
//! Edit and play: while the room is edited, the simulation stands still
//! (`Time::paused`), the keys are those of the editor ([`MODE`]), the player does not
//! show, and the picture is that of the editor camera. Play gives all four back to
//! the game; stop takes them again and loads the document, so nothing that the game
//! did stays.
//!
//! This crate is the last one before the game. A game adds [`EditorModule`] after
//! its own modules, behind a cargo feature: a build without it has no editor code.

mod camera;
mod document;
mod panels;
mod ron_text;
mod widgets;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::Context;
use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::query::{Has, With};
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::ResMut;
use bevy_ecs::world::{Mut, World};
use maki_core::{App, Input, Interpolated, Module, Startup, Time, Update};
use maki_events::{CommandRegistry, EventRunner, GameState, MessageBox};
use maki_gameplay::{MoveCapture, StartRoom};
use maki_platform::{
    Action, ActionState, InputMode, Key, QuitHold, QuitRequested, add_mode_bindings,
};
use maki_scene::{
    ActiveCamera, Camera, CurrentRoom, MeshInstance, Player, RoomDef, RoomRequest, RoomTarget,
    RoomValidators, ScreenFade, load_requested_room, patch_room,
};
use maki_ui::{Panels, TOGGLE_HELP};

pub use camera::{EditorCamera, ORBIT, PAN, VIEW_GAME, VIEW_TOP, ZOOM_IN, ZOOM_OUT};
pub use document::{Document, Item};
pub use ron_text::{Notes, pretty, read_notes};

/// The input mode while a room is edited: the keys of the game start nothing.
pub const MODE: InputMode = InputMode("editor");
/// Plays the room. To stop: the quit action of the game, or the close button.
pub const PLAY: Action = Action("editor_play");

/// What the user asks the editor to do. The panels and the keys put commands into a
/// queue ([`Editor::request`]); the editor runs them in the next frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// A new, empty room.
    New,
    Open(PathBuf),
    Save,
    /// Saves the room to another file, which then is the open one.
    SaveAs(PathBuf),
    Play,
    Stop,
    /// Ends the app.
    Quit,
    /// The answer to the question that a command asked for a document with changes
    /// that are not saved.
    Answer(Answer),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// Save, then do it.
    Save,
    /// Do it. The changes are lost, but for `Play`: the game plays the room as it is
    /// in the editor.
    DoNotSave,
    /// Do not do it.
    Cancel,
}

/// The state of the editor.
#[derive(Resource)]
pub struct Editor {
    pub document: Document,
    /// What the property panel shows.
    pub selection: Item,
    /// The spawn point where the game starts on `Play`. `None`, or a name that the
    /// room does not have: the first one.
    pub play_from: Option<String>,
    /// One line for the user: what the last command did, or why it did not.
    pub status: String,
    /// The directory of the room files of the game.
    rooms: PathBuf,
    queue: Vec<Command>,
    /// A command that waits for an [`Answer`].
    pending: Option<Command>,
    /// `Some` while the game is played: the game state of before.
    playing: Option<GameState>,
    /// The room that the world shows. `None`: load the document.
    shown: Option<(PathBuf, RoomDef)>,
}

impl Editor {
    pub fn request(&mut self, command: Command) {
        self.queue.push(command);
    }

    pub fn playing(&self) -> bool {
        self.playing.is_some()
    }

    /// The command that waits for an answer.
    pub fn pending(&self) -> Option<&Command> {
        self.pending.as_ref()
    }

    /// The directory of the room files.
    pub fn rooms(&self) -> &Path {
        &self.rooms
    }

    fn run(&mut self, world: &mut World, command: Command, confirmed: bool) {
        if self.playing() && command != Command::Stop {
            return;
        }
        let asks = matches!(
            command,
            Command::New | Command::Open(_) | Command::Play | Command::Quit
        );
        if asks && self.document.dirty && !confirmed {
            self.pending = Some(command);
            return;
        }
        match command {
            Command::New => {
                self.open(Document::new_room(self.free_path()));
                self.status = "New room".to_owned();
            }
            Command::Open(path) => {
                let registry = world.resource::<CommandRegistry>();
                let checks = world.resource::<RoomValidators>();
                match Document::read(&path, registry, checks) {
                    Ok(document) => {
                        self.open(document);
                        self.status = "Opened".to_owned();
                    }
                    Err(error) => self.fail(format!("{error:#}")),
                }
            }
            Command::Save => self.save(),
            Command::SaveAs(path) => {
                self.document.path = path;
                self.save();
            }
            Command::Play => self.play(world),
            Command::Stop => self.stop(world),
            Command::Quit => {
                world.resource_mut::<QuitHold>().0 = false;
                world.resource_mut::<QuitRequested>().0 = true;
            }
            Command::Answer(answer) => {
                let Some(pending) = self.pending.take() else {
                    return;
                };
                if answer == Answer::Save {
                    self.save();
                }
                // A save that failed: the changes are still there, so not done.
                let go = answer == Answer::DoNotSave || !self.document.dirty;
                if answer != Answer::Cancel && go {
                    self.run(world, pending, true);
                }
            }
        }
    }

    fn open(&mut self, document: Document) {
        self.document = document;
        self.selection = Item::Room;
        self.shown = None;
    }

    fn save(&mut self) {
        match self.document.save() {
            Ok(()) => self.status = "Saved".to_owned(),
            Err(error) => self.fail(format!("{error:#}")),
        }
    }

    fn fail(&mut self, error: String) {
        tracing::error!("{error}");
        self.status = error;
    }

    /// A file name in the room directory that no file has.
    fn free_path(&self) -> PathBuf {
        let name = |number: u32| match number {
            1 => "untitled.ron".to_owned(),
            number => format!("untitled_{number}.ron"),
        };
        let mut paths = (1..).map(|number| self.rooms.join(name(number)));
        paths
            .find(|path| !path.exists())
            .expect("the range has no end")
    }

    fn play(&mut self, world: &mut World) {
        self.playing = Some(world.resource::<GameState>().clone());
        reset_play_state(world);
        let spawns = &self.document.room.spawns;
        let chosen = self
            .play_from
            .as_ref()
            .filter(|name| spawns.contains_key(*name));
        let spawn = chosen.or_else(|| spawns.keys().next()).cloned();
        if spawn.is_none() {
            // A room with no spawn point: the player starts where the editor looks.
            let mut cameras = world.query::<&EditorCamera>();
            let target = cameras
                .iter(world)
                .next()
                .map_or(glam::Vec3::ZERO, |c| c.target);
            let mut players = world.query_filtered::<&mut Interpolated, With<Player>>();
            for mut pose in players.iter_mut(world) {
                let mut at = pose.current;
                at.translation = target.with_y(0.0);
                pose.snap(at);
            }
        }
        // From the text of the document, not from the file: the room as it is here.
        world.resource_mut::<RoomRequest>().0 = Some(RoomTarget {
            path: self.document.path.clone(),
            spawn,
            text: Some(self.document.text()),
        });
        self.status = "Playing".to_owned();
    }

    fn stop(&mut self, world: &mut World) {
        let Some(state) = self.playing.take() else {
            return;
        };
        *world.resource_mut::<GameState>() = state;
        reset_play_state(world);
        // The world shows what the game made of the room, or another room.
        self.shown = None;
        self.status = "Stopped".to_owned();
    }

    /// Makes the world show the document, while the room is edited.
    fn sync(&mut self, world: &mut World) {
        let (path, room) = (&self.document.path, &self.document.room);
        let shown = self.shown.as_ref();
        if self.playing() || shown.is_some_and(|shown| shown.0 == *path && shown.1 == *room) {
            return;
        }
        let text = self.document.text();
        // A change of some values needs no load: the picture follows a slider with
        // no stop and no noise. Each other change loads the room again, and each mesh
        // of the room is built again on the GPU.
        let patched = shown.is_some() && patch_room(world, room, text.clone());
        if !patched {
            world.resource_mut::<RoomRequest>().0 = Some(RoomTarget {
                path: path.clone(),
                spawn: None,
                text: Some(text),
            });
        }
        let shown = Some((path.clone(), room.clone()));
        self.shown = shown;
    }

    /// Gives the keys, the clock, the player, and the picture to the editor or to
    /// the game. Runs in each frame: each part is written only if it differs.
    fn apply_mode(&self, world: &mut World) {
        let playing = self.playing();
        let mode = if playing { InputMode::GAME } else { MODE };
        world.resource_mut::<InputMode>().set_if_neq(mode);
        if world.resource::<Time>().paused == playing {
            world.resource_mut::<Time>().paused = !playing;
        }

        // The player is not a part of the room: it does not show in the editor.
        let mut shown = world.query_filtered::<(Entity, &MeshInstance), With<Player>>();
        let mut parked = world.query::<(Entity, &Parked)>();
        if playing {
            let players: Vec<_> = parked
                .iter(world)
                .map(|(entity, mesh)| (entity, mesh.0))
                .collect();
            for (entity, mesh) in players {
                world.entity_mut(entity).remove::<Parked>().insert(mesh);
            }
        } else {
            let players: Vec<_> = shown
                .iter(world)
                .map(|(entity, mesh)| (entity, *mesh))
                .collect();
            for (entity, mesh) in players {
                world
                    .entity_mut(entity)
                    .remove::<MeshInstance>()
                    .insert(Parked(mesh));
            }
        }

        let mut cameras =
            world.query_filtered::<(Entity, Has<EditorCamera>, Has<ActiveCamera>), With<Camera>>();
        let cameras: Vec<_> = cameras.iter(world).collect();
        for (entity, of_editor, active) in cameras {
            match (of_editor != playing, active) {
                (true, false) => world.entity_mut(entity).insert(ActiveCamera),
                (false, true) => world.entity_mut(entity).remove::<ActiveCamera>(),
                _ => continue,
            };
        }

        let ask = self.pending.is_some();
        let mut panels = world.resource_mut::<Panels>();
        if panels.is_open(panels::CONFIRM) != ask {
            panels.set_open(panels::CONFIRM, ask);
        }
    }
}

/// The mesh of the player while the room is edited.
#[derive(Component)]
struct Parked(MeshInstance);

/// What a game leaves behind, beside the game state and the room: an event that
/// runs, a message, a fade, the move keys in the hands of a debug tool.
fn reset_play_state(world: &mut World) {
    *world.resource_mut::<EventRunner>() = EventRunner::default();
    world
        .resource_mut::<MessageBox>()
        .set_if_neq(MessageBox(None));
    world
        .resource_mut::<ScreenFade>()
        .set_if_neq(ScreenFade(0.0));
    if let Some(mut capture) = world.get_resource_mut::<MoveCapture>() {
        capture.set_if_neq(MoveCapture(false));
    }
}

/// Adds the editor. The app then starts in the editor, with the start room of the
/// game open. Add it after the modules of the game. Needs `maki_ui::UiModule`,
/// `maki_events::EventsModule`, and `maki_gameplay::RoomsModule`.
pub struct EditorModule {
    /// The directory of the room files of the game: for "Open", "Save as", and new
    /// rooms.
    pub rooms: PathBuf,
}

impl Module for EditorModule {
    fn build(&self, app: &mut App) {
        let start = app.world().resource::<StartRoom>().clone();
        let mut bindings = camera::BINDINGS.to_vec();
        bindings.push((Key::F5, PLAY, "Play the room"));
        bindings.push((Key::F1, TOGGLE_HELP, "Toggle help panel"));
        add_mode_bindings(app, MODE, &bindings);
        // A quit request is a command of the editor: it asks for changes that are
        // not saved, and in a game it is a stop.
        app.resource_mut::<QuitHold>().0 = true;
        let mut document = Document::new_room(self.rooms.join("untitled.ron"));
        document.dirty = false;
        app.insert_resource(Editor {
            document,
            selection: Item::Room,
            play_from: Some(start.spawn),
            status: String::new(),
            rooms: self.rooms.clone(),
            // In the first frame: all modules have added their event commands then.
            queue: vec![Command::Open(start.path)],
            pending: None,
            playing: None,
            shown: None,
        })
        .add_systems(Startup, camera::spawn)
        .add_systems(Input, take_quit_request)
        .add_systems(
            Update,
            (
                // It has the full world: a command can load a room and stop a game.
                // Before the loader: a room that it asks for shows in this frame.
                run_commands
                    .before(load_requested_room)
                    .ambiguous_with_all(),
                camera::move_camera,
            ),
        );
        panels::build(app);
    }
}

/// The command line options of the editor, for the usage text of a game.
pub const USAGE: &str = "[--editor] [--editor-resave ROOM]...";

/// What the command line asks of the editor.
#[derive(Debug, Default, PartialEq)]
pub struct EditorArgs {
    /// `--editor`: the game starts in the editor. Add [`EditorModule`].
    pub open: bool,
    /// `--editor-resave ROOM`, one or more times: call [`resave`] with the files,
    /// and end.
    pub resave: Vec<PathBuf>,
}

/// Takes the editor options from the command line. Returns them and the other
/// arguments, as `maki_devtools::parse_args` does for the benchmark options.
pub fn parse_args(
    args: impl IntoIterator<Item = OsString>,
) -> anyhow::Result<(EditorArgs, Vec<OsString>)> {
    let (mut editor, mut rest) = (EditorArgs::default(), Vec::new());
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--editor" {
            editor.open = true;
        } else if arg == "--editor-resave" {
            let path = args.next().context("--editor-resave needs a room file")?;
            editor.resave.push(PathBuf::from(path));
        } else {
            rest.push(arg);
        }
    }
    Ok((editor, rest))
}

/// Reads each room file and writes it again as the editor saves it: the same room,
/// events, and comments, with the line breaks and the field order of the editor. For
/// room files that a person or another tool wrote. `app` has the modules of the
/// game: the files are read with their event commands.
pub fn resave(app: &App, paths: &[PathBuf]) -> anyhow::Result<()> {
    let world = app.world();
    let registry = world.resource::<CommandRegistry>();
    let checks = world.resource::<RoomValidators>();
    for path in paths {
        Document::read(path, registry, checks)?.save()?;
        tracing::info!(path = %path.display(), "room file written");
    }
    Ok(())
}

/// Runs in `Input`: a quit request of the frame before (the close button of the
/// window, the quit action of the game) becomes a command.
fn take_quit_request(mut quit: ResMut<QuitRequested>, mut editor: ResMut<Editor>) {
    if std::mem::take(&mut quit.0) {
        let command = if editor.playing() {
            Command::Stop
        } else {
            Command::Quit
        };
        editor.request(command);
    }
}

/// Runs in `Update`: the commands of the queue, then the world follows the document.
fn run_commands(world: &mut World) {
    world.resource_scope(|world, mut editor: Mut<Editor>| {
        let actions = world.resource::<ActionState>();
        if actions.just_pressed(PLAY) {
            editor.request(Command::Play);
        }
        for command in std::mem::take(&mut editor.queue) {
            editor.run(world, command, false);
        }
        editor.sync(world);
        editor.apply_mode(world);
        // The room that the game is in, for the panel.
        if editor.playing() {
            let room = &world.resource::<CurrentRoom>().path;
            let name = room.file_name().unwrap_or(room.as_os_str());
            let status = format!("Playing {}", name.to_string_lossy());
            if editor.status != status {
                editor.status = status;
            }
        }
    });
}

#[cfg(test)]
mod tests;
