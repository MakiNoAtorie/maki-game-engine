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
//! - `view`: the pixel of a point, the ray of a pixel, the pick. `gizmo`: the handles
//!   that move, turn, and scale the selection. `overlay`: the marks on the picture.
//! - `ops`: add, delete, duplicate, names, and the history of undo and redo.
//! - `events`: the commands of a page as rows, the names that the events of all
//!   rooms use, the checks of the editor. `event_panel`: the panel of an event.
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
mod event_panel;
mod events;
mod gizmo;
mod ops;
mod overlay;
mod panels;
mod ron_text;
mod view;
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
use glam::Vec3;
use maki_core::{App, Input, Interpolated, Module, Settings, Startup, Time, Update};
use maki_events::{CommandRegistry, EventRunner, GameState, MessageBox, Problem};
use maki_gameplay::{MoveCapture, StartRoom};
use maki_platform::{
    Action, ActionState, InputMode, Key, Pointer, QuitHold, QuitRequested, WindowMargin,
    WindowSettings, add_mode_bindings,
};
use maki_scene::{
    ActiveCamera, Camera, CurrentRoom, MeshInstance, Player, RoomDef, RoomRequest, RoomTarget,
    ScreenFade, load_requested_room, patch_room,
};
use maki_ui::{Panels, TOGGLE_HELP};
use serde::{Deserialize, Serialize};

pub use camera::{EditorCamera, ORBIT, PAN, VIEW_GAME, VIEW_TOP, ZOOM_IN, ZOOM_OUT};
pub use document::{Document, Item, Place};
pub use event_panel::{ArgumentEditors, EventUi, add_argument_editor};
pub use events::{Names, OtherRoom, Row, RowKind, Use, names, other_rooms, problems};
pub use gizmo::{Drag, Gizmo, Handle, Tool};
pub use ops::{History, Kind, spawn_users};
pub use ron_text::{Notes, pretty, read_notes};
pub use view::{Ray, View, pick};

/// The input mode while a room is edited: the keys of the game start nothing.
pub const MODE: InputMode = InputMode("editor");
/// Plays the room. To stop: the quit action of the game, or the close button.
pub const PLAY: Action = Action("editor_play");
/// Selects the item under the pointer, or takes a handle of the gizmo.
pub const SELECT: Action = Action("editor_select");
/// Held: a click adds to the selection.
pub const ADD_TO_SELECTION: Action = Action("editor_add_to_selection");
/// Held: a drag does not snap (or does, with `snap = false`), and the keys of
/// undo, redo, duplicate, and save do their work.
pub const MODIFIER: Action = Action("editor_modifier");
pub const TOOL_MOVE: Action = Action("editor_tool_move");
pub const TOOL_ROTATE: Action = Action("editor_tool_rotate");
pub const TOOL_SCALE: Action = Action("editor_tool_scale");
/// The handles follow the axes of the item, or those of the room.
pub const TOGGLE_LOCAL: Action = Action("editor_toggle_local");
pub const DELETE: Action = Action("editor_delete");
// The four work only with `MODIFIER`: a binding is one key.
pub const UNDO: Action = Action("editor_undo");
pub const REDO: Action = Action("editor_redo");
pub const DUPLICATE: Action = Action("editor_duplicate");
pub const SAVE: Action = Action("editor_save");

const BINDINGS: [(Key, Action, &str); 13] = [
    (Key::F5, PLAY, "Play the room"),
    (Key::MouseLeft, SELECT, "Select, or drag a handle"),
    (
        Key::Shift,
        ADD_TO_SELECTION,
        "Add to the selection (with a click)",
    ),
    (Key::Control, MODIFIER, "No snap (in a drag)"),
    (Key::W, TOOL_MOVE, "Tool: move"),
    (Key::E, TOOL_ROTATE, "Tool: rotate"),
    (Key::R, TOOL_SCALE, "Tool: scale"),
    (Key::Q, TOGGLE_LOCAL, "Handles of the item / of the room"),
    (Key::Delete, DELETE, "Delete the selection"),
    (Key::Z, UNDO, "Undo (with Ctrl)"),
    (Key::Y, REDO, "Redo (with Ctrl)"),
    (Key::D, DUPLICATE, "Duplicate (with Ctrl)"),
    (Key::S, SAVE, "Save (with Ctrl)"),
];

