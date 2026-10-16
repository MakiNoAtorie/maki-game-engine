//! The event panel: the property panel of an event. An event has pages; a page has
//! conditions, a trigger, and a list of commands, as in RPG Maker.
//!
//! Names are picked, not typed, where the editor knows them: a light, a door, or a
//! mover of the room; a flag or a variable that an event of any room file uses; a
//! room file and one of its spawn points. A command of a module that the editor does
//! not know has a text field with the command as a room file has it.

use std::collections::BTreeMap;
use std::sync::Arc;

use bevy_ecs::resource::Resource;
use bevy_ecs::world::World;
use maki_core::fields::{Field, Kind, Value};
use maki_core::{App, FieldTypes, Fields};
use maki_events::{
    Builtin, Command, CommandRegistry, CommandSet, Condition, EventPages, Page, Problem, Trigger,
    Wire, active_page,
};
use maki_scene::{ComponentTypes, ObjectId, RoomDef, ron_options};
use maki_ui::egui::{self, Color32, Ui};
use serde::de::DeserializeOwned;

use crate::events::{
    ListPath, Names, Row, RowKind, edit_list, file_name, move_command, names, problems, rows,
};
use crate::widgets::{choice, drag, number, object_names, object_pick, object_ref, row};
use crate::{Command as EditorCommand, Editor};

pub(crate) const ERROR: Color32 = maki_ui::THEME.error;
/// A warning of a check, and the mark of a name that no event writes.
pub(crate) const HINT: Color32 = maki_ui::THEME.warning;
/// The mark of a flag or a variable that an event reads and no event writes.
pub(crate) const NEVER_WRITTEN: &str = "no event sets it";

/// What the event panel keeps between frames.
#[derive(Default)]
pub struct EventUi {
    /// The page that the panel shows.
    pub page: usize,
    /// The row of the command list that had the last click.
    pub row: Option<Row>,
    /// With Shift and a click: `row` as it was then, and the index of the other end
    /// of the range of selected rows, in the same list.
    anchor: Option<(Row, usize)>,
    /// True: a command row had the last click, not an item of the room. Delete,
    /// duplicate, copy, and paste go to the rows then.
    pub focused: bool,
    /// The copied commands. They stay when another event or another room is opened.
    pub clipboard: Vec<Command>,
    /// A command as a text that the user types.
    draft: Option<Draft>,
    /// The row that the pointer drags to another place.
    dragged: Option<Row>,
    /// The event and the page that the panel showed in the frame before: the row
    /// and the typed command are of that page.
    shown: (usize, usize),
    /// The `SetField` lists and the room and directory they are of: a prefab file is
    /// read again only when the document changes.
    pub(crate) live: Option<(RoomDef, std::path::PathBuf, LiveLists)>,
}

/// The parts of [`Lists`] that `live_lists` fills.
#[derive(Clone, Default)]
pub(crate) struct LiveLists {
    objects: Vec<(String, Vec<String>)>,
    fields: BTreeMap<String, Vec<Field>>,
    current: BTreeMap<(String, String, String), Value>,
}

impl EventUi {
    /// The selected command rows: their list, and their indexes in it.
    pub(crate) fn selected(&self) -> Option<(ListPath, std::ops::RangeInclusive<usize>)> {
        let row = self.row.as_ref()?;
        if row.kind != RowKind::Command {
            return None;
        }
        // The other end, if the row did not change after the Shift click.
        let anchor = self.anchor.as_ref().filter(|anchor| anchor.0 == *row);
        let other = anchor.map_or(row.index, |anchor| anchor.1);
        Some((
            row.list.clone(),
            row.index.min(other)..=row.index.max(other),
        ))
    }
}

impl EventUi {
    /// Shows a page of an event, with one of its command rows selected: for a
    /// link to that place (a problem, a use of a name).
    pub(crate) fn go_to(&mut self, event: usize, page: usize, row: Option<Row>) {
        (self.page, self.shown) = (page, (event, page));
        (self.row, self.anchor, self.draft) = (row, None, None);
        self.focused = true;
    }

    /// Selects command rows: `row` had the click. `other`: with Shift, the index of
    /// the other end of the range, in the same list.
    pub(crate) fn select_rows(&mut self, row: Row, other: Option<usize>) {
        self.anchor = other.map(|other| (row.clone(), other));
        self.row = Some(row);
        self.draft = None;
        self.focused = true;
    }
}

/// What the user does with the selected command rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowCommand {
    Delete,
    /// Into the buffer.
    Copy,
    /// The buffer, after the selected rows.
    Paste,
    /// A copy of the rows, after them.
    Duplicate,
}

/// Does a command of the user on the selected rows of a page. True: the list of
/// the page was changed.
pub(crate) fn row_command(
    commands: &mut Vec<Command>,
    ui: &mut EventUi,
    registry: &CommandRegistry,
    command: RowCommand,
) -> bool {
    let selected = ui.selected();
    let copies = |range: &std::ops::RangeInclusive<usize>, path: &ListPath| -> Vec<Command> {
        let list = crate::events::list(commands, path, registry).unwrap_or_default();
        let rows = list.iter().skip(*range.start());
        rows.take(range.end() - range.start() + 1)
            .cloned()
            .collect()
    };
    // New commands: after the last selected row, or where a new command goes for
    // an `Else` or an `End`, or at the end of the page.
    let place = |ui: &EventUi, commands: &[Command]| match (&selected, &ui.row) {
        (Some((path, range)), _) => (path.clone(), range.end() + 1),
        (None, Some(row)) => row.insert_place(commands, registry),
        (None, None) => (Vec::new(), commands.len()),
    };
    let new = match command {
        RowCommand::Copy => {
            if let Some((path, range)) = &selected {
                ui.clipboard = copies(range, path);
            }
            return false;
        }
        RowCommand::Delete => {
            let Some((path, range)) = selected else {
                return false;
            };
            let done = edit_list(commands, &path, registry, |list| {
                let end = (range.end() + 1).min(list.len());
                list.drain((*range.start()).min(end)..end);
            });
            ui.row = None;
            return done.is_some();
        }
        RowCommand::Paste => ui.clipboard.clone(),
        RowCommand::Duplicate => match &selected {
            Some((path, range)) => copies(range, path),
            None => Vec::new(),
        },
    };
    if new.is_empty() {
        return false;
    }
    let (path, index) = place(ui, commands);
    let count = new.len();
    let done = edit_list(commands, &path, registry, |list| {
        let index = index.min(list.len());
        list.splice(index..index, new);
    });
    if done.is_none() {
        return false;
    }
    // The new commands are the selected rows.
    let last = Row {
        list: path,
        index: index + count - 1,
        kind: RowKind::Command,
    };
    ui.anchor = Some((last.clone(), index));
    ui.row = Some(last);
    true
}

struct Draft {
    /// Where the command goes.
    list: ListPath,
    index: usize,
    /// True: it takes the place of the command there. False: it is put in before.
    replace: bool,
    text: String,
}

/// Draws the widgets for the arguments of a command of a type that the editor does
/// not know. `None`: the command is not of the type of this editor.
type ArgumentEditor = Arc<
    dyn Fn(&mut Ui, &Command, &CommandRegistry, &RoomDef) -> Option<Option<Command>> + Send + Sync,
>;

/// The argument editors that modules gave for their commands.
#[derive(Resource, Default, Clone)]
pub struct ArgumentEditors(pub(crate) Vec<ArgumentEditor>);

