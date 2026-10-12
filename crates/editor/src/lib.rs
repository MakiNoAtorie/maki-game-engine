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

mod audio;
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
pub mod widgets;

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
use maki_core::{App, Input, Interpolated, Module, ResetPlay, Settings, Startup, Time, Update};
use maki_events::{CommandRegistry, EventRunner, GameState, Problem};
use maki_gameplay::StartRoom;
use maki_platform::{
    Action, ActionState, Chord, InputMode, Key, Picture, PictureFit, Pointer, QuitHold,
    QuitRequested, WindowTitle, add_mode_bindings,
};
use maki_scene::{
    ActiveCamera, Camera, CurrentRoom, MeshInstance, Player, RoomDef, RoomOverride, RoomRequest,
    RoomTarget, load_requested_room, patch_room,
};
use maki_ui::{PanelSizes, Panels, TOGGLE_HELP};
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

const BINDINGS: [(Key, Action, &str); 11] = [
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
    (Key::Escape, CANCEL, "Cancel a drag, or select nothing"),
];

const CHORDS: [(Chord, Action, &str); 6] = [
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
    /// The width of the outliner and of the property panel, and the height of the
    /// event panel, in points of the UI. The user drags their edges.
    pub left_width: f32,
    pub right_width: f32,
    pub event_height: f32,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            snap: true,
            grid_step: 0.25,
            angle_step: 15.0,
            left_width: 230.0,
            right_width: 340.0,
            event_height: 300.0,
        }
    }
}

