//! The event panel: the property panel of an event. An event has pages; a page has
//! conditions, a trigger, and a list of commands, as in RPG Maker.
//!
//! Names are picked, not typed, where the editor knows them: a light, a door, or a
//! mover of the room; a flag or a variable that an event of any room file uses; a
//! room file and one of its spawn points. A command of a module that the editor does
//! not know has a text field with the command as a room file has it.

use std::sync::Arc;

use bevy_ecs::resource::Resource;
use bevy_ecs::world::World;
use maki_core::App;
use maki_events::{
    Area, Builtin, Command, CommandRegistry, CommandSet, Compare, Condition, GameState, Page,
    Problem, Trigger, active_page,
};
use maki_scene::{RoomDef, ron_options};
use maki_ui::egui::{self, Color32, Ui};

use crate::events::{
    ListPath, Names, Row, RowKind, edit_list, file_name, move_command, names, problems, rows,
};
use crate::widgets::{choice, drag, number, optional, row, same_kind, text};
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
/// command can have; `crate::widgets` has the widgets. Without this, such a command
/// has a text field.
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

/// The names that the pickers offer.
struct Lists {
    flags: Vec<String>,
    variables: Vec<String>,
    /// The flags and the variables that an event reads and no event writes.
    unset_flags: Vec<String>,
    unset_variables: Vec<String>,
    lights: Vec<String>,
    /// Doors and movers: `OpenDoor` finds the two.
    movers: Vec<String>,
    /// Each room file with its spawn points, the open room too.
    rooms: Vec<(String, Vec<String>)>,
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
fn pick_or_new(
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
            pick(ui, "mover", name, &lists.movers)
        }
        Builtin::LightOn(name) | Builtin::LightOff(name) => pick(ui, "light", name, &lists.lights),
        Builtin::LightIntensity(name, value) => {
            pick(ui, "light", name, &lists.lights)
                | number(ui, "intensity", value, 0.1, 0.0..=10000.0)
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
            let mut changed = pick(ui, "room", room, &files);
            let spawns = lists.rooms.iter().find(|other| other.0 == *room);
            let spawns = spawns.map_or(&[][..], |room| &room.1);
            // Another room: its first spawn point, if it has not the one of before.
            if let (true, false, Some(first)) = (changed, spawns.contains(spawn), spawns.first()) {
                spawn.clone_from(first);
            }
            ui.label("spawn point");
            changed |= pick(ui, "spawn", spawn, spawns);
            changed
        }
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
    let (file, spawns) = lists.rooms.first().cloned().unwrap_or_default();
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
        "OpenDoor" => Builtin::OpenDoor(first(&lists.movers, "")),
        "CloseDoor" => Builtin::CloseDoor(first(&lists.movers, "")),
        "LightOn" => Builtin::LightOn(first(&lists.lights, "")),
        "LightOff" => Builtin::LightOff(first(&lists.lights, "")),
        "LightIntensity" => Builtin::LightIntensity(first(&lists.lights, ""), 1.0),
        "Message" => Builtin::Message(String::new()),
        "FadeOut" => Builtin::FadeOut(0.4),
        "FadeIn" => Builtin::FadeIn(0.4),
        "Transfer" => Builtin::Transfer(file, first(&spawns, "")),
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
            // One line for each part: the column is narrow.
            ui.horizontal(|ui| {
                if ui
                    .small_button("x")
                    .on_hover_text("Remove the condition")
                    .clicked()
                {
                    removed = Some(index);
                }
                // Another kind keeps the name if it is of the same list (flags or
                // variables), else it takes the first name of the other list.
                let name = |variable: bool| {
                    let names = if variable {
                        &lists.variables
                    } else {
                        &lists.flags
                    };
                    let first = || names.first().map_or("name", String::as_str).to_owned();
                    if condition.is_variable() == variable {
                        condition.name().to_owned()
                    } else {
                        first()
                    }
                };
                let kinds = [
                    ("flag is set", Condition::Flag(name(false))),
                    ("flag is not set", Condition::NotFlag(name(false))),
                    (
                        "variable",
                        Condition::Variable(name(true), Compare::AtLeast, 1),
                    ),
                ];
                changed |= choice(ui, "", condition, &kinds, same_kind);
            });
            changed |= match condition {
                Condition::Flag(name) | Condition::NotFlag(name) => pick_flag(ui, name, lists),
                Condition::Variable(name, compare, value) => {
                    let compares = [
                        ("equal to", Compare::Equals),
                        ("not equal to", Compare::NotEquals),
                        ("less than", Compare::Less),
                        ("at least", Compare::AtLeast),
                    ];
                    let picked = pick_variable(ui, name, lists);
                    let compared = ui.horizontal(|ui| {
                        choice(ui, "is", compare, &compares, PartialEq::eq)
                            | ui.add(drag(value)).changed()
                    });
                    picked | compared.inner
                }
            };
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

/// The names that the pickers offer for the open room, and what the checks find in
/// one event.
fn lists_and_problems(editor: &mut Editor, index: usize) -> (Lists, Vec<Problem>) {
    let registry = editor.registry.clone();
    let others = editor.others().to_vec();
    let used = names(&editor.document, &others, &registry);
    let mut problems: Vec<Problem> = problems(&editor.document, &others, &registry);
    problems.retain(|problem| problem.event == index);
    let room = &editor.document.room;
    let named = |names: Vec<&Option<String>>| -> Vec<String> {
        names.into_iter().flatten().cloned().collect()
    };
    let doors = room.doors.iter().map(|door| &door.name);
    let own = (
        file_name(&editor.document),
        room.spawns.keys().cloned().collect(),
    );
    let other_rooms = others
        .iter()
        .map(|room| (room.file.clone(), room.spawns.iter().cloned().collect()));
    let lists = Lists {
        flags: used.flags.keys().cloned().collect(),
        variables: used.variables.keys().cloned().collect(),
        unset_flags: Names::never_written(&used.flags),
        unset_variables: Names::never_written(&used.variables),
        lights: named(
            room.lights
                .iter()
                .map(|light| &light.name)
                .chain(room.objects.iter().filter(|o| o.components.contains_key("light")).map(|o| &o.name))
                .collect(),
        ),
        movers: named(
            doors
                .chain(room.movers.iter().map(|mover| &mover.name))
                .collect(),
        ),
        rooms: std::iter::once(own).chain(other_rooms).collect(),
        row_keys: Default::default(),
    };
    (lists, problems)
}

/// The part of an event in the property panel: its name and its area. True: the
/// event was changed. `None`: the document has no such event. `old_name`: set if the
/// name was changed.
pub(crate) fn event_properties(
    ui: &mut Ui,
    editor: &mut Editor,
    index: usize,
    old_name: &mut Option<Option<String>>,
) -> Option<bool> {
    let (_, problems) = lists_and_problems(editor, index);
    let event = editor.document.events.get_mut(index)?;
    let mut changed = false;

    let old = event.name.clone();
    if text(ui, "name", &mut event.name) {
        *old_name = Some(Some(old));
        changed = true;
    }
    // An event with no area is one that no player walks to: Autorun, Parallel.
    let new_area = || Area {
        center: [0.0; 2],
        size: [1.0; 2],
    };
    let label = "has an area on the floor";
    changed |= optional(ui, label, &mut event.area, new_area, |ui, area| {
        let mut changed = false;
        for (label, value, least) in [
            ("center (X, Z)", &mut area.center, -100.0),
            ("size (X, Z)", &mut area.size, 0.05),
        ] {
            row(ui, label, |ui| {
                for part in value {
                    let drag = drag(part).speed(0.01).range(least..=100.0).suffix(" m");
                    changed |= ui.add(drag).changed();
                }
            });
        }
        changed
    });
    for problem in problems.iter().filter(|problem| problem.command.is_none()) {
        ui.colored_label(color(problem), problem.text.as_str());
    }
    ui.weak("Pages and commands: the panel \"Events\".");
    Some(changed)
}

/// The event panel, under the picture: the pages with their conditions and trigger
/// at the left, the command rows in the middle, the arguments of the selected row
/// at the right. True: the event was changed. `None`: the document has no such event.
pub(crate) fn event_panel(
    ui: &mut Ui,
    world: &mut World,
    editor: &mut Editor,
    index: usize,
) -> Option<bool> {
    let registry = editor.registry.clone();
    let (mut lists, problems) = lists_and_problems(editor, index);
    let actions = [crate::DELETE, crate::DUPLICATE, crate::COPY, crate::PASTE];
    lists.row_keys = actions.map(|action| maki_ui::key_label(world, action));
    let problems: Vec<&Problem> = problems.iter().collect();
    let state = world.resource::<GameState>().clone();
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
    let room = &document.room;
    let event = document.events.get_mut(index)?;
    let mut changed = false;
    if event.pages.is_empty() {
        event.pages.push(empty_page());
        changed = true;
    }
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
            changed |= pages(ui, event, event_ui, &state, &lists, queue);
        });
    });
    let at = event_ui.page;
    // Another event or another page: the row and the typed command are of the old one.
    if std::mem::replace(&mut event_ui.shown, (index, at)) != (index, at) {
        event_ui.row = None;
        event_ui.draft = None;
    }
    let page = &mut event.pages[at];
    part(egui::Panel::right("event arguments")).show(ui, |ui| {
        scroll("arguments").show(ui, |ui| {
            let places = (&lists, &registry, &editors, room);
            changed |= command_arguments(ui, page, event_ui, places);
        });
    });
    egui::Frame::NONE.inner_margin(6).show(ui, |ui| {
        scroll("commands").show(ui, |ui| {
            let places = (&lists, &registry, room);
            changed |= command_rows(ui, page, event_ui, places, &problems, at);
        });
    });
    Some(changed)
}