/// Gives the event panel widgets for the arguments of the commands of the enum `T`
/// (a command enum of the game). `edit` draws them for one value and returns true if
/// it changed the value. It gets the open room, for a list of the names that the
/// command can have; `crate::widgets` has the widgets. Without this, a command of an
/// enum with a field table (`CommandRegistry::add_fields`) has the widgets of the
/// table, and else a text field.
///
/// The "Add" list makes a new command of a module with
/// `CommandRegistry::add_defaults`.
pub fn add_argument_editor<T: CommandSet>(
    app: &mut App,
    edit: impl Fn(&mut Ui, &mut T, &RoomDef) -> bool + Send + Sync + 'static,
) {
    app.init_resource::<ArgumentEditors>();
    let editor: ArgumentEditor = Arc::new(move |ui, command, registry, room| {
        let mut value = command.get::<T>()?.clone();
        let changed = edit(ui, &mut value, room);
        Some(changed.then(|| registry.command(value)).flatten())
    });
    app.resource_mut::<ArgumentEditors>().0.push(editor);
}

/// The objects of a room with an id, and their names (`object_names`).
type ObjectNames = Vec<(ObjectId, String)>;

/// The names that the pickers offer.
#[derive(Default)]
pub(crate) struct Lists {
    flags: Vec<String>,
    variables: Vec<String>,
    /// The flags and the variables that an event reads and no event writes.
    unset_flags: Vec<String>,
    unset_variables: Vec<String>,
    lights: Vec<String>,
    /// Doors and movers: `OpenDoor` finds the two.
    movers: Vec<String>,
    /// Each room file with its spawn points and its objects with an id, the open
    /// room too.
    rooms: Vec<(String, Vec<String>, ObjectNames)>,
    /// The objects of the open room with an id: a pick of a name stores the id.
    ids: ObjectNames,
    /// The objects with a name, each with its components that have a value in the
    /// game (`maki_scene::Live`): what `SetField` changes.
    objects: Vec<(String, Vec<String>)>,
    /// The fields of each of those component types.
    fields: BTreeMap<String, Vec<Field>>,
    /// The value that a field has now in the document, by object, component, field.
    current: BTreeMap<(String, String, String), Value>,
    /// The script functions that `CallScript` calls.
    functions: Vec<String>,
    /// The objects with a name and outputs, each with its outputs: what `Signal`
    /// sends.
    outputs: Vec<(String, Vec<String>)>,
    /// The outputs of the object of the event: what its wires join.
    own_outputs: Vec<String>,
    /// The keys of delete, duplicate, copy, and paste, for the hover texts.
    row_keys: [String; 4],
}

/// Draws the picture of a name of a list.
pub(crate) type Picture<'a> = &'a dyn Fn(&mut Ui, &str);

/// What a click in a list of names took.
enum Picked {
    Name(String),
    /// The entry "New...".
    New,
}

/// A button that opens a list of names, with a search field above them. `marked`:
/// the names with the mark [`NEVER_WRITTEN`]. `new`: the list has the entry "New...".
/// `help`: the hover text of a name. `picture`: draws the picture of a name, at the
/// left of it.
#[allow(clippy::too_many_arguments)]
fn described_list(
    ui: &mut Ui,
    id: egui::Id,
    button: egui::WidgetText,
    names: &[String],
    marked: &[String],
    new: bool,
    help: &dyn Fn(&str) -> Option<&'static str>,
    picture: Option<Picture>,
) -> Option<Picked> {
    let mut picked = None;
    egui::ComboBox::from_id_salt(id)
        .selected_text(button)
        .height(320.0)
        // A click into the search field does not close the list.
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| {
            let search_id = id.with("search");
            let mut search: String = ui.data(|data| data.get_temp(search_id)).unwrap_or_default();
            let field = egui::TextEdit::singleline(&mut search)
                .hint_text("search")
                .desired_width(180.0);
            // The keyboard is at the search field while the list is open: a key
            // starts no tool of the editor.
            ui.add(field).request_focus();
            let wanted = search.to_lowercase();
            let found: Vec<&String> = names
                .iter()
                .filter(|name| name.to_lowercase().contains(&wanted))
                .collect();
            if new && ui.selectable_label(false, "New...").clicked() {
                picked = Some(Picked::New);
            }
            // Enter takes the first name that the search leaves.
            if let (true, Some(first)) = (
                ui.input(|input| input.key_pressed(egui::Key::Enter)),
                found.first(),
            ) {
                picked = Some(Picked::Name((*first).clone()));
            }
            for name in found {
                let label = if marked.contains(name) {
                    egui::RichText::new(format!("{name}  ({NEVER_WRITTEN})")).color(HINT)
                } else {
                    egui::RichText::new(name)
                };
                let mut entry = match picture {
                    Some(picture) => {
                        let row = ui.horizontal(|ui| {
                            picture(ui, name);
                            ui.selectable_label(false, label)
                        });
                        row.inner
                    }
                    None => ui.selectable_label(false, label),
                };
                if let Some(help) = help(name) {
                    entry = entry.on_hover_text(help);
                }
                if entry.clicked() {
                    picked = Some(Picked::Name(name.clone()));
                }
            }
            if picked.is_some() {
                search.clear();
                ui.close();
            }
            ui.data_mut(|data| data.insert_temp(search_id, search));
        });
    picked
}

/// A name of a list, by a click, with a search field. A name that the list does not
/// have shows in red. `new`: the list has the entry "New..." for a name that it does
/// not have; the name is typed then, and Enter takes it.
pub(crate) fn picker(
    ui: &mut Ui,
    salt: &str,
    value: &mut String,
    known: &[String],
    marked: &[String],
    new: bool,
) -> bool {
    picker_with(ui, salt, value, known, marked, new, None)
}

/// As [`picker`]. `picture`: draws the picture of a name in the list.
pub(crate) fn picker_with(
    ui: &mut Ui,
    salt: &str,
    value: &mut String,
    known: &[String],
    marked: &[String],
    new: bool,
    picture: Option<Picture>,
) -> bool {
    let id = ui.make_persistent_id(("picker", salt));
    let typed_id = id.with("new");
    if let Some(mut typed) = ui.data(|data| data.get_temp::<String>(typed_id)) {
        let field = egui::TextEdit::singleline(&mut typed)
            .hint_text("new name, then Enter")
            .desired_width(180.0);
        let response = ui.add(field);
        // Esc, or a click at another place: no new name.
        if response.lost_focus() {
            ui.data_mut(|data| data.remove::<String>(typed_id));
            let name = typed.trim();
            let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
            if enter && !name.is_empty() && name != value {
                *value = name.to_owned();
                return true;
            }
            return false;
        }
        response.request_focus();
        ui.data_mut(|data| data.insert_temp(typed_id, typed));
        return false;
    }
    let shown = egui::RichText::new(value.as_str());
    let shown = if known.contains(value) {
        shown
    } else {
        shown.color(ERROR)
    };
    match described_list(ui, id, shown.into(), known, marked, new, &|_| None, picture) {
        Some(Picked::Name(name)) if name != *value => {
            *value = name;
            true
        }
        Some(Picked::New) => {
            ui.data_mut(|data| data.insert_temp(typed_id, String::new()));
            false
        }
        _ => false,
    }
}

/// A name of a list that the room or the game gives: a light, a door, a room file.
fn pick(ui: &mut Ui, salt: &str, value: &mut String, known: &[String]) -> bool {
    picker(ui, salt, value, known, &[], false)
}

/// The name of a flag or a variable: one that an event of the game uses, or a new
/// one ("New..."). `unset`: the names that no event writes. Such a name has a mark:
/// it can be a name with a wrong letter.
pub(crate) fn pick_or_new(
    ui: &mut Ui,
    salt: &str,
    value: &mut String,
    known: &[String],
    unset: &[String],
) -> bool {
    let changed = picker(ui, salt, value, known, unset, true);
    if unset.contains(value) {
        ui.colored_label(HINT, format!("`{value}`: {NEVER_WRITTEN}"));
    }
    changed
}