/// The `[editor]` table of the config file.
#[derive(Resource, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EditorSettings {
    /// True: a drag of a handle snaps. The modifier key turns it around for a drag.
    pub snap: bool,
    /// A position snaps to steps of this, in meters.
    pub grid_step: f32,
    /// A rotation snaps to steps of this, in degrees.
    pub angle_step: f32,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            snap: true,
            grid_step: 0.25,
            angle_step: 15.0,
        }
    }
}

impl Settings for EditorSettings {
    const TABLE: &'static str = "editor";

    fn check(&self) -> Result<(), String> {
        let positive = |value: f32| value.is_finite() && value > 0.0;
        if !(positive(self.grid_step) && positive(self.angle_step)) {
            return Err("grid_step and angle_step must be positive numbers".to_owned());
        }
        Ok(())
    }
}

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
    /// Plays the room in step mode and starts a page of an event (its name, the
    /// index of the page), whatever its trigger and conditions are.
    Test(String, usize),
    /// A new item, on the floor at the point that the view looks at.
    Add(Kind),
    /// Removes the selected items.
    Delete,
    /// A copy of each selected item. The copies are the selection then.
    Duplicate,
    Undo,
    Redo,
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
    /// The selected items. The property panel shows the last one, and the gizmo is
    /// there. None: the panel shows the room.
    pub selection: Vec<Item>,
    pub tool: Tool,
    /// The handles of the gizmo follow the axes of the item, not those of the room.
    pub local: bool,
    /// The gizmo is at pose `b` of a selected mover.
    pub second_pose: bool,
    /// The panels write it in each frame: a mouse button is down or a widget has the
    /// keyboard, so a change can go on. The history makes no step then.
    pub(crate) busy: bool,
    /// The event commands of the game: to change the events with a name.
    pub(crate) registry: CommandRegistry,
    /// The handle that the pointer drags.
    drag: Option<Drag>,
    history: History,
    /// The state of the event panel.
    pub(crate) event_ui: EventUi,
    /// The other room files beside the open one. `None`: read them again.
    others: Option<Vec<OtherRoom>>,
    /// The page that a test waits to start, when the room of the game is loaded.
    test: Option<(String, usize)>,
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

    /// The item of the property panel: the last selected one, or the room.
    pub fn primary(&self) -> Item {
        self.selection.last().cloned().unwrap_or_default()
    }

    /// Selects an item. `add`: the other items stay selected, and an item that is
    /// selected is not selected then.
    pub fn select(&mut self, item: Item, add: bool) {
        let was_selected = self.selection.contains(&item);
        if !add {
            self.selection.clear();
        }
        self.selection.retain(|other| *other != item);
        if !(add && was_selected) {
            self.selection.push(item);
        }
    }

    /// The gizmo of the selection in a view.
    pub fn gizmo(&self, view: &View) -> Option<Gizmo> {
        let item = self.selection.last()?;
        Gizmo::of(
            &self.document,
            item,
            self.second_pose,
            self.local,
            self.tool,
            view,
        )
    }

    /// The handle that the pointer drags.
    pub fn dragged(&self) -> Option<Handle> {
        self.drag.as_ref().map(|drag| drag.handle)
    }

    /// The numbers of steps that undo and redo can do.
    pub fn history_steps(&self) -> (usize, usize) {
        self.history.steps()
    }

    /// The other room files of the game: for the names that their events use, and
    /// for the spawn points that a `Transfer` can name. Read from the disk after an
    /// open and a save, not in each frame.
    pub fn others(&mut self) -> &[OtherRoom] {
        let (path, registry) = (&self.document.path, &self.registry);
        self.others
            .get_or_insert_with(|| other_rooms(path, registry))
    }

    /// What the checks of the editor find in the events of the room.
    pub fn problems(&mut self) -> Vec<Problem> {
        let others = self.others().to_vec();
        problems(&self.document, &others, &self.registry)
    }

    /// The pointer in the picture: a click takes a handle of the gizmo or selects
    /// the item under it, and a drag of a handle changes the document.
    fn viewport(&mut self, world: &mut World) {
        let Some(view) = editor_view(world).filter(|_| !self.playing()) else {
            self.drag = None;
            return;
        };
        let actions = world.resource::<ActionState>();
        let (pressed, clicked) = (actions.pressed(SELECT), actions.just_pressed(SELECT));
        let (add, modifier) = (actions.pressed(ADD_TO_SELECTION), actions.pressed(MODIFIER));
        let settings = world.resource::<EditorSettings>();
        let steps =
            (settings.snap != modifier).then_some((settings.grid_step, settings.angle_step));
        // The picture starts at the right of the left panel.
        let left = world.resource::<WindowMargin>().left as f32;
        let pixel = world
            .resource::<Pointer>()
            .position
            .map(|[x, y]| [x - left, y]);

        if let Some(drag) = &self.drag {
            match pixel.filter(|_| pressed) {
                Some(pixel) => {
                    let (document, selection) = (&mut self.document, &self.selection);
                    drag.apply(&view, pixel, steps, document, selection, self.second_pose);
                }
                None => {
                    tracing::debug!(?pixel, pressed, "the drag of the gizmo ends");
                    self.drag = None;
                }
            }
            return;
        }
        let in_picture =
            |[x, y]: &[f32; 2]| (0.0..view.size[0]).contains(x) && (0.0..view.size[1]).contains(y);
        let Some(pixel) = pixel.filter(|pixel| clicked && in_picture(pixel)) else {
            return;
        };
        let gizmo = self.gizmo(&view);
        if let Some((gizmo, handle)) =
            gizmo.and_then(|gizmo| Some((gizmo, gizmo.handle_at(&view, pixel)?)))
        {
            tracing::debug!(?handle, ?pixel, "a drag of the gizmo starts");
            self.drag = Some(Drag::start(
                gizmo,
                handle,
                self.local,
                &view,
                pixel,
                &self.document,
            ));
            return;
        }
        let picked = pick(world, &self.document, &view.ray(pixel));
        tracing::debug!(?picked, ?pixel, "click in the picture");
        match picked {
            Some(item) => self.select(item, add),
            None if !add => self.selection.clear(),
            None => {}
        }
    }

    fn run(&mut self, world: &mut World, command: Command, confirmed: bool) {
        if self.playing() && command != Command::Stop {
            return;
        }
        let asks = matches!(
            command,
            Command::New | Command::Open(_) | Command::Play | Command::Test(..) | Command::Quit
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
                match Document::read(&path, registry) {
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
            Command::Test(event, page) => {
                self.play(world);
                // After `play`: it sets the event runner back.
                world.resource_mut::<EventRunner>().set_step_mode(true);
                self.test = Some((event, page));
            }
            Command::Add(kind) => {
                // On the floor, at the point that the view looks at, on the grid.
                let mut cameras = world.query::<&EditorCamera>();
                let target = cameras
                    .iter(world)
                    .next()
                    .map_or(Vec3::ZERO, |camera| camera.target);
                let step = world.resource::<EditorSettings>().grid_step;
                let at = (target / step).round() * step;
                match self.document.add(kind, at.with_y(0.0)) {
                    Ok(item) => {
                        self.selection = vec![item];
                        self.status = "Added".to_owned();
                    }
                    Err(reason) => self.status = reason,
                }
            }
            Command::Delete => {
                let more = self.document.delete(&self.selection);
                self.selection.clear();
                self.status = if more.is_empty() {
                    "Deleted".to_owned()
                } else {
                    more
                };
            }
            Command::Duplicate => {
                let copies = self.document.duplicate(&self.selection);
                if !copies.is_empty() {
                    self.selection = copies;
                    self.status = "Duplicated".to_owned();
                }
            }
            Command::Undo | Command::Redo => {
                let done = match command {
                    Command::Undo => self.history.undo(&mut self.document),
                    _ => self.history.redo(&mut self.document),
                };
                let (undo, redo) = self.history.steps();
                let name = if command == Command::Undo {
                    "Undo"
                } else {
                    "Redo"
                };
                self.status = match done {
                    true => format!("{name}: {undo} steps back, {redo} forward"),
                    false => format!("{name}: no step"),
                };
            }
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
        self.selection.clear();
        self.shown = None;
        self.history.reset(&self.document);
        self.others = None;
        self.event_ui.row = None;
    }

    fn save(&mut self) {
        match self.document.save() {
            Ok(()) => {
                // A "Save as" is a new file among the others.
                self.others = None;
                self.status = match self.problems().len() {
                    0 => "Saved".to_owned(),
                    count => format!("Saved, with {count} problems in the events"),
                };
            }
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
            let target = cameras.iter(world).next().map_or(Vec3::ZERO, |c| c.target);
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
        self.test = None;
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

impl Editor {
    /// The keys of the editor that are commands or switches.
    fn keys(&mut self, actions: &ActionState) {
        let pressed = |action| actions.just_pressed(action);
        let with_modifier = actions.pressed(MODIFIER);
        let commands = [
            (PLAY, false, Command::Play),
            (DELETE, false, Command::Delete),
            (UNDO, true, Command::Undo),
            (REDO, true, Command::Redo),
            (DUPLICATE, true, Command::Duplicate),
            (SAVE, true, Command::Save),
        ];
        for (action, modifier, command) in commands {
            if pressed(action) && modifier == with_modifier {
                self.request(command);
            }
        }
        // W, E, R with the modifier are not tools: Ctrl+S is next to them.
        for (action, tool) in [
            (TOOL_MOVE, Tool::Move),
            (TOOL_ROTATE, Tool::Rotate),
            (TOOL_SCALE, Tool::Scale),
        ] {
            if pressed(action) && !with_modifier {
                self.tool = tool;
            }
        }
        if pressed(TOGGLE_LOCAL) && !with_modifier {
            self.local = !self.local;
        }
    }
}

/// The view of the editor camera. `None`: the app has no editor camera.
pub(crate) fn editor_view(world: &mut World) -> Option<View> {
    let window = world.resource::<WindowSettings>();
    let size = [window.width.max(1) as f32, window.height.max(1) as f32];
    let mut cameras = world.query_filtered::<&Camera, With<EditorCamera>>();
    let camera = *cameras.iter(world).next()?;
    Some(View::new(camera, size))
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
        let registry = app.world().resource::<CommandRegistry>().clone();
        app.add_settings::<EditorSettings>();
        add_mode_bindings(app, MODE, &camera::BINDINGS);
        add_mode_bindings(app, MODE, &BINDINGS);
        add_mode_bindings(app, MODE, &[(Key::F1, TOGGLE_HELP, "Toggle help panel")]);
        // A quit request is a command of the editor: it asks for changes that are
        // not saved, and in a game it is a stop.
        app.resource_mut::<QuitHold>().0 = true;
        let mut document = Document::new_room(self.rooms.join("untitled.ron"));
        document.dirty = false;
        app.insert_resource(Editor {
            document,
            selection: Vec::new(),
            tool: Tool::Move,
            local: false,
            second_pose: false,
            busy: false,
            registry,
            drag: None,
            history: History::default(),
            event_ui: EventUi::default(),
            others: None,
            test: None,
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
    for path in paths {
        Document::read(path, registry)?.save()?;
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
        editor.keys(world.resource::<ActionState>());
        for command in std::mem::take(&mut editor.queue) {
            editor.run(world, command, false);
        }
        editor.viewport(world);
        // An item that a delete or an undo removed is not selected.
        let Editor {
            selection,
            document,
            ..
        } = &mut *editor;
        selection.retain(|item| document.has(item));
        if !editor.playing() {
            let busy = editor.busy || editor.drag.is_some();
            let Editor {
                history, document, ..
            } = &mut *editor;
            history.track(document, busy);
        }
        editor.sync(world);
        editor.apply_mode(world);
        // A test: its page starts when the events of the room are loaded.
        if let Some((event, page)) = &editor.test
            && world
                .resource_mut::<EventRunner>()
                .start_event(event, *page)
        {
            editor.test = None;
        }
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