/// The width of the left and of the right part of the event panel at the start.
const SIDE_WIDTH: f32 = 340.0;

/// The left part of the event panel: the pages as tabs, and the conditions and the
/// trigger of the page that shows.
fn pages(
    ui: &mut Ui,
    event: &mut maki_events::EventDef,
    event_ui: &mut EventUi,
    state: &GameState,
    lists: &Lists,
    queue: &mut Vec<EditorCommand>,
) -> bool {
    let mut changed = false;
    // The pages, as tabs. The one that the game would run now has a star.
    let count = event.pages.len();
    event_ui.page = event_ui.page.min(count - 1);
    let active = active_page(event, state);
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
    let at = event_ui.page;
    ui.horizontal(|ui| {
        let before = (event.pages.clone(), at);
        if ui.button("Add").clicked() {
            event.pages.insert(at + 1, empty_page());
            event_ui.page = at + 1;
        }
        if ui
            .button("Duplicate")
            .on_hover_text("A copy of the page, after it")
            .clicked()
        {
            event.pages.insert(at + 1, event.pages[at].clone());
            event_ui.page = at + 1;
        }
        if ui
            .add_enabled(count > 1, egui::Button::new("Delete"))
            .clicked()
        {
            event.pages.remove(at);
            event_ui.page = at.min(count - 2);
        }
        if ui
            .add_enabled(at > 0, egui::Button::new("<"))
            .on_hover_text("Earlier in the order")
            .clicked()
        {
            event.pages.swap(at, at - 1);
            event_ui.page = at - 1;
        }
        if ui
            .add_enabled(at + 1 < count, egui::Button::new(">"))
            .on_hover_text("Later: it wins over the pages before it")
            .clicked()
        {
            event.pages.swap(at, at + 1);
            event_ui.page = at + 1;
        }
        if before != (event.pages.clone(), event_ui.page) {
            event_ui.row = None;
            changed = true;
        }
    });
    let at = event_ui.page;
    let name = event.name.clone();
    let page = &mut event.pages[at];

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
        ui.weak("Autorun runs one time when the page gets active (also when the room loads), not again and again as in RPG Maker.");
    }
    ui.separator();
    if ui
        .button("Test this page")
        .on_hover_text("Plays the room and runs this page, one command for each step")
        .clicked()
    {
        queue.push(EditorCommand::Test(name, at));
    }
    changed
}

/// Red for an error of a check (the game does not load the room), else yellow.
fn color(problem: &Problem) -> Color32 {
    if problem.warning { HINT } else { ERROR }
}

fn empty_page() -> Page {
    Page {
        conditions: Vec::new(),
        trigger: Trigger::Interact,
        commands: Vec::new(),
    }
}

/// The middle part of the event panel: the command rows of a page.
fn command_rows(
    ui: &mut Ui,
    page: &mut Page,
    event_ui: &mut EventUi,
    (lists, registry, room): (&Lists, &CommandRegistry, &RoomDef),
    problems: &[&Problem],
    page_index: usize,
) -> bool {
    let list = &mut page.commands;
    let all = rows(list, registry);
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
            // A line for a reader, if the module of the command gives one. The text
            // of the room file is the hover text.
            let shown = command.and_then(|command| registry.row_text(command));
            let label = egui::RichText::new(shown.as_deref().unwrap_or(text));
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
fn command_arguments(
    ui: &mut Ui,
    page: &mut Page,
    event_ui: &mut EventUi,
    (lists, registry, editors, room): (&Lists, &CommandRegistry, &ArgumentEditors, &RoomDef),
) -> bool {
    let list = &mut page.commands;
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