fn pick_flag(ui: &mut Ui, value: &mut String, lists: &Lists) -> bool {
    pick_or_new(ui, "flag", value, &lists.flags, &lists.unset_flags)
}

fn pick_variable(ui: &mut Ui, value: &mut String, lists: &Lists) -> bool {
    let (known, unset) = (&lists.variables, &lists.unset_variables);
    pick_or_new(ui, "variable", value, known, unset)
}

fn seconds(ui: &mut Ui, value: &mut f32) -> bool {
    number(ui, "seconds", value, 0.05, 0.0..=600.0)
}

/// The widgets of the arguments of a built-in command. True: a value was changed.
fn arguments(ui: &mut Ui, command: &mut Builtin, lists: &Lists) -> bool {
    let whole = |ui: &mut Ui, value: &mut i32| row(ui, "value", |ui| ui.add(drag(value)).changed());
    match command {
        Builtin::SetFlag(name, value) => {
            pick_flag(ui, name, lists) | ui.checkbox(value, "set").changed()
        }
        Builtin::SetVariable(name, value) | Builtin::AddVariable(name, value) => {
            pick_variable(ui, name, lists) | whole(ui, value)
        }
        Builtin::Wait(time) | Builtin::FadeOut(time) | Builtin::FadeIn(time) => seconds(ui, time),
        // The widget of the conditions of a page.
        Builtin::If(list, ..) => {
            ui.label("Conditions (all must hold)");
            conditions(ui, list, lists)
        }
        Builtin::OpenDoor(name) | Builtin::CloseDoor(name) => {
            ui.label("door or mover");
            object_pick(name, &lists.ids, |name| {
                pick(ui, "mover", name, &lists.movers)
            })
        }
        Builtin::LightOn(name) | Builtin::LightOff(name) => object_pick(name, &lists.ids, |name| {
            pick(ui, "light", name, &lists.lights)
        }),
        Builtin::LightIntensity(name, value) => {
            object_pick(name, &lists.ids, |name| {
                pick(ui, "light", name, &lists.lights)
            }) | number(ui, "intensity", value, 0.1, 0.0..=10000.0)
        }
        Builtin::Message(message) => {
            let field = egui::TextEdit::multiline(message)
                .desired_rows(2)
                .desired_width(f32::INFINITY);
            ui.add(field).changed()
        }
        Builtin::Transfer(room, spawn) => {
            let files: Vec<String> = lists.rooms.iter().map(|room| room.0.clone()).collect();
            ui.label("room file");
            let picked = pick(ui, "room", room, &files);
            let found = lists.rooms.iter().find(|other| other.0 == *room);
            let (spawns, ids) = found.map_or((&[][..], &[][..]), |room| (&room.1, &room.2));
            object_pick(spawn, ids, |spawn| {
                // Another room: its first spawn point, if it has not the one of before.
                if let (true, false, Some(first)) = (picked, spawns.contains(spawn), spawns.first())
                {
                    spawn.clone_from(first);
                }
                ui.label("spawn point");
                picked | pick(ui, "spawn", spawn, spawns)
            })
        }
        Builtin::SetField(object, component, field, value) => {
            object_pick(object, &lists.ids, |object| {
                let objects: Vec<String> =
                    lists.objects.iter().map(|found| found.0.clone()).collect();
                ui.label("object");
                let picked_object = pick(ui, "object", object, &objects);
                let of_object = lists.objects.iter().find(|found| found.0 == *object);
                let components = of_object.map_or(&[][..], |found| &found.1);
                // Another object: its first component, unless it has the one of before.
                if let (true, false, Some(first)) = (
                    picked_object,
                    components.contains(component),
                    components.first(),
                ) {
                    component.clone_from(first);
                }
                ui.label("component");
                let picked_component = pick(ui, "component", component, components);
                let fields = lists.fields.get(component.as_str());
                let fields = fields.map_or(&[][..], Vec::as_slice);
                let names: Vec<String> = fields.iter().map(|found| found.name.to_owned()).collect();
                if let (true, false, Some(first)) = (
                    picked_object || picked_component,
                    names.contains(field),
                    fields.first(),
                ) {
                    (*field, *value) = (
                        first.name.to_owned(),
                        field_value(lists, object, component, first),
                    );
                }
                let mut changed = picked_object || picked_component;
                ui.label("field");
                let before = field.clone();
                changed |= pick(ui, "field", field, &names);
                if let Some(found) = fields.iter().find(|found| found.name == field) {
                    if *field != before {
                        *value = field_value(lists, object, component, found);
                    }
                    ui.label("value");
                    let objects = crate::fields::Lists::objects(&lists.ids);
                    changed |= crate::fields::show(ui, &found.kind, value, objects);
                }
                changed
            })
        }
        Builtin::CallScript(name) => {
            ui.label("script function");
            pick(ui, "function", name, &lists.functions)
        }
        Builtin::Signal(object, output) => object_pick(object, &lists.ids, |object| {
            let objects: Vec<String> = lists.outputs.iter().map(|found| found.0.clone()).collect();
            ui.label("object");
            let picked = pick(ui, "object", object, &objects);
            let of_object = lists.outputs.iter().find(|found| found.0 == *object);
            let outputs = of_object.map_or(&[][..], |found| &found.1);
            // Another object picked: its first output, if it has not the one of before.
            if let (true, false, Some(first)) = (picked, outputs.contains(output), outputs.first())
            {
                output.clone_from(first);
            }
            ui.label("output");
            picked | pick(ui, "output", output, outputs)
        }),
        Builtin::Choice(question, answers) => {
            ui.label("question");
            let field = egui::TextEdit::multiline(question)
                .desired_rows(2)
                .desired_width(f32::INFINITY);
            let mut changed = ui.add(field).changed();
            // The commands of an answer are rows of the list, under its label.
            ui.label("answers");
            let mut removed = None;
            for (index, (answer, _)) in answers.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    if ui
                        .small_button("x")
                        .on_hover_text("Remove the answer, with its commands")
                        .clicked()
                    {
                        removed = Some(index);
                    }
                    let field = egui::TextEdit::singleline(answer).desired_width(f32::INFINITY);
                    changed |= ui.add(field).changed();
                });
            }
            if let Some(index) = removed {
                answers.remove(index);
                changed = true;
            }
            if ui.button("Add an answer").clicked() {
                answers.push(("Answer".to_owned(), Vec::new()));
                changed = true;
            }
            changed
        }
    }
}

