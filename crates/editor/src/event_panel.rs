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
use maki_scene::ron_options;
use maki_ui::egui::{self, Color32, Ui};

use crate::events::{
    ListPath, Names, Row, RowKind, edit_list, file_name, move_command, names, problems, rows,
};
use crate::widgets::{choice, drag, number, optional, same_kind, text};
use crate::{Command as EditorCommand, Editor};

pub(crate) const ERROR: Color32 = Color32::from_rgb(255, 110, 90);
/// A warning of a check, and the mark of a name that no event writes.
pub(crate) const HINT: Color32 = Color32::from_rgb(235, 190, 80);
/// The mark of a flag or a variable that an event reads and no event writes.
pub(crate) const NEVER_WRITTEN: &str = "no event sets it";

/// What the event panel keeps between frames.
#[derive(Default)]
pub struct EventUi {
    /// The page that the panel shows.
    pub page: usize,
    /// The selected row of the command list.
    pub row: Option<Row>,
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
type ArgumentEditor =
    Arc<dyn Fn(&mut Ui, &Command, &CommandRegistry) -> Option<Option<Command>> + Send + Sync>;

/// The argument editors that modules gave for their commands.
#[derive(Resource, Default, Clone)]
pub struct ArgumentEditors(pub(crate) Vec<ArgumentEditor>);

/// Gives the event panel widgets for the arguments of the commands of the enum `T`
/// (a command enum of the game). `edit` draws them for one value and returns true if
/// it changed the value. Without this, such a command has a text field.
pub fn add_argument_editor<T: CommandSet>(
    app: &mut App,
    edit: impl Fn(&mut Ui, &mut T) -> bool + Send + Sync + 'static,
) {
    app.init_resource::<ArgumentEditors>();
    let editor: ArgumentEditor = Arc::new(move |ui, command, registry| {
        let mut value = command.get::<T>()?.clone();
        let changed = edit(ui, &mut value);
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
}

/// What a click in a list of names took.
enum Picked {
    Name(String),
    /// The entry "New...".
    New,
}

/// A button that opens a list of names, with a search field above them. `marked`:
/// the names with the mark [`NEVER_WRITTEN`]. `new`: the list has the entry "New...".
fn name_list(
    ui: &mut Ui,
    id: egui::Id,
    button: egui::WidgetText,
    names: &[String],
    marked: &[String],
    new: bool,
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
                if ui.selectable_label(false, label).clicked() {
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
fn picker(
    ui: &mut Ui,
    salt: &str,
    value: &mut String,
    known: &[String],
    marked: &[String],
    new: bool,
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
    match name_list(ui, id, shown.into(), known, marked, new) {
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
    let whole = |ui: &mut Ui, value: &mut i32| {
        ui.horizontal(|ui| {
            ui.label("value");
            ui.add(drag(value)).changed()
        })
        .inner
    };
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
    }
}

/// A new command for a name of the "Add" menu, with arguments that exist where the
/// lists have some. `None`: not a built-in command, and not one with no argument.
fn new_command(name: &str, lists: &Lists, registry: &CommandRegistry) -> Option<Command> {
    let first =
        |names: &[String], or: &str| names.first().cloned().unwrap_or_else(|| or.to_owned());
    let (room, spawns) = lists.rooms.first().cloned().unwrap_or_default();
    let builtin = match name {
        "SetFlag" => Builtin::SetFlag(first(&lists.flags, "flag"), true),
        "SetVariable" => Builtin::SetVariable(first(&lists.variables, "variable"), 0),
        "AddVariable" => Builtin::AddVariable(first(&lists.variables, "variable"), 1),
        "Wait" => Builtin::Wait(1.0),
        "If" => {
            let flag = Condition::Flag(first(&lists.flags, "flag"));
            Builtin::If(vec![flag], Vec::new(), Vec::new())
        }
        "OpenDoor" => Builtin::OpenDoor(first(&lists.movers, "")),
        "CloseDoor" => Builtin::CloseDoor(first(&lists.movers, "")),
        "LightOn" => Builtin::LightOn(first(&lists.lights, "")),
        "LightOff" => Builtin::LightOff(first(&lists.lights, "")),
        "LightIntensity" => Builtin::LightIntensity(first(&lists.lights, ""), 1.0),
        "Message" => Builtin::Message(String::new()),
        "FadeOut" => Builtin::FadeOut(0.4),
        "FadeIn" => Builtin::FadeIn(0.4),
        "Transfer" => Builtin::Transfer(room, first(&spawns, "")),
        // A command of another module with no argument is its name.
        other => return registry.parse_command(other).ok(),
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
    let used = names(&editor.document, &others);
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
        lights: named(room.lights.iter().map(|light| &light.name).collect()),
        movers: named(
            doors
                .chain(room.movers.iter().map(|mover| &mover.name))
                .collect(),
        ),
        rooms: std::iter::once(own).chain(other_rooms).collect(),
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
            ui.horizontal(|ui| {
                ui.label(label);
                for part in value {
                    changed |= ui
                        .add(drag(part).speed(0.01).range(least..=100.0))
                        .changed();
                }
            });
        }
        changed
    });
    for problem in problems.iter().filter(|problem| problem.command.is_none()) {
        ui.colored_label(color(problem), problem.text.as_str());
    }
    ui.weak("Pages and commands: the panel under the picture.");
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
    let (lists, problems) = lists_and_problems(editor, index);
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
            changed |= command_arguments(ui, page, event_ui, &lists, &registry, &editors);
        });
    });
    egui::Frame::NONE.inner_margin(6).show(ui, |ui| {
        scroll("commands").show(ui, |ui| {
            changed |= command_rows(ui, page, event_ui, &lists, &registry, &problems, at);
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
            .button("Copy")
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
    lists: &Lists,
    registry: &CommandRegistry,
    problems: &[&Problem],
    page_index: usize,
) -> bool {
    let list = &mut page.commands;
    let all = rows(list);
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
    let place = selected
        .as_ref()
        .map_or((Vec::new(), list.len()), |row| row.insert_place(list));
    let mut changed = false;
    let mut insert: Vec<Command> = Vec::new();

    ui.horizontal(|ui| {
        ui.label("Commands");
        let names: Vec<String> = registry.names().into_iter().map(str::to_owned).collect();
        let add = ui.make_persistent_id("add command");
        if let Some(Picked::Name(name)) = name_list(ui, add, "Add".into(), &names, &[], false) {
            match new_command(&name, lists, registry) {
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
        if ui.add_enabled(some, egui::Button::new("Delete")).clicked()
            && let Some(row) = selected_command
        {
            edit_list(list, &row.list, registry, |list| list.remove(row.index));
            event_ui.row = None;
            changed = true;
        }
        if ui.add_enabled(some, egui::Button::new("Copy")).clicked()
            && let Some(command) = selected_command.and_then(|row| row.command(list))
        {
            event_ui.clipboard = vec![command.clone()];
        }
        if ui
            .add_enabled(!event_ui.clipboard.is_empty(), egui::Button::new("Paste"))
            .clicked()
        {
            insert.clone_from(&event_ui.clipboard);
        }
    });

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
                .and_then(|row| row.command(list));
            let bad = problems.iter().find(|problem| {
                let of_problem = problem.command.as_ref();
                command.is_some_and(|command| of_problem == Some(&(page_index, command.clone())))
            });
            let label = egui::RichText::new(text).monospace();
            let label = match bad {
                Some(problem) => label.color(color(problem)),
                None => label,
            };
            let response = ui.selectable_label(selected.as_ref() == Some(row), label);
            let response = response.interact(egui::Sense::drag());
            if response.clicked() {
                event_ui.row = Some(row.clone());
                event_ui.draft = None;
            }
            if response.drag_started() && row.kind == RowKind::Command {
                event_ui.dragged = Some(row.clone());
            }
            let over = response.contains_pointer()
                && event_ui.dragged.as_ref().is_some_and(|from| from != row);
            if over {
                ui.weak("< here");
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
            _ => to.insert_place(list),
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
    lists: &Lists,
    registry: &CommandRegistry,
    editors: &ArgumentEditors,
) -> bool {
    let list = &mut page.commands;
    let mut changed = false;
    ui.label("Arguments");
    // The arguments of the selected command.
    if let Some(row) = event_ui
        .row
        .clone()
        .filter(|row| row.kind == RowKind::Command)
        && let Some(command) = row.command(list).cloned()
    {
        let new = if let Some(mut builtin) = command.get::<Builtin>().cloned() {
            arguments(ui, &mut builtin, lists)
                .then(|| registry.command(builtin))
                .flatten()
        } else if let Some(new) = editors
            .0
            .iter()
            .find_map(|edit| edit(ui, &command, registry))
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
