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
//! - `camera`: the free camera. `panels`, `widgets`: the menus, the tool row, the
//!   outliner, and the inspector, in the shell of `maki_ui` (docks, command list).
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
//! The editor program (`maki_engine::run_editor`) adds [`EditorModule`] after the
//! modules of the engine and of the project. The program of a game does not have it.

mod audio;
mod camera;
mod document;
mod event_panel;
mod events;
pub mod fields;
mod gizmo;
mod ids;
mod ops;
mod overlay;
mod panels;
mod pictures;
mod ron_text;
pub mod sources;
mod view;
pub mod widgets;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::message::MessageReader;
use bevy_ecs::query::{Has, With};
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::ResMut;
use bevy_ecs::world::{Mut, World};
use glam::Vec3;
use maki_assets::{AssetChanged, AssetFailed, AssetWatch};
use maki_core::{
    App, ConfigFile, Input, Interpolated, Module, ResetPlay, Settings, Startup, Time, Update,
};
use maki_events::{CommandRegistry, EventRunner, GameState, Problem};
use maki_gameplay::StartRoom;
use maki_platform::{
    Action, ActionState, Bindings, Chord, InputMode, Key, Picture, PictureFit, Pointer, QuitHold,
    QuitRequested, WindowTitle, add_mode_bindings,
};
use maki_scene::{
    ActiveCamera, Camera, ComponentTypes, CurrentRoom, MeshInstance, ObjectId, Player, RoomDef,
    RoomOverride, RoomRequest, RoomTarget, load_requested_room, patch_room,
};
use maki_ui::{COMMAND_SEARCH, NoticeLevel, Panels, TOGGLE_HELP, ToolCommand, notify};
use serde::{Deserialize, Serialize};

pub use camera::{EditorCamera, ORBIT, PAN, VIEW_GAME, VIEW_TOP, ZOOM_IN, ZOOM_OUT};
pub use document::{Document, Item, Place};
use event_panel::row_command;
pub use event_panel::{ArgumentEditors, EventUi, RowCommand, add_argument_editor};
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
/// Held: a drag does not snap (or does, with `snap = false`).
pub const SNAP_SWITCH: Action = Action("editor_snap_switch");
pub const TOOL_MOVE: Action = Action("editor_tool_move");
pub const TOOL_ROTATE: Action = Action("editor_tool_rotate");
pub const TOOL_SCALE: Action = Action("editor_tool_scale");
/// The handles follow the axes of the item, or those of the room.
pub const TOGGLE_LOCAL: Action = Action("editor_toggle_local");
pub const DELETE: Action = Action("editor_delete");
pub const UNDO: Action = Action("editor_undo");
pub const REDO: Action = Action("editor_redo");
pub const DUPLICATE: Action = Action("editor_duplicate");
pub const SAVE: Action = Action("editor_save");
pub const COPY: Action = Action("editor_copy");
pub const PASTE: Action = Action("editor_paste");
/// Puts the selection in the middle of the view.
pub const FRAME: Action = Action("editor_frame");
/// Ends a drag with no change. With no drag: selects nothing.
pub const CANCEL: Action = Action("editor_cancel");
/// The name of the selected item becomes a text field in its row of the outliner.
pub const RENAME: Action = Action("editor_rename");

const BINDINGS: [(Key, Action, &str); 12] = [
    (Key::F5, PLAY, "Play the room"),
    (Key::MouseLeft, SELECT, "Select, or drag a handle"),
    (
        Key::Shift,
        ADD_TO_SELECTION,
        "Add to the selection (with a click)",
    ),
    (Key::Control, SNAP_SWITCH, "No snap (in a drag)"),
    (Key::W, TOOL_MOVE, "Tool: move"),
    (Key::E, TOOL_ROTATE, "Tool: rotate"),
    (Key::R, TOOL_SCALE, "Tool: scale"),
    (Key::Q, TOGGLE_LOCAL, "Handles of the item / of the room"),
    (Key::Delete, DELETE, "Delete the selection"),
    (Key::F, FRAME, "Frame the selection in the view"),
    (Key::F2, RENAME, "Rename the selected item"),
    (Key::Escape, CANCEL, "Cancel a drag, or select nothing"),
];