/// A new command for a name of the "Add" menu, with arguments that exist where the
/// lists have some. `None`: a command of a module that gives no new command for the
/// name, and that has arguments.
fn new_command(
    name: &str,
    lists: &Lists,
    registry: &CommandRegistry,
    room: &RoomDef,
) -> Option<Command> {
    let first =
        |names: &[String], or: &str| names.first().cloned().unwrap_or_else(|| or.to_owned());
    let (file, spawns, spawn_ids) = lists.rooms.first().cloned().unwrap_or_default();
    // The id of the first object of a list of names.
    let first_object = |names: &[String]| object_ref(&first(names, ""), &lists.ids);
    let builtin = match name {
        "SetFlag" => Builtin::SetFlag(first(&lists.flags, "flag"), true),
        "SetVariable" => Builtin::SetVariable(first(&lists.variables, "variable"), 0),
        "AddVariable" => Builtin::AddVariable(first(&lists.variables, "variable"), 1),
        "Wait" => Builtin::Wait(1.0),
        "If" => {
            let flag = Condition::Flag(first(&lists.flags, "flag"));
            Builtin::If(vec![flag], Vec::new(), Vec::new())
        }
        "Choice" => {
            let answer = |text: &str| (text.to_owned(), Vec::new());
            Builtin::Choice("Question?".to_owned(), vec![answer("Yes"), answer("No")])
        }
        "OpenDoor" => Builtin::OpenDoor(first_object(&lists.movers)),
        "CloseDoor" => Builtin::CloseDoor(first_object(&lists.movers)),
        "LightOn" => Builtin::LightOn(first_object(&lists.lights)),
        "LightOff" => Builtin::LightOff(first_object(&lists.lights)),
        "LightIntensity" => Builtin::LightIntensity(first_object(&lists.lights), 1.0),
        "Message" => Builtin::Message(String::new()),
        "FadeOut" => Builtin::FadeOut(0.4),
        "FadeIn" => Builtin::FadeIn(0.4),
        "Transfer" => Builtin::Transfer(file, object_ref(&first(&spawns, ""), &spawn_ids)),
        // The first field of the first object with a value in the game.
        "SetField" => {
            let (object, components) = lists.objects.first().cloned().unwrap_or_default();
            let component = first(&components, "");
            let fields = lists
                .fields
                .get(&component)
                .and_then(|fields| fields.first());
            let (field, value) = fields.map_or_else(
                || (String::new(), Value::Null),
                |found| {
                    (
                        found.name.to_owned(),
                        field_value(lists, &object, &component, found),
                    )
                },
            );
            Builtin::SetField(object_ref(&object, &lists.ids), component, field, value)
        }
        "CallScript" => Builtin::CallScript(first(&lists.functions, "")),
        "Signal" => {
            let (object, outputs) = lists.outputs.first().cloned().unwrap_or_default();
            Builtin::Signal(object_ref(&object, &lists.ids), first(&outputs, ""))
        }
        // A command of another module: what the module gives for the name, or the
        // name alone for a command with no argument.
        other => {
            let new = registry.new_command(other, room);
            return new.or_else(|| registry.parse_command(other).ok());
        }
    };
    registry.command(builtin)
}

/// The condition rows of a page or of an `If`.
fn conditions(ui: &mut Ui, conditions: &mut Vec<Condition>, lists: &Lists) -> bool {
    let mut changed = false;
    let mut removed = None;
    for (index, condition) in conditions.iter_mut().enumerate() {
        ui.push_id(("condition", index), |ui| {
            if ui
                .small_button("x")
                .on_hover_text("Remove the condition")
                .clicked()
            {
                removed = Some(index);
            }
            changed |= condition_row(ui, condition, lists);
        });
    }
    if let Some(index) = removed {
        conditions.remove(index);
        changed = true;
    }
    if ui.button("Add a condition").clicked() {
        let flag = lists
            .flags
            .first()
            .cloned()
            .unwrap_or_else(|| "flag".to_owned());
        conditions.push(Condition::Flag(flag));
        changed = true;
    }
    changed
}

/// The widgets of one condition, from the field table of its type: its kind and its
/// values. A name field with `pick = "flags"` or `"variables"` picks a known name or
/// a new one. True: the user changed the condition.
fn condition_row<T: Fields + DeserializeOwned>(
    ui: &mut Ui,
    condition: &mut T,
    lists: &Lists,
) -> bool {
    let mut tree = condition.tree();
    if !with_picks(lists, |picks| {
        crate::fields::show(ui, &T::kind(), &mut tree, picks)
    }) {
        return false;
    }
    match maki_core::fields::read::<T>(tree) {
        Ok(changed) => {
            *condition = changed;
            true
        }
        Err(_) => false,
    }
}

/// Calls `draw` with what the picks of the field rows offer: the flags and the
/// variables (`pick = "flags"`, `"variables"`) and the objects.
fn with_picks<R>(lists: &Lists, draw: impl FnOnce(crate::fields::Lists) -> R) -> R {
    let names = |list: &str| match list {
        "flags" => lists.flags.clone(),
        "variables" => lists.variables.clone(),
        _ => Vec::new(),
    };
    let open = |list: &str| match list {
        "flags" => Some(lists.unset_flags.clone()),
        "variables" => Some(lists.unset_variables.clone()),
        _ => None,
    };
    draw(crate::fields::Lists {
        names: &names,
        open: &open,
        objects: &lists.ids,
    })
}

/// The names that the pickers offer for the open room, and what the checks find in
/// one event (`None`: no event).
pub(crate) fn lists_and_problems(
    editor: &mut Editor,
    index: Option<usize>,
) -> (Lists, Vec<Problem>) {
    let registry = editor.registry.clone();
    let others = editor.others().to_vec();
    let used = names(&editor.document, &others, &registry);
    let mut problems: Vec<Problem> = problems(&editor.document, &others, &registry);
    problems.retain(|problem| Some(problem.object) == index);
    let room = &editor.document.room;
    let ids = object_names(&room.objects);
    let own = (file_name(&editor.document), room.spawn_names(), ids.clone());
    let other_rooms = others.iter().map(|other| {
        let ids = object_names(&other.room.objects);
        (other.file.clone(), other.room.spawn_names(), ids)
    });
    // The objects with a component, by name: what a command can name.
    let with = |kinds: &[&str]| -> Vec<String> {
        let objects = room.objects.iter();
        let objects = objects.filter(|object| {
            kinds
                .iter()
                .any(|kind| object.components.contains_key(*kind))
        });
        objects.filter_map(|object| object.name.clone()).collect()
    };
    let lists = Lists {
        flags: used.flags.keys().cloned().collect(),
        variables: used.variables.keys().cloned().collect(),
        unset_flags: Names::never_written(&used.flags),
        unset_variables: Names::never_written(&used.variables),
        lights: with(&["light"]),
        movers: with(&["door", "mover"]),
        rooms: std::iter::once(own).chain(other_rooms).collect(),
        ids,
        // `live_lists` fills the objects, the fields, and their values.
        objects: Vec::new(),
        fields: BTreeMap::new(),
        current: BTreeMap::new(),
        functions: registry
            .functions()
            .into_iter()
            .map(str::to_owned)
            .collect(),
        outputs: room
            .objects
            .iter()
            .filter_map(|object| {
                let outputs = registry.outputs_of(object);
                let outputs = outputs.into_iter().map(str::to_owned).collect::<Vec<_>>();
                (!outputs.is_empty()).then(|| Some((object.name.clone()?, outputs)))?
            })
            .collect(),
        own_outputs: index.and_then(|index| room.objects.get(index)).map_or_else(
            Vec::new,
            |object| {
                let outputs = registry.outputs_of(object).into_iter();
                outputs.map(str::to_owned).collect()
            },
        ),
        row_keys: Default::default(),
    };
    (lists, problems)
}

/// Fills `lists` from the cache, made again when the room or its directory changed.
pub(crate) fn cached_live_lists(
    world: &World,
    room: &RoomDef,
    directory: &std::path::Path,
    cache: &mut Option<(RoomDef, std::path::PathBuf, LiveLists)>,
    lists: &mut Lists,
) {
    let same =
        |(old, at, _): &(RoomDef, std::path::PathBuf, LiveLists)| old == room && at == directory;
    if !cache.as_ref().is_some_and(same) {
        let live = live_lists(world, room, directory);
        *cache = Some((room.clone(), directory.to_owned(), live));
    }
    let Some((.., live)) = cache else { return };
    let live = live.clone();
    (lists.objects, lists.fields, lists.current) = (live.objects, live.fields, live.current);
}