impl EditorSettings {
    fn panel_sizes(&self) -> PanelSizes {
        PanelSizes {
            left: self.left_width,
            right: self.right_width,
            bottom: self.event_height,
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
    /// Removes the selected items, or the selected command rows: what had the last
    /// click (`EventUi::focused`). The same for `Duplicate`, `Copy`, and `Paste`.
    Delete,
    /// A copy of each selected item. The copies are the selection then.
    Duplicate,
    /// The selected command rows go into the buffer.
    Copy,
    /// The command rows of the buffer, after the selected row.
    Paste,
    /// The view looks at the last selected item.
    Frame,
    /// Ends a drag of a handle with no change. With no drag: selects nothing.
    Cancel,
    Undo,
    Redo,
    /// Ends the app.
    Quit,
    /// Shows or hides the window with the debug panels of the game.
    DebugPanels,
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
    test: Option<(String, usize)>,
    /// The spawn point where the game starts on `Play`. `None`, or a name that the
    /// room does not have: the first one.
    pub play_from: Option<String>,
    /// One line for the user: what the last command did.
    pub status: String,
    /// Why a command did not do its work. It shows until the user closes it.
    pub error: Option<String>,
    /// True until the cancel key is up: its press was for the game.
    ignore_cancel: bool,
    /// What a click would take now, and the pixel that it was found for.
    pub hover: Option<Hover>,
    hover_pixel: Option<[f32; 2]>,
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
        let Item::Event(index) = self.primary() else {
            return;
        };
        let page = self.event_ui.page;
        let pages = self
            .document
            .events
            .get_mut(index)
            .map(|event| &mut event.pages);
        let Some(page) = pages.and_then(|pages| pages.get_mut(page)) else {
            return;
        };
        if row_command(
            &mut page.commands,
            &mut self.event_ui,
            &self.registry,
            command,
        ) {
            self.document.dirty = true;
        }
        self.status = format!("{command:?}: command rows");
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
        let on_rows = self.event_ui.focused && matches!(self.primary(), Item::Event(_));
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
            Command::Copy | Command::Paste => {
                self.status = "Copy and paste: click a command row of an event first".to_owned();
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
            Command::DebugPanels => {
                let mut panels = world.resource_mut::<Panels>();
                let open = panels.debug_window();
                panels.set_debug_window(!open);
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
        self.error = Some(error);
    }

    /// The text of the title bar: the room file, the mark of a change that is not
    /// saved, and the name of the editor.
    pub fn title(&self) -> String {
        let path = &self.document.path;
        let file = path.file_name().unwrap_or(path.as_os_str());
        let changed = if self.document.dirty { " *" } else { "" };
        format!("{}{changed} - maki editor", file.to_string_lossy())
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
        // The event panel is under the picture while an event is selected.
        let event = !playing && matches!(self.primary(), Item::Event(_));
        let mut panels = world.resource_mut::<Panels>();
        for (panel, open) in [(panels::CONFIRM, ask), (panels::EVENT, event)] {
            if panels.is_open(panel) != open {
                panels.set_open(panel, open);
            }
        }
    }
}

impl Editor {
    /// The keys of the editor that are commands or switches.
    fn keys(&mut self, actions: &ActionState) {
        let pressed = |action| actions.just_pressed(action);
        // The play key stops a game. Its action is the same in the two input modes,
        // so a key that is held over the change of the mode is one press.
        if pressed(PLAY) && self.playing() {
            self.request(Command::Stop);
        }
        if !actions.pressed(CANCEL) {
            self.ignore_cancel = false;
        }
        if pressed(CANCEL) && !self.ignore_cancel {
            self.request(Command::Cancel);
        }
        let commands = [
            (PLAY, Command::Play),
            (DELETE, Command::Delete),
            (COPY, Command::Copy),
            (PASTE, Command::Paste),
            (FRAME, Command::Frame),
            (UNDO, Command::Undo),
            (REDO, Command::Redo),
            (DUPLICATE, Command::Duplicate),
            (SAVE, Command::Save),
        ];
        for (action, command) in commands {
            // Not in a game: the press of the play key that stops the game would
            // start it again, after the stop in the same queue.
            if pressed(action) && !self.playing() {
                self.request(command);
            }
        }
        for (action, tool) in [
            (TOOL_MOVE, Tool::Move),
            (TOOL_ROTATE, Tool::Rotate),
            (TOOL_SCALE, Tool::Scale),
        ] {
            if pressed(action) {
                self.tool = tool;
            }
        }
        if pressed(TOGGLE_LOCAL) {
            self.local = !self.local;
        }
    }
}

/// The key of an action of the editor, as the binding table has it: for the hover
/// text of a button. Empty: the action has no key.
pub(crate) fn key_of(world: &World, action: Action) -> String {
    let bindings = &world.resource::<maki_platform::Bindings>().0;
    let of_action =
        |binding: &&maki_platform::Binding| binding.mode == MODE && binding.action == action;
    let keys: Vec<String> = bindings
        .iter()
        .filter(of_action)
        .map(|b| b.chord.label())
        .collect();
    keys.join(" or ")
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
    /// The directory of the room files of the game: for "Open", "Save as", and new
    /// rooms.
    pub rooms: PathBuf,
}

impl Module for EditorModule {
    fn build(&self, app: &mut App) {
        let start = app.world().resource::<StartRoom>().clone();
        let registry = app.world().resource::<CommandRegistry>().clone();
        // Written back to the config file: the grid step and the panel sizes of
        // the last session.
        app.add_saved_settings::<EditorSettings>();
        let sizes = app.world().resource::<EditorSettings>().panel_sizes();
        app.init_resource::<Panels>();
        app.resource_mut::<Panels>().set_tool_layout(sizes);
        // The picture has the room that the panels leave in the window.
        *app.resource_mut::<PictureFit>() = PictureFit::Window;
        add_mode_bindings(app, MODE, &camera::BINDINGS);
        add_mode_bindings(app, MODE, &BINDINGS);
        add_mode_bindings(app, MODE, &CHORDS);
        // In a game: the same key and the same action stop it.
        maki_platform::add_bindings(app, &[(Key::F5, PLAY, "Stop the game (editor)")]);
        add_mode_bindings(app, MODE, &[(Key::F1, TOGGLE_HELP, "Toggle help panel")]);
        // A quit request is a command of the editor: it asks for changes that are
        // not saved, and in a game it is a stop.
        app.resource_mut::<QuitHold>().0 = true;
        let mut document = Document::new_room(self.rooms.join("untitled.ron"));
        document.dirty = false;
        audio::add_argument_editors(app);
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
            ignore_cancel: false,
            hover: None,
            hover_pixel: None,
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
        // A question waits: the keys of the editor do nothing.
        if editor.pending.is_none() {
            editor.keys(world.resource::<ActionState>());
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
        // The panel sizes that the user dragged go to the settings, and so to the
        // config file.
        if let Some(sizes) = world.resource::<Panels>().tool_sizes() {
            let old = world.resource::<EditorSettings>();
            let rounded = |size: f32| size.round();
            let new = EditorSettings {
                left_width: rounded(sizes.left),
                right_width: rounded(sizes.right),
                event_height: rounded(sizes.bottom),
                ..old.clone()
            };
            if new != *old {
                *world.resource_mut::<EditorSettings>() = new;
            }
        }
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