const CHORDS: [(Chord, Action, &str); 7] = [
    (Chord::ctrl(Key::P), COMMAND_SEARCH, "Search a command"),
    (Chord::ctrl(Key::Z), UNDO, "Undo"),
    (Chord::ctrl(Key::Y), REDO, "Redo"),
    (Chord::ctrl(Key::D), DUPLICATE, "Duplicate"),
    (Chord::ctrl(Key::C), COPY, "Copy the selected command rows"),
    (Chord::ctrl(Key::V), PASTE, "Paste command rows"),
    (Chord::ctrl(Key::S), SAVE, "Save"),
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
    /// The command that opens a file at a line in the code editor ("Open in
    /// editor"). `{file}` and `{line}` are replaced.
    pub code_editor: String,
    /// The program file of Blender, for the `.blend` and `.obj` files of the asset
    /// folder (`sources` module). Empty: the editor looks at the usual places.
    pub blender: String,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            snap: true,
            grid_step: 0.25,
            angle_step: 15.0,
            code_editor: "code -g \"{file}:{line}\"".to_owned(),
            blender: String::new(),
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
    /// The document, the view, and the spawn point of an editor app before this one
    /// ([`Session`]).
    Restore(Session),
    Save,
    /// Saves the room to another file, which then is the open one.
    SaveAs(PathBuf),
    Play,
    Stop,
    /// Plays the room in step mode and starts a page of an event (the id of its
    /// object, the index of the page), whatever its trigger and conditions are.
    Test(ObjectId, usize),
    /// A new item, on the floor at the point that the view looks at.
    Add(Kind),
    /// Removes the selected items, or the selected command rows: what had the last
    /// click (`EventUi::focused`). The same for `Duplicate`, `Copy`, and `Paste`.
    Delete,
    /// A copy of each selected item. The copies are the selection then.
    Duplicate,
    /// The selected command rows go into the buffer of the rows; with no click
    /// on a row, the selected items go into the buffer of the items.
    Copy,
    /// The command rows of the buffer, after the selected row; or the items of the
    /// buffer, into the room that is open now.
    Paste,
    /// The selected props and lights become the prefab file of this name
    /// (`prefabs/<name>.prefab`) and one prefab of the room.
    MakePrefab(String),
    /// The items of the selected prefab go into the room, in place of the prefab.
    UnpackPrefab,
    /// The view looks at the last selected item.
    Frame,
    /// The name of the last selected item becomes a text field in the outliner.
    Rename,
    /// Ends a drag of a handle with no change. With no drag: selects nothing.
    Cancel,
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
    /// Do it. The changes are lost.
    DoNotSave,
    /// "Save as" writes over the file that exists.
    Replace,
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
    test: Option<(ObjectId, usize)>,
    /// The spawn point where the game starts on `Play`. `None`, or a name that the
    /// room does not have: the first one.
    pub play_from: Option<String>,
    /// One line for the user: what the last command did. The status row shows it.
    pub status: String,
    /// Why a command did not do its work. It becomes a notice in the same frame.
    pub error: Option<String>,
    /// The item whose row of the outliner becomes a text field in the next frame.
    pub(crate) renaming: Option<Item>,
    /// The converters of the source files of other programs: the file lists mark
    /// a file that has a source.
    pub(crate) converters: sources::Converters,
    /// An event was the selected item in the frame before.
    event_selected: bool,
    /// True until the cancel key is up: its press was for the game.
    ignore_cancel: bool,
    /// What a click would take now, and the pixel that it was found for.
    pub hover: Option<Hover>,
    hover_pixel: Option<[f32; 2]>,
    /// The directory of the room files of the project, and its name.
    rooms: PathBuf,
    project: String,
    queue: Vec<Command>,
    /// A command that waits for an [`Answer`].
    pending: Option<Command>,
    /// `Some` while the game is played: the game state of before.
    playing: Option<GameState>,
    /// The room that the world shows. `None`: load the document.
    shown: Option<(PathBuf, RoomDef)>,
    /// The items that "Copy" took, in the document that had them. It stays when
    /// another room is opened: "Paste" puts the items into that room.
    item_buffer: Option<(Document, Vec<Item>)>,
    /// The name in the "Make prefab" field of the "Edit" menu.
    pub prefab_name: String,
    /// The file and the pose of the prefab that was unpacked last: "Make prefab"
    /// with this file puts the prefab back at this pose.
    unpacked: Option<(PathBuf, maki_scene::PoseDef)>,
}