/// The objects that `SetField` can change (`RoomDef::live_objects`, prefab
/// instances too) with their live components, the fields of their types that change
/// in the game, and the value that each field has now. `directory`: of the room file.
fn live_lists(world: &World, room: &RoomDef, directory: &std::path::Path) -> LiveLists {
    let mut lists = LiveLists::default();
    let (Some(types), tables) = (
        world.get_resource::<ComponentTypes>(),
        world.get_resource::<FieldTypes>(),
    ) else {
        return lists;
    };
    for (name, live) in room.live_objects(directory, types) {
        for (component, text) in &live {
            let table = tables.and_then(|tables| tables.get(component));
            let Some(Kind::Group(fields)) = table.map(|table| &table.kind) else {
                continue;
            };
            let changes = |field: &&Field| types.is_live_field(component, field.name);
            let fields: Vec<Field> = fields.iter().filter(changes).cloned().collect();
            for field in &fields {
                if let Some(now) = types.field(component, text, field.name) {
                    let key = (name.clone(), component.clone(), field.name.to_owned());
                    lists.current.insert(key, now);
                }
            }
            lists.fields.insert(component.clone(), fields);
        }
        lists.objects.push((name, live.into_keys().collect()));
    }
    lists
}

/// The value of a new `SetField` row or of another field: the value that the field
/// has now in the document, else its default.
fn field_value(lists: &Lists, object: &str, component: &str, field: &Field) -> Value {
    let key = (
        object.to_owned(),
        component.to_owned(),
        field.name.to_owned(),
    );
    let now = lists.current.get(&key).cloned();
    now.unwrap_or_else(|| field.default.clone().unwrap_or_else(|| field.kind.zero()))
}

/// What the property panel shows of the event of an object: the problems of the
/// event itself (a name that another event has). The name is that of the object;
/// the area is its component `area`.
pub(crate) fn event_properties(ui: &mut Ui, editor: &mut Editor, index: usize) {
    let (_, problems) = lists_and_problems(editor, Some(index));
    for problem in problems.iter().filter(|problem| problem.command.is_none()) {
        ui.colored_label(color(problem), problem.text.as_str());
    }
    ui.weak("Pages and commands: the panel \"Events\".");
}

/// The event panel, under the picture: the pages with their conditions and trigger
/// at the left, the command rows in the middle, the arguments of the selected row
/// at the right. `index`: the object of the event. True: the event was changed.
/// `None`: the object has no event that reads.
pub(crate) fn event_panel(
    ui: &mut Ui,
    world: &mut World,
    editor: &mut Editor,
    index: usize,
) -> Option<bool> {
    let registry = editor.registry.clone();
    let (mut lists, problems) = lists_and_problems(editor, Some(index));
    let room_directory = editor
        .document
        .path
        .parent()
        .unwrap_or(std::path::Path::new(""));
    let (room, cache) = (&editor.document.room, &mut editor.event_ui.live);
    cached_live_lists(world, room, room_directory, cache, &mut lists);
    let actions = [crate::DELETE, crate::DUPLICATE, crate::COPY, crate::PASTE];
    lists.row_keys = actions.map(|action| maki_ui::key_label(world, action));
    let problems: Vec<&Problem> = problems.iter().collect();
    let editors = world
        .get_resource::<ArgumentEditors>()
        .cloned()
        .unwrap_or_default();

    let Editor {
        document,
        event_ui,
        queue,
        ..
    } = editor;
    // For the file pickers of the arguments.
    let directory = document.path.parent().unwrap_or(std::path::Path::new(""));
    crate::widgets::set_room_directory(ui, directory, &editor.converters);
    let mut event = document.event(index, &registry)?.ok()?;
    let id = document.room.objects[index].id;
    let facts = maki_events::Facts::new(world, id);
    let room = &document.room;
    let mut changed = false;
    if event.pages.is_empty() && event.wires.is_empty() {
        event.pages.push(empty_page());
        changed = true;
    }
    // Another event: a page or a wire that it has.
    let lists_count = event.pages.len() + event.wires.len();
    event_ui.page = event_ui.page.min(lists_count - 1);
    let scroll = |salt: &'static str| egui::ScrollArea::both().id_salt(salt).auto_shrink(false);

    let frame = egui::Frame::NONE.inner_margin(6);
    let part = |panel: egui::Panel| {
        panel
            .resizable(true)
            .default_size(SIDE_WIDTH)
            .size_range(200.0..=600.0)
            .frame(frame)
    };
    part(egui::Panel::left("event pages")).show(ui, |ui| {
        scroll("pages").show(ui, |ui| {
            changed |= pages(ui, &mut event.pages, id, event_ui, &facts, &lists, queue);
            ui.separator();
            changed |= wires(ui, &mut event, event_ui, &lists);
        });
    });
    let at = event_ui.page;
    // Another event or another page: the row and the typed command are of the old one.
    if std::mem::replace(&mut event_ui.shown, (index, at)) != (index, at) {
        event_ui.row = None;
        event_ui.draft = None;
    }
    // A page, or a wire: its commands.
    let Some(list) = event.list_mut(at) else {
        return Some(changed);
    };
    // The rows have a panel of their own: at any width the arguments are next to their rows.
    part(egui::Panel::left("event commands")).show(ui, |ui| {
        scroll("commands").show(ui, |ui| {
            let places = (&lists, &registry, room);
            changed |= command_rows(ui, list, event_ui, places, &problems, at);
        });
    });
    egui::Frame::NONE.inner_margin(6).show(ui, |ui| {
        scroll("arguments").show(ui, |ui| {
            let places = (&lists, &registry, &editors, room);
            changed |= command_arguments(ui, list, event_ui, places);
        });
    });
    if changed {
        document.set_event(index, &event);
    }
    Some(changed)
}

/// The width of the left and of the right part of the event panel at the start.
const SIDE_WIDTH: f32 = 340.0;

/// The left part of the event panel: the pages as tabs, and the conditions and the
/// trigger of the page that shows.
fn pages(
    ui: &mut Ui,
    pages: &mut Vec<Page>,
    id: Option<ObjectId>,
    event_ui: &mut EventUi,
    facts: &maki_events::Facts,
    lists: &Lists,
    queue: &mut Vec<EditorCommand>,
) -> bool {
    let mut changed = false;
    if pages.is_empty() {
        if ui.button("Add a page").clicked() {
            pages.push(empty_page());
            (event_ui.page, event_ui.row) = (0, None);
            changed = true;
        }
        return changed;
    }
    // The pages, as tabs. The one that the game would run now has a star.
    let count = pages.len();
    let active = active_page(pages, facts);
    ui.horizontal_wrapped(|ui| {
        ui.label("Page");
        for page in 0..count {
            let star = if active == Some(page) { " *" } else { "" };
            let tab = ui
                .selectable_label(event_ui.page == page, format!("{}{star}", page + 1))
                .on_hover_text(
                    "* the page that is active with the flags and variables of the \
                     Game state panel: the last one whose conditions hold",
                );
            if tab.clicked() {
                event_ui.page = page;
                event_ui.row = None;
            }
        }
    });
    // A wire is shown: no page part.
    if event_ui.page >= count {
        return changed;
    }
    let at = event_ui.page;
    ui.horizontal(|ui| {
        let before = (pages.clone(), at);
        if ui.button("Add").clicked() {
            pages.insert(at + 1, empty_page());
            event_ui.page = at + 1;
        }
        if ui
            .button("Duplicate")
            .on_hover_text("A copy of the page, after it")
            .clicked()
        {
            pages.insert(at + 1, pages[at].clone());
            event_ui.page = at + 1;
        }
        if ui
            .add_enabled(count > 1, egui::Button::new("Delete"))
            .clicked()
        {
            pages.remove(at);
            event_ui.page = at.min(count - 2);
        }
        if ui
            .add_enabled(at > 0, egui::Button::new("<"))
            .on_hover_text("Earlier in the order")
            .clicked()
        {
            pages.swap(at, at - 1);
            event_ui.page = at - 1;
        }
        if ui
            .add_enabled(at + 1 < count, egui::Button::new(">"))
            .on_hover_text("Later: it wins over the pages before it")
            .clicked()
        {
            pages.swap(at, at + 1);
            event_ui.page = at + 1;
        }
        if before != (pages.clone(), event_ui.page) {
            event_ui.row = None;
            changed = true;
        }
    });
    let at = event_ui.page;
    let page = &mut pages[at];

    ui.label("Conditions (all must hold)");
    changed |= ui
        .indent("conditions", |ui| {
            conditions(ui, &mut page.conditions, lists)
        })
        .inner;
    let triggers = [
        (
            "Interact: the player faces the area and presses interact",
            Trigger::Interact,
        ),
        ("Touch: the player walks into the area", Trigger::Touch),
        ("Autorun: when the page gets active", Trigger::Autorun),
        (
            "Parallel: again and again, the player keeps control",
            Trigger::Parallel,
        ),
    ];
    changed |= choice(ui, "trigger", &mut page.trigger, &triggers, PartialEq::eq);
    if page.trigger == Trigger::Autorun {
        ui.weak("Autorun runs one time when the page gets active (also when the room loads).");
        changed |= ui
            .checkbox(&mut page.repeat, "repeat")
            .on_hover_text("Runs again, one step after the run ends, while this page is active")
            .changed();
    }
    ui.separator();
    // The editor gives each object an id: `None` only in a room it did not open.
    if let Some(id) = id
        && ui
            .button("Test this page")
            .on_hover_text("Plays the room and runs this page, one command for each step")
            .clicked()
    {
        queue.push(EditorCommand::Test(id, at));
    }
    changed
}

/// The wires of the object, as tabs under the pages: the output of the wire that
/// shows, and buttons to add and delete one. The index of a wire in
/// `EventUi::page` is after the pages.
fn wires(ui: &mut Ui, event: &mut EventPages, event_ui: &mut EventUi, lists: &Lists) -> bool {
    let first = event.pages.len();
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.label("Wires");
        for index in 0..event.wires.len() {
            let text = Wire::label(&event.wires, index);
            let tab = ui.selectable_label(event_ui.page == first + index, text);
            if tab.clicked() {
                (event_ui.page, event_ui.row) = (first + index, None);
            }
        }
    });
    let add = ui
        .button("Add a wire")
        .on_hover_text("When an output of this object is sent, the commands of the wire run");
    if add.clicked() {
        let output = lists.own_outputs.first().cloned().unwrap_or_default();
        event.wires.push(Wire {
            output,
            commands: Vec::new(),
        });
        (event_ui.page, event_ui.row) = (first + event.wires.len() - 1, None);
        changed = true;
    }
    let shown = event_ui.page.checked_sub(first);
    let Some(at) = shown.filter(|at| *at < event.wires.len()) else {
        return changed;
    };
    ui.label("output");
    changed |= pick(
        ui,
        "output",
        &mut event.wires[at].output,
        &lists.own_outputs,
    );
    if ui.button("Delete the wire").clicked() {
        event.wires.remove(at);
        (event_ui.page, event_ui.row) = (event_ui.page.saturating_sub(1), None);
        changed = true;
    }
    changed
}

/// Red for an error of a check (the game does not load the room), else yellow.
fn color(problem: &Problem) -> Color32 {
    if problem.warning { HINT } else { ERROR }
}

pub(crate) fn empty_page() -> Page {
    Page {
        conditions: Vec::new(),
        trigger: Trigger::Interact,
        repeat: false,
        commands: Vec::new(),
    }
}

/// The middle part of the event panel: the command rows of a page.
pub(crate) fn command_rows(
    ui: &mut Ui,
    list: &mut Vec<Command>,
    event_ui: &mut EventUi,
    (lists, registry, room): (&Lists, &CommandRegistry, &RoomDef),
    problems: &[&Problem],
    page_index: usize,
) -> bool {
    let all = rows(list, registry, &lists.ids);
    // A row that is not there after a change of the list is not selected.
    if event_ui
        .row
        .as_ref()
        .is_some_and(|row| !all.iter().any(|other| other.0 == *row))
    {
        event_ui.row = None;
    }
    let selected = event_ui.row.clone();
    let selected_command = selected.as_ref().filter(|row| row.kind == RowKind::Command);
    // Where "Add" and "Paste" put a command: after the selected row, or at the end.
    let place = selected.as_ref().map_or((Vec::new(), list.len()), |row| {
        row.insert_place(list, registry)
    });
    let mut changed = false;
    let mut insert: Vec<Command> = Vec::new();
    let mut row_command = None;

    ui.horizontal(|ui| {
        ui.label("Commands");
        let names: Vec<String> = registry.names().into_iter().map(str::to_owned).collect();
        let add = ui.make_persistent_id("add command");
        // The hover text of a name: the line of help of its module.
        let help = |name: &str| registry.help(name);
        let picked = described_list(ui, add, "Add".into(), &names, &[], false, &help, None);
        if let Some(Picked::Name(name)) = picked {
            match new_command(&name, lists, registry, room) {
                Some(command) => insert.push(command),
                // A command with arguments that the editor does not know: the
                // user types them.
                None => {
                    let (list, index) = place.clone();
                    let text = format!("{name}(");
                    event_ui.draft = Some(Draft {
                        list,
                        index,
                        replace: false,
                        text,
                    });
                }
            }
        }
        let some = selected_command.is_some();
        let buffer = !event_ui.clipboard.is_empty();
        let [delete, duplicate, copy, paste] = &lists.row_keys;
        for (text, enabled, command, key) in [
            ("Delete", some, RowCommand::Delete, delete),
            ("Duplicate", some, RowCommand::Duplicate, duplicate),
            ("Copy", some, RowCommand::Copy, copy),
            ("Paste", buffer, RowCommand::Paste, paste),
        ] {
            let button = ui.add_enabled(enabled, egui::Button::new(text));
            if button.on_hover_text(key.as_str()).clicked() {
                row_command = Some(command);
            }
        }
    });
    if let Some(command) = row_command {
        event_ui.focused = true;
        return crate::event_panel::row_command(list, event_ui, registry, command);
    }
    let range = event_ui.selected();

    // The rows. A click selects one; a drag moves a command to the row where the
    // button goes up.
    let released = ui.ctx().input(|input| input.pointer.any_released());
    let mut drop = None;
    for (row, depth, text) in &all {
        ui.horizontal(|ui| {
            ui.add_space(*depth as f32 * 18.0);
            // The problem has the command, not the row: two equal commands of a
            // page have the same problem.
            let command = Some(row)
                .filter(|row| row.kind == RowKind::Command)
                .and_then(|row| row.command(list, registry));
            let bad = problems.iter().find(|problem| {
                let of_problem = problem.command.as_ref();
                command.is_some_and(|command| of_problem == Some(&(page_index, command.clone())))
            });
            // The text of `rows`: the line for a reader, with "Else if" and names.
            let label = egui::RichText::new(text);
            let label = match bad {
                Some(problem) => label.color(color(problem)),
                None => label,
            };
            // Selected: the row of the last click, and the rows of a Shift click.
            let in_range = range.as_ref().is_some_and(|(list, range)| {
                row.kind == RowKind::Command && *list == row.list && range.contains(&row.index)
            });
            let is_selected = selected.as_ref() == Some(row) || in_range;
            let response = ui.selectable_label(is_selected, label);
            let response = response.interact(egui::Sense::drag());
            if response.clicked() {
                // Shift: the rows from the row of the click before to this one, in
                // the same list.
                let shift = ui.input(|input| input.modifiers.shift);
                let old = event_ui
                    .selected()
                    .filter(|(list, _)| shift && row.kind == RowKind::Command && *list == row.list);
                let other = old.map(|(_, range)| {
                    let was = event_ui.row.as_ref().map_or(row.index, |row| row.index);
                    // The end that the click before did not set.
                    if *range.start() == was {
                        *range.end()
                    } else {
                        *range.start()
                    }
                });
                event_ui.select_rows(row.clone(), other);
            }
            if response.drag_started() && row.kind == RowKind::Command {
                event_ui.dragged = Some(row.clone());
            }
            let over = response.contains_pointer()
                && event_ui.dragged.as_ref().is_some_and(|from| from != row);
            if over {
                // Where the dragged command goes: a line above this row, at the
                // indent of its list.
                let edge = response.rect;
                let stroke = egui::Stroke::new(2.0, HINT);
                ui.painter()
                    .hline(edge.left()..=edge.left() + 260.0, edge.top() - 1.0, stroke);
                if released {
                    drop = Some(row.clone());
                }
            }
            if let Some(problem) = bad {
                ui.colored_label(color(problem), problem.text.as_str());
            }
        });
    }
    if all.is_empty() {
        ui.weak("No command.");
    }
    if let (Some(from), Some(to)) = (event_ui.dragged.clone(), drop) {
        // Onto a command: before it. Onto `Else` or `End`: where a new command goes.
        let to = match to.kind {
            RowKind::Command => (to.list.clone(), to.index),
            _ => to.insert_place(list, registry),
        };
        if move_command(list, &from, to, registry) {
            event_ui.row = None;
            changed = true;
        }
    }
    if released {
        event_ui.dragged = None;
    }

    if !insert.is_empty() {
        let (path, index) = place.clone();
        let count = insert.len();
        let done = edit_list(list, &path, registry, |list| {
            let index = index.min(list.len());
            list.splice(index..index, insert);
        });
        if done.is_some() {
            // The last new command is the selected one.
            event_ui.row = Some(Row {
                list: path,
                index: index + count - 1,
                kind: RowKind::Command,
            });
            changed = true;
        }
    }

    changed
}