impl Editor {
    /// The room and the selection, for a test.
    #[cfg(test)]
    fn clone_parts(&self) -> (RoomDef, Vec<Item>) {
        (self.document.room.clone(), self.selection.clone())
    }

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
        self.event_ui.focused = false;
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
        let (add, switch) = (
            actions.pressed(ADD_TO_SELECTION),
            actions.pressed(SNAP_SWITCH),
        );
        let settings = world.resource::<EditorSettings>();
        let steps = (settings.snap != switch).then_some((settings.grid_step, settings.angle_step));
        let picture = *world.resource::<Picture>();
        let pointer = *world.resource::<Pointer>();
        let in_picture = |at: [f32; 2]| picture.from_window(at);
        let pixel = pointer.position.map(in_picture);
        // A click is where the button went down: the pointer can be at another place
        // at the end of the frame.
        let press = pointer.press.map(in_picture).or(pixel);

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
        // What a click would take. Only for a new place of the pointer: the pick
        // reads each triangle of the room.
        let over = pixel.filter(in_picture);
        if over != self.hover_pixel || clicked {
            self.hover_pixel = over;
            self.hover = over.and_then(|pixel| {
                let handle = self
                    .gizmo(&view)
                    .and_then(|gizmo| gizmo.handle_at(&view, pixel));
                handle
                    .map(Hover::Handle)
                    .or_else(|| pick(world, &self.document, &view.ray(pixel)).map(Hover::Item))
            });
        }
        let Some(pixel) = press.filter(|pixel| clicked && in_picture(pixel)) else {
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
            None if !add => {
                self.selection.clear();
                self.event_ui.focused = false;
            }
            None => {}
        }
    }

    /// A command for the selected command rows of the event panel.
    fn rows(&mut self, command: RowCommand) {
        let Some(index) = self.selected_event() else {
            return;
        };
        let Some(Ok(mut event)) = self.document.event(index, &self.registry) else {
            return;
        };
        let Some(commands) = event.list_mut(self.event_ui.page) else {
            return;
        };
        if row_command(commands, &mut self.event_ui, &self.registry, command) {
            self.document.set_event(index, &event);
            self.document.dirty = true;
        }
        self.status = format!("{command:?}: command rows");
    }

    /// The object of the primary item, if it has an event.
    pub(crate) fn selected_event(&self) -> Option<usize> {
        let Item::Object(index) = self.primary() else {
            return None;
        };
        let object = self.document.room.objects.get(index)?;
        object.components.contains_key("event").then_some(index)
    }

    fn run(&mut self, world: &mut World, command: Command, confirmed: bool) {
        if self.playing() && command != Command::Stop {
            return;
        }
        // Not `Play` and `Test`: the game plays the room of the editor, not the file.
        let loses_changes = matches!(command, Command::New | Command::Open(_) | Command::Quit);
        // "Save as" with the name of another room file.
        let replaces = matches!(&command, Command::SaveAs(path)
            if *path != self.document.path && path.exists());
        if (loses_changes && self.document.dirty || replaces) && !confirmed {
            self.pending = Some(command);
            return;
        }
        // The list that had the last click gets the command: the command rows of
        // the event panel, or the items.
        let on_rows = self.event_ui.focused && self.selected_event().is_some();
        let row = match command {
            Command::Delete => Some(RowCommand::Delete),
            Command::Duplicate => Some(RowCommand::Duplicate),
            Command::Copy => Some(RowCommand::Copy),
            Command::Paste => Some(RowCommand::Paste),
            _ => None,
        };
        if let (true, Some(row)) = (on_rows, row) {
            return self.rows(row);
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
                        self.error = None;
                    }
                    Err(error) => self.fail(format!("{error:#}")),
                }
            }
            Command::Restore(session) => {
                let registry = world.resource::<CommandRegistry>();
                match Document::parse(&session.path, &session.text, registry) {
                    Ok(mut document) => {
                        document.dirty = session.dirty;
                        self.open(document);
                        self.selection = session.selection;
                        self.play_from = session.play_from;
                        if let Some(view) = session.view {
                            let mut cameras = world.query::<&mut EditorCamera>();
                            cameras
                                .iter_mut(world)
                                .for_each(|mut camera| *camera = view);
                        }
                        self.status =
                            "The code of the project was loaded again. The undo steps are gone"
                                .to_owned();
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
                self.document.delete(&self.selection);
                self.selection.clear();
                self.status = "Deleted".to_owned();
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
                    Command::Undo => self.history.undo(&mut self.document, &mut self.selection),
                    _ => self.history.redo(&mut self.document, &mut self.selection),
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
            Command::Copy => {
                let items = self.selection.clone();
                self.status = format!("{} items copied", items.len());
                self.item_buffer = Some((self.document.clone(), items));
            }
            Command::Paste => match &self.item_buffer {
                Some((source, items)) => {
                    // In the room that they came from: beside the items.
                    let same = source.path == self.document.path;
                    let offset = if same { ops::DUPLICATE_OFFSET } else { 0.0 };
                    let copies = self.document.copy_from(source, items, offset);
                    self.status = format!("{} items pasted", copies.len());
                    if !copies.is_empty() {
                        self.selection = copies;
                    }
                }
                None => self.status = "Paste: copy items or command rows first".to_owned(),
            },
            Command::MakePrefab(name) => {
                let file = Path::new("prefabs").join(&name).with_extension("prefab");
                // The prefab that was unpacked goes back to its pose: its items are
                // then at the same places in each other room.
                let outer = self
                    .unpacked
                    .as_ref()
                    .filter(|(unpacked, _)| *unpacked == file);
                let outer = outer.map(|(_, pose)| *pose);
                match self.document.make_prefab(&self.selection, &file, outer) {
                    Ok(item) => {
                        self.selection = vec![item];
                        self.status = format!("{} written", file.display());
                        // The world shows the file, not the document: load again.
                        self.shown = None;
                    }
                    Err(reason) => self.status = reason,
                }
            }
            Command::UnpackPrefab => {
                let no_types = ComponentTypes::default();
                let types = world.get_resource::<ComponentTypes>().unwrap_or(&no_types);
                match self.document.unpack_prefab(&self.primary(), types) {
                    Ok((items, file, pose)) => {
                        let stem = file.file_stem().unwrap_or_default();
                        self.prefab_name = stem.to_string_lossy().into_owned();
                        self.unpacked = Some((file, pose));
                        self.selection = items;
                        self.status = "Unpacked".to_owned();
                    }
                    Err(reason) => self.status = reason,
                }
            }
            Command::Frame => {
                let at = self.document.pose_of(&self.primary(), self.second_pose);
                let room = &self.document.room;
                // An item: near it. The room, or an item with no place: all of it.
                let (target, distance) = match at {
                    Some((at, _)) => (at, FRAME_DISTANCE),
                    None => (Vec3::ZERO, room.width.max(room.depth) * 1.5),
                };
                let mut cameras = world.query::<&mut EditorCamera>();
                for mut camera in cameras.iter_mut(world) {
                    (camera.target, camera.distance) = (target, distance);
                }
            }
            Command::Rename => {
                self.renaming = Some(self.primary()).filter(Item::has_name);
                // The text field is in the outliner.
                world.resource_mut::<Panels>().show(panels::OUTLINER);
            }
            Command::Cancel => match self.drag.take() {
                // No step of the history: the document is as before the drag.
                Some(drag) => {
                    drag.cancel(&mut self.document);
                    self.status = "The drag was cancelled".to_owned();
                }
                None => {
                    self.selection.clear();
                    self.event_ui.focused = false;
                }
            },
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
                let go =
                    matches!(answer, Answer::DoNotSave | Answer::Replace) || !self.document.dirty;
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
        // A file that moved while the room was closed: the room gets its new path.
        ids::correct_paths(&mut self.document);
        self.history.reset(&self.document);
        self.others = None;
        self.event_ui.row = None;
    }

    fn save(&mut self) {
        match self.document.save() {
            Ok(()) => {
                // A "Save as" is a new file among the others.
                self.others = None;
                self.error = None;
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
        self.status = "The command failed".to_owned();
        self.error = Some(error);
    }

    /// Gives an item a new name, as the name field of the inspector does: the
    /// events that name the item follow. An empty name is no name.
    pub fn rename(&mut self, item: &Item, new: String) {
        let registry = self.registry.clone();
        // The name that the item had.
        let old = match item {
            Item::Object(index) => match self.document.room.objects.get_mut(*index) {
                Some(object) => {
                    std::mem::replace(&mut object.name, (!new.is_empty()).then_some(new))
                }
                None => return,
            },
            Item::Room => return,
        };
        let spawn = match item {
            Item::Object(index) => self.document.room.objects[*index].is_spawn(),
            _ => false,
        };
        let old_spawn = old.clone().filter(|_| spawn);
        let more = self.document.renamed(item, old, &registry);
        if !more.is_empty() {
            self.status = more;
        }
        // The rooms that go to the spawn point by its old name.
        if let Some(old) = old_spawn {
            let users = spawn_users(&self.rooms, &self.document.path, &old, &registry);
            if !users.is_empty() {
                let users = users.join(", ");
                self.status = format!("{users}: a Transfer still names `{old}`");
            }
        }
        self.document.dirty = true;
    }

    /// The text of the title bar: the room file, the mark of a change that is not
    /// saved, and the name of the editor.
    pub fn title(&self) -> String {
        let path = &self.document.path;
        let file = path.file_name().unwrap_or(path.as_os_str());
        let changed = if self.document.dirty { " *" } else { "" };
        let project = &self.project;
        format!(
            "{}{changed} - {project} - maki editor",
            file.to_string_lossy()
        )
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
        let spawns = self.document.room.spawn_names();
        let chosen = self.play_from.as_ref().filter(|name| spawns.contains(name));
        let spawn = chosen.or_else(|| spawns.first()).cloned();
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
        // Also when the game comes back to this room by a `Transfer`.
        let (path, text) = (self.document.path.clone(), self.document.text());
        world.resource_mut::<RoomOverride>().0 = Some((path.clone(), text.clone()));
        world.resource_mut::<RoomRequest>().0 = Some(RoomTarget {
            path,
            spawn,
            text: Some(text),
        });
        self.status = "Playing".to_owned();
    }

    fn stop(&mut self, world: &mut World) {
        let Some(state) = self.playing.take() else {
            return;
        };
        *world.resource_mut::<GameState>() = state;
        reset_play_state(world);
        world.resource_mut::<RoomOverride>().0 = None;
        // The quit key of the game is also the cancel key of the editor: the press
        // that stopped the game does not clear the selection.
        self.ignore_cancel = true;
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
    fn apply_mode(&mut self, world: &mut World) {
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
        // An object with an event becomes the selected item: the event panel shows.
        // The user can close the tab while the object stays selected.
        let event = !playing && self.selected_event().is_some();
        if event && !self.event_selected {
            panels.show(panels::EVENT);
        }
        self.event_selected = event;
    }
}

/// "Can run now" of the commands: the room is edited, and no question waits.
fn editing(world: &World) -> bool {
    let editor = world.resource::<Editor>();
    !editor.playing() && editor.pending.is_none()
}

fn playing(world: &World) -> bool {
    world.resource::<Editor>().playing()
}

/// A test runs: the events wait for a step.
fn stepping(world: &World) -> bool {
    playing(world) && world.resource::<EventRunner>().step_mode()
}

fn selected(world: &World) -> bool {
    editing(world) && !world.resource::<Editor>().selection.is_empty()
}

/// The commands of the editor, for the command list of `maki_ui`: the menus, the
/// tool buttons, the keys, and the command search are made from it.
fn add_commands(app: &mut App) {
    type CanRun = fn(&World) -> bool;
    let again = ToolCommand {
        menu: "Assets",
        ..ToolCommand::new("assets.load_again", "Load all again", load_assets_again)
    };
    maki_ui::add_command(app, again);
    let mut add = |id, name, menu, action: Option<Action>, can_run: CanRun, command: Command| {
        let run = move |world: &mut World| world.resource_mut::<Editor>().request(command.clone());
        maki_ui::add_command(
            app,
            ToolCommand {
                menu,
                action,
                can_run,
                ..ToolCommand::new(id, name, run)
            },
        );
    };
    let can_undo: CanRun =
        |world| editing(world) && world.resource::<Editor>().history_steps().0 > 0;
    let can_redo: CanRun =
        |world| editing(world) && world.resource::<Editor>().history_steps().1 > 0;
    let named: CanRun = |world| editing(world) && world.resource::<Editor>().primary().has_name();
    let prefab: CanRun = |world| {
        let editor = world.resource::<Editor>();
        editing(world) && editor.document.prefab_use(&editor.primary()).is_some()
    };
    // The press of the cancel key that stopped a game is not a cancel.
    let can_cancel: CanRun = |world| editing(world) && !world.resource::<Editor>().ignore_cancel;
    add(
        "editor.new",
        "New room",
        "File",
        None,
        editing,
        Command::New,
    );
    add(
        "editor.save",
        "Save",
        "File",
        Some(SAVE),
        editing,
        Command::Save,
    );
    add("editor.quit", "Quit", "File", None, editing, Command::Quit);
    add(
        "editor.undo",
        "Undo",
        "Edit",
        Some(UNDO),
        can_undo,
        Command::Undo,
    );
    add(
        "editor.redo",
        "Redo",
        "Edit",
        Some(REDO),
        can_redo,
        Command::Redo,
    );
    let duplicate = Command::Duplicate;
    add(
        "editor.duplicate",
        "Duplicate",
        "Edit",
        Some(DUPLICATE),
        selected,
        duplicate,
    );
    add(
        "editor.delete",
        "Delete",
        "Edit",
        Some(DELETE),
        selected,
        Command::Delete,
    );
    add(
        "editor.copy",
        "Copy",
        "Edit",
        Some(COPY),
        selected,
        Command::Copy,
    );
    add(
        "editor.paste",
        "Paste",
        "Edit",
        Some(PASTE),
        editing,
        Command::Paste,
    );
    add(
        "editor.rename",
        "Rename",
        "Edit",
        Some(RENAME),
        named,
        Command::Rename,
    );
    let unpack = Command::UnpackPrefab;
    add(
        "editor.unpack_prefab",
        "Unpack prefab",
        "Edit",
        None,
        prefab,
        unpack,
    );
    for (text, kind) in Kind::MENU {
        // The id of a command is a text of the program: made one time, and kept.
        let id = format!("editor.add.{}", text.to_lowercase().replace(' ', "_"));
        add(id.leak(), text, "Add", None, editing, Command::Add(kind));
    }
    add(
        "editor.frame",
        "Frame in the view",
        "View",
        Some(FRAME),
        editing,
        Command::Frame,
    );
    add(
        "editor.play",
        "Play",
        "",
        Some(PLAY),
        editing,
        Command::Play,
    );
    // The same action in a game: the play key stops it.
    add(
        "editor.stop",
        "Stop",
        "",
        Some(PLAY),
        playing,
        Command::Stop,
    );
    let cancel = "Cancel a drag, or select nothing";
    add(
        "editor.cancel",
        cancel,
        "",
        Some(CANCEL),
        can_cancel,
        Command::Cancel,
    );

    // The commands that are not in the queue of the editor: they change one value.
    let mut add = |id, name, menu, action: Option<Action>, can_run: CanRun, run: fn(&mut World)| {
        maki_ui::add_command(
            app,
            ToolCommand {
                menu,
                action,
                can_run,
                run: Arc::new(run),
                ..ToolCommand::new(id, name, |_| {})
            },
        );
    };
    add(
        "editor.view_top",
        "View from the top",
        "View",
        Some(VIEW_TOP),
        editing,
        camera::view_top,
    );
    let game = "View of the game camera";
    add(
        "editor.view_game",
        game,
        "View",
        Some(VIEW_GAME),
        editing,
        camera::view_game,
    );
    add(
        "editor.tool_move",
        "Tool: move",
        "",
        Some(TOOL_MOVE),
        editing,
        |world| {
            world.resource_mut::<Editor>().tool = Tool::Move;
        },
    );
    add(
        "editor.tool_rotate",
        "Tool: rotate",
        "",
        Some(TOOL_ROTATE),
        editing,
        |world| {
            world.resource_mut::<Editor>().tool = Tool::Rotate;
        },
    );
    add(
        "editor.tool_scale",
        "Tool: scale",
        "",
        Some(TOOL_SCALE),
        editing,
        |world| {
            world.resource_mut::<Editor>().tool = Tool::Scale;
        },
    );
    let local = "Handles along the axes of the item, not of the room";
    add(
        "editor.local",
        local,
        "",
        Some(TOGGLE_LOCAL),
        editing,
        |world| {
            let mut editor = world.resource_mut::<Editor>();
            editor.local = !editor.local;
        },
    );
    let step = "Step: the next command of the event";
    add("editor.step", step, "", None, stepping, |world| {
        world.resource_mut::<EventRunner>().step_command();
    });
    add(
        "editor.run",
        "Run: no more steps",
        "",
        None,
        stepping,
        |world| {
            world.resource_mut::<EventRunner>().set_step_mode(false);
        },
    );
}

/// What an editor app hands to the next one, when the code of the project is loaded
/// again (`maki_platform::NextApp`). The document goes as the text of a room file:
/// its event commands are values of types of the old code.
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    /// The room file of the document: where a save writes.
    pub path: PathBuf,
    pub text: String,
    /// True: the document is not what the file holds.
    pub dirty: bool,
    /// The selected items.
    pub selection: Vec<Item>,
    /// The spawn point where the game starts on "Play".
    pub play_from: Option<String>,
    /// The view of the editor camera.
    pub view: Option<EditorCamera>,
}

/// The session of an app with the editor. `None`: the app has no editor.
pub fn session(world: &World) -> Option<Session> {
    let editor = world.get_resource::<Editor>()?;
    let mut cameras = world.try_query::<&EditorCamera>()?;
    Some(Session {
        path: editor.document.path.clone(),
        text: editor.document.text(),
        dirty: editor.document.dirty,
        selection: editor.selection.clone(),
        play_from: editor.play_from.clone(),
        view: cameras.iter(world).next().copied(),
    })
}

/// What is under the pointer in the picture: a click takes it.
#[derive(Debug, Clone, PartialEq)]
pub enum Hover {
    Handle(Handle),
    Item(Item),
}

/// The distance of the view from an item that `Command::Frame` puts in its middle.
const FRAME_DISTANCE: f32 = 4.0;

/// The view of the editor camera. `None`: the app has no editor camera.
pub(crate) fn editor_view(world: &mut World) -> Option<View> {
    let size = world.resource::<Picture>().size();
    let mut cameras = world.query_filtered::<&Camera, With<EditorCamera>>();
    let camera = *cameras.iter(world).next()?;
    Some(View::new(camera, size))
}

/// The mesh of the player while the room is edited.
#[derive(Component)]
struct Parked(MeshInstance);

/// What a game leaves behind, beside the game state and the room: each module sets
/// its own play state back (`maki_core::ResetPlay`).
fn reset_play_state(world: &mut World) {
    world.run_schedule(ResetPlay);
}

/// Adds the editor. The app then starts in the editor, with the start room of the
/// game open. Add it after the modules of the game. Needs `maki_ui::UiModule`,
/// `maki_events::EventsModule`, and `maki_gameplay::RoomsModule`.
pub struct EditorModule {
    /// The directory of the room files of the project: for "Open", "Save as", and
    /// new rooms.
    pub rooms: PathBuf,
    /// The name of the project, for the title of the window.
    pub project: String,
    /// `Some`: the editor goes on with the document of the app before this one.
    /// `None`: it opens the start room.
    pub session: Option<Session>,
}

impl Module for EditorModule {
    fn build(&self, app: &mut App) {
        let start = app.world().resource::<StartRoom>().clone();
        let registry = app.world().resource::<CommandRegistry>().clone();
        // Written back to the config file: the snap and its steps of the last
        // session.
        app.add_saved_settings::<EditorSettings>();
        // The layout of the docks is in a file beside the config file.
        let layout = app.world().resource::<ConfigFile>().beside("layout.ron");
        app.init_resource::<Panels>();
        let mut panels = app.resource_mut::<Panels>();
        panels.set_tool_layout(panels::LAYOUTS.to_vec(), layout);
        // The status row has the frame time: nothing is over the picture.
        panels.set_open("Frame counter", false);
        // The picture has the room that the panels leave in the window.
        *app.resource_mut::<PictureFit>() = PictureFit::Window;
        add_mode_bindings(app, MODE, &camera::BINDINGS);
        add_mode_bindings(app, MODE, &BINDINGS);
        add_mode_bindings(app, MODE, &CHORDS);
        // In a game: the same key and the same action stop it.
        maki_platform::add_bindings(app, &[(Key::F5, PLAY, "Stop the game (editor)")]);
        add_mode_bindings(app, MODE, &[(Key::F1, TOGGLE_HELP, "Toggle help panel")]);
        // F5 plays here: the room reload loses its key after all modules, in any order.
        app.add_systems(Startup, |mut bindings: ResMut<Bindings>| {
            bindings
                .0
                .retain(|binding| binding.action != maki_gameplay::RELOAD_ROOM);
        });
        // A quit request is a command of the editor: it asks for changes that are
        // not saved, and in a game it is a stop.
        app.resource_mut::<QuitHold>().0 = true;
        // The source files of other programs in the asset folder: a thread
        // converts them, at the start and when one is saved.
        let blender = app.world().resource::<EditorSettings>().blender.clone();
        sources::start(app, &self.rooms, &blender);
        let mut document = Document::new_room(self.rooms.join("untitled.ron"));
        document.dirty = false;
        audio::add_argument_editors(app);
        app.init_resource::<overlay::MarkDepth>();
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
            error: None,
            renaming: None,
            converters: app.world().resource::<sources::Converters>().clone(),
            event_selected: false,
            ignore_cancel: false,
            hover: None,
            hover_pixel: None,
            rooms: self.rooms.clone(),
            project: self.project.clone(),
            // In the first frame: all modules have added their event commands then.
            queue: vec![match &self.session {
                Some(session) => Command::Restore(session.clone()),
                None => Command::Open(start.path),
            }],
            pending: None,
            playing: None,
            shown: None,
            item_buffer: None,
            prefab_name: String::new(),
            unpacked: None,
        })
        .add_systems(Startup, camera::spawn)
        .add_systems(Input, take_quit_request)
        // A file of an asset that another program writes again shows at once, while
        // a room is edited and while a game runs.
        .init_resource::<AssetWatch>()
        .add_message::<AssetChanged>()
        .add_message::<AssetFailed>()
        .add_systems(
            Update,
            (tell_asset_changes, sources::tell_reports)
                .chain()
                .before(run_commands),
        )
        .add_systems(
            Update,
            (
                // It has the full world: a command can load a room and stop a game.
                // Before the loader: a room that it asks for shows in this frame.
                // After the keys of the commands: a key is a command of this frame.
                run_commands
                    .before(load_requested_room)
                    .after(maki_ui::command_keys)
                    .ambiguous_with_all(),
                // It has the full world: the GPU, and the meshes of the mirrors.
                overlay::read_depth.ambiguous_with_all(),
                camera::move_camera,
            ),
        );
        app.resource_mut::<AssetWatch>().on = true;
        add_commands(app);
        panels::build(app);
    }
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

/// Runs in `Update`: one line in the status row for each asset file that changed on
/// the disk, and for each one that changed and does not read (the old content
/// stays then).
fn tell_asset_changes(
    mut changed: MessageReader<AssetChanged>,
    mut failed: MessageReader<AssetFailed>,
    mut editor: ResMut<Editor>,
) {
    let name = |path: &Path| {
        path.file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned()
    };
    let files: Vec<&AssetChanged> = changed.read().collect();
    match files[..] {
        [] => {}
        [AssetChanged(path)] => editor.status = format!("{}: loaded again", name(path)),
        _ => editor.status = format!("{} asset files: loaded again", files.len()),
    }
    for AssetFailed { path, error } in failed.read() {
        let file = name(path);
        editor.status =
            format!("{file} changed, but it does not read: {error}. The old content stays");
    }
}

/// The command "Assets > Load all again": each store forgets each file that it
/// loaded, and the room is loaded again. For a program whose save does not change
/// the time of the file.
fn load_assets_again(world: &mut World) {
    let watch = world.resource::<AssetWatch>();
    let files: Vec<PathBuf> = watch.files().map(Path::to_owned).collect();
    let count = files.len();
    for file in files {
        world.write_message(AssetChanged(file));
    }
    // Also a room with no such file: its prefab files are read again.
    let mut editor = world.resource_mut::<Editor>();
    if !editor.playing() {
        editor.shown = None;
    }
    editor.status = format!("{count} asset files are loaded again");
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
pub fn run_commands(world: &mut World) {
    world.resource_scope(|world, mut editor: Mut<Editor>| {
        // The cancel key is up: its next press is a cancel again.
        if !world.resource::<ActionState>().pressed(CANCEL) {
            editor.ignore_cancel = false;
        }
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
                history,
                document,
                selection,
                ..
            } = &mut *editor;
            history.track(document, selection, busy);
        }
        editor.sync(world);
        editor.apply_mode(world);
        let title = WindowTitle(editor.title());
        world.resource_mut::<WindowTitle>().set_if_neq(title);
        // The tab of the picture has the name of the room file.
        let path = &editor.document.path;
        let file = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy();
        let changed = if editor.document.dirty { " *" } else { "" };
        let tab = format!("{file}{changed}");
        world.resource_mut::<Panels>().set_document(&tab);
        if let Some(error) = editor.error.take() {
            notify(world, NoticeLevel::Error, error);
        }
        // A test: its page starts when the events of the room are loaded.
        if let Some((event, page)) = editor.test
            && maki_events::start_event(world, event, page)
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