/// The right part of the event panel: the arguments of the selected command row, or
/// the text of a command that is typed.
pub(crate) fn command_arguments(
    ui: &mut Ui,
    list: &mut Vec<Command>,
    event_ui: &mut EventUi,
    (lists, registry, editors, room): (&Lists, &CommandRegistry, &ArgumentEditors, &RoomDef),
) -> bool {
    let mut changed = false;
    ui.label("Arguments");
    // The arguments of the selected command.
    if let Some(row) = event_ui
        .row
        .clone()
        .filter(|row| row.kind == RowKind::Command)
        && let Some(command) = row.command(list, registry).cloned()
    {
        let new = if let Some(mut builtin) = command.get::<Builtin>().cloned() {
            arguments(ui, &mut builtin, lists)
                .then(|| registry.command(builtin))
                .flatten()
        } else if let Some(new) = editors
            .0
            .iter()
            .find_map(|edit| edit(ui, &command, registry, room))
        {
            new
        } else if let Some(new) = with_picks(lists, |picks| {
            field_arguments(ui, &command, registry, picks)
        }) {
            new
        } else {
            // As a text. The buffer is the draft: it can be a text that is not a
            // command while the user types.
            let draft = event_ui.draft.get_or_insert_with(|| Draft {
                list: row.list.clone(),
                index: row.index,
                replace: true,
                text: ron_options().to_string(&command).unwrap_or_default(),
            });
            draft_field(ui, draft, registry).filter(|new| *new != command)
        };
        if let Some(new) = new {
            edit_list(list, &row.list, registry, |list| list[row.index] = new);
            changed = true;
        }
    } else if event_ui.draft.as_ref().is_some_and(|draft| draft.replace) {
        // The text of a command that is not selected now.
        event_ui.draft = None;
    } else if let Some(draft) = &mut event_ui.draft {
        // A new command that is typed. It goes into the list with the button: a
        // text can read as a command before all of it is typed (`Wait(1` and `0)`).
        let new = draft_field(ui, draft, registry);
        let insert = ui
            .add_enabled(new.is_some(), egui::Button::new("Put into the list"))
            .clicked();
        if let Some(new) = new.filter(|_| insert) {
            let (path, index) = (draft.list.clone(), draft.index);
            let done = edit_list(list, &path, registry, |list| {
                list.insert(index.min(list.len()), new)
            });
            if done.is_some() {
                event_ui.row = Some(Row {
                    list: path,
                    index,
                    kind: RowKind::Command,
                });
                event_ui.draft = None;
                changed = true;
            }
        }
    }
    if event_ui.row.is_none() && event_ui.draft.is_none() {
        ui.weak("Select a command row.");
    }
    changed
}

/// The widgets of the arguments of a command from the field table of its enum
/// (`CommandRegistry::add_fields`). `None`: its module gave no table. `Some(new)`:
/// the changed command, if the user changed it.
pub(crate) fn field_arguments(
    ui: &mut Ui,
    command: &Command,
    registry: &CommandRegistry,
    lists: crate::fields::Lists,
) -> Option<Option<Command>> {
    let (kind, mut tree) = registry.fields(command)?;
    let changed = crate::fields::show_arguments(ui, &kind, &mut tree, lists);
    Some(changed.then(|| registry.from_tree(command, tree)).flatten())
}

/// The text field of a command as a room file has it, checked with each key.
/// Returns the command if the text reads as one.
fn draft_field(ui: &mut Ui, draft: &mut Draft, registry: &CommandRegistry) -> Option<Command> {
    ui.label(if draft.replace {
        "The command, as in a room file"
    } else {
        "New command, as in a room file"
    });
    ui.add(egui::TextEdit::singleline(&mut draft.text).desired_width(f32::INFINITY));
    // One line for the two cases: a button below does not move while the user types.
    match registry.parse_command(&draft.text) {
        Ok(command) => {
            ui.weak("Reads as a command.");
            Some(command)
        }
        Err(error) => {
            ui.colored_label(ERROR, error.code.to_string());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use maki_core::ConfigFile;

    use super::*;

    /// The texts that one frame of `draw` shows (`Shape::Text` galleys).
    fn shown_texts(draw: impl FnMut(&mut Ui)) -> Vec<String> {
        let mut output = egui::Context::default().run_ui(egui::RawInput::default(), draw);
        // egui panics when texture changes are dropped unread.
        output.textures_delta.clear();
        let texts = output.shapes.iter().filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        });
        texts.collect()
    }

    /// Each form of a condition is drawn from its field table: the kind, the picked
    /// name, the compare, the number. With no input nothing changes.
    #[test]
    fn each_condition_form_is_drawn_from_its_field_table() {
        let lists = Lists {
            flags: vec!["lit".to_owned()],
            variables: vec!["turns".to_owned()],
            ..Lists::default()
        };
        let forms = [
            (
                Condition::Flag("lit".to_owned()),
                vec!["flag is set", "lit"],
            ),
            (
                Condition::NotFlag("lit".to_owned()),
                vec!["flag is not set", "lit"],
            ),
            (
                Condition::Variable("turns".to_owned(), maki_events::Compare::AtLeast, 2),
                vec!["variable", "turns", "is", "at least", "value", "2"],
            ),
            (
                Condition::Switch(ObjectId(20), "open".to_owned()),
                vec!["switch is on", "object", "chest", "switch", "open"],
            ),
            (
                Condition::NotSwitch(ObjectId(20), "open".to_owned()),
                vec!["switch is off", "chest", "open"],
            ),
            (
                Condition::Number(
                    ObjectId(20),
                    "coins".to_owned(),
                    maki_events::Compare::Less,
                    3,
                ),
                vec![
                    "number of an object",
                    "chest",
                    "number",
                    "coins",
                    "less than",
                    "3",
                ],
            ),
            // A list of conditions in a condition: a row for each, and "Add".
            (
                Condition::Any(vec![Condition::NotFlag("lit".to_owned())]),
                vec!["any of", "flag is not set", "lit", "Add"],
            ),
        ];
        let lists = Lists {
            ids: vec![(ObjectId(20), "chest".to_owned())],
            ..lists
        };
        for (condition, expected) in forms {
            let mut shown = condition.clone();
            let texts = shown_texts(|ui| assert!(!condition_row(ui, &mut shown, &lists)));
            for text in expected {
                assert!(texts.iter().any(|found| found == text), "{text}: {texts:?}");
            }
            assert_eq!(shown, condition);
        }
    }

    /// The component `state` in the inspector: its lists of named values, drawn from
    /// its field table, with a button to add an item.
    #[test]
    fn the_state_component_is_drawn_from_its_field_table() {
        let state = maki_events::ObjectState {
            switches: vec![maki_events::NamedSwitch {
                name: "open".to_owned(),
                on: true,
            }],
            numbers: vec![maki_events::NamedNumber {
                name: "coins".to_owned(),
                value: 5,
            }],
        };
        let kind = maki_events::ObjectState::kind();
        let mut tree = state.tree();
        let texts = shown_texts(|ui| {
            assert!(!crate::fields::show(
                ui,
                &kind,
                &mut tree,
                crate::fields::Lists::objects(&[])
            ));
        });
        for text in [
            "switches", "open", "on", "numbers", "coins", "5", "Add", "x",
        ] {
            assert!(texts.iter().any(|found| found == text), "{text}: {texts:?}");
        }
        assert_eq!(
            maki_core::fields::read::<maki_events::ObjectState>(tree).unwrap(),
            state
        );
    }

    /// A change of the kind keeps a value of the same kind at the same place: "flag
    /// is set" to "flag is not set" keeps the flag; to "variable" it does not.
    #[test]
    fn a_change_of_the_kind_of_a_condition_keeps_the_name_of_the_same_list() {
        let maki_core::fields::Kind::Choice(variants) = Condition::kind() else {
            panic!("an enum has a choice");
        };
        let tree = Condition::Flag("lit".to_owned()).tree();
        let change = |to: usize| {
            let tree = crate::fields::change_variant(variants.first(), tree.clone(), &variants[to]);
            maki_core::fields::read::<Condition>(tree).unwrap()
        };
        assert_eq!(change(1), Condition::NotFlag("lit".to_owned()));
        let compare = maki_events::Compare::Equals;
        assert_eq!(change(2), Condition::Variable(String::new(), compare, 0));
    }

    /// A new form of a condition is a variant: the panel draws its widgets with no
    /// code of the editor.
    #[test]
    fn a_new_condition_form_has_widgets_with_no_editor_code() {
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Fields)]
        enum Test {
            Flag(#[field(pick = "flags")] String),
            #[field(label = "hour of the day")]
            Hour(#[field(label = "after")] i32),
        }
        let mut hour = Test::Hour(20);
        let texts = shown_texts(|ui| assert!(!condition_row(ui, &mut hour, &Lists::default())));
        for text in ["hour of the day", "after", "20"] {
            assert!(texts.iter().any(|found| found == text), "{text}: {texts:?}");
        }
    }

    /// The pickers of `SetField`: the objects with a component that has a value in
    /// the game (a light, a prop, a door), the fields that change in the game, and a
    /// new row that starts with the value the field has now.
    #[test]
    fn the_set_field_pickers_offer_the_live_components_and_their_fields() {
        let mut app = App::new(ConfigFile::default()).unwrap();
        app.add_module(maki_platform::PlatformModule)
            .add_module(maki_scene::SceneModule)
            .add_module(maki_events::EventsModule);
        let text = maki_scene::room_text(
            6.0,
            6.0,
            3.0,
            &[
                r#"(id: 1, name: "lamp", components: {"light": (radius: 0.1, color: (1.0, 1.0, 1.0), intensity: 8.0)})"#,
                r#"(id: 2, name: "crate", components: {"prop": (mesh: Cube(1.0))})"#,
                r#"(name: "gate", components: {"door": (seconds: 1.0, color: (0.3, 0.2, 0.1), open: false)})"#,
                r#"(name: "sign", components: {"event": (pages: [])})"#,
            ],
        );
        let room = maki_scene::parse_room(&text).unwrap();
        let mut lists = Lists {
            flags: Vec::new(),
            variables: Vec::new(),
            unset_flags: Vec::new(),
            unset_variables: Vec::new(),
            lights: Vec::new(),
            movers: Vec::new(),
            rooms: Vec::new(),
            ids: Vec::new(),
            objects: Vec::new(),
            fields: BTreeMap::new(),
            current: BTreeMap::new(),
            functions: Vec::new(),
            outputs: Vec::new(),
            own_outputs: Vec::new(),
            row_keys: Default::default(),
        };
        let mut cache = None;
        cached_live_lists(
            app.world(),
            &room,
            std::path::Path::new(""),
            &mut cache,
            &mut lists,
        );
        let names: Vec<&str> = lists.objects.iter().map(|found| found.0.as_str()).collect();
        assert_eq!(names, ["lamp", "crate", "gate"]);
        assert_eq!(lists.objects[0].1, ["light"]);
        let fields = |component: &str| -> Vec<&str> {
            lists.fields[component].iter().map(|f| f.name).collect()
        };
        let light = fields("light");
        assert!(
            light.contains(&"intensity") && !light.contains(&"sound"),
            "{light:?}"
        );
        assert_eq!(fields("prop"), ["collider"]);
        assert_eq!(fields("door"), ["seconds", "open"]);
        // A new row: the first field of the first object, with the value it has now.
        let registry = app.world().resource::<CommandRegistry>().clone();
        lists.ids = crate::widgets::object_names(&room.objects);
        let new = new_command("SetField", &lists, &registry, &room).unwrap();
        let Some(Builtin::SetField(object, component, field, value)) = new.get() else {
            panic!("not a SetField row");
        };
        assert_eq!((*object, component.as_str()), (ObjectId(1), "light"));
        let key = ("lamp".to_owned(), component.clone(), field.clone());
        assert_eq!(Some(value), lists.current.get(&key));
        let intensity = (
            "lamp".to_owned(),
            "light".to_owned(),
            "intensity".to_owned(),
        );
        assert_eq!(
            lists.current[&intensity],
            maki_core::fields::number(8.0, false)
        );
    }

    /// The star of the active page reads the state of the object of the event.
    #[test]
    fn the_active_page_star_reads_object_state() {
        let mut world = World::new();
        world.insert_resource(maki_events::GameState::default());
        let mut room = maki_scene::CurrentRoom::default();
        room.path = "a.ron".into();
        world.insert_resource(room);
        let mut states = maki_events::ObjectStates::default();
        states
            .get_mut("a.ron", ObjectId(20))
            .switches
            .insert("open".into(), true);
        world.insert_resource(states);
        let open = Condition::Switch(ObjectId::default(), "open".to_owned());
        let mut list = vec![empty_page(), empty_page()];
        list[1].conditions.push(open);
        let facts = maki_events::Facts::new(&world, Some(ObjectId(20)));
        let texts = shown_texts(|ui| {
            let mut event_ui = EventUi::default();
            let (lists, mut queue) = (Lists::default(), Vec::new());
            let id = Some(ObjectId(20));
            pages(ui, &mut list, id, &mut event_ui, &facts, &lists, &mut queue);
        });
        assert!(texts.iter().any(|text| text == "2 *"), "{texts:?}");
    }
}
