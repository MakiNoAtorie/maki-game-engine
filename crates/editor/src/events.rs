//! Events as the event panel works with them: the commands of a page as rows, the
//! names that the events of all rooms use, and the checks of the editor.
//!
//! A command is a value with a hidden type (`maki_events::Command`), and the two
//! lists of an `If` are in that value. So a change in a list of an `If` makes the
//! `If` again, up to the list of the page: [`edit_list`] does that. The rows of the
//! panel are the commands in the order of the file, with the lists of an `If` under
//! it, as RPG Maker shows them:
//!
//! ```text
//! SetFlag("seen", true)
//! If "open"
//!     OpenDoor("door")
//! Else
//!     Message("Locked.")
//! End
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use maki_events::{Builtin, Command, CommandRegistry, Condition, EventDef};

use crate::Document;

/// A list of commands in a page: for each `If` on the way down from the list of the
/// page, its index and its branch (true: the second list, "else").
pub type ListPath = Vec<(usize, bool)>;

/// A row of the command list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Row {
    pub list: ListPath,
    /// In the list. For `Else` and `End`: the index of their `If`.
    pub index: usize,
    pub kind: RowKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RowKind {
    /// A command. An `If` is the row of its first line.
    #[default]
    Command,
    /// The line between the two lists of an `If`.
    Else,
    /// The line after an `If`.
    End,
}

impl Row {
    /// Where a new command goes when this row is selected: after a command, at the
    /// start of the first list of an `If`, at the start of its second list for
    /// `Else`, and after the `If` for `End`.
    pub fn insert_place(&self, commands: &[Command]) -> (ListPath, usize) {
        let is_if = matches!(
            self.command(commands).and_then(|c| c.get()),
            Some(Builtin::If(..))
        );
        let branch = |second: bool| {
            let mut list = self.list.clone();
            list.push((self.index, second));
            (list, 0)
        };
        match self.kind {
            RowKind::Command if is_if => branch(false),
            RowKind::Else => branch(true),
            RowKind::Command | RowKind::End => (self.list.clone(), self.index + 1),
        }
    }

    /// The command of the row. For `Else` and `End`: their `If`.
    pub fn command<'a>(&self, commands: &'a [Command]) -> Option<&'a Command> {
        list(commands, &self.list)?.get(self.index)
    }
}

/// The list at a path. `None`: the path does not lead to an `If`.
pub fn list<'a>(commands: &'a [Command], path: &[(usize, bool)]) -> Option<&'a [Command]> {
    let Some(((index, second), rest)) = path.split_first() else {
        return Some(commands);
    };
    match commands.get(*index)?.get::<Builtin>()? {
        Builtin::If(_, yes, no) => list(if *second { no } else { yes }, rest),
        _ => None,
    }
}

/// Changes the list at a path, and makes each `If` on the way again with its new
/// list. `None`, with no change: the path does not lead to an `If`.
pub fn edit_list<R>(
    commands: &mut Vec<Command>,
    path: &[(usize, bool)],
    registry: &CommandRegistry,
    change: impl FnOnce(&mut Vec<Command>) -> R,
) -> Option<R> {
    let Some(((index, second), rest)) = path.split_first() else {
        return Some(change(commands));
    };
    let Builtin::If(flag, yes, no) = commands.get(*index)?.get::<Builtin>()? else {
        return None;
    };
    let (flag, mut yes, mut no) = (flag.clone(), yes.clone(), no.clone());
    let result = edit_list(
        if *second { &mut no } else { &mut yes },
        rest,
        registry,
        change,
    )?;
    commands[*index] = registry.command(Builtin::If(flag, yes, no))?;
    Some(result)
}

/// The rows of a command list, with the depth of each (the number of `If` around it)
/// and its text.
pub fn rows(commands: &[Command]) -> Vec<(Row, usize, String)> {
    fn walk(commands: &[Command], path: &ListPath, out: &mut Vec<(Row, usize, String)>) {
        for (index, command) in commands.iter().enumerate() {
            let row = |kind| Row {
                list: path.clone(),
                index,
                kind,
            };
            let Some(Builtin::If(flag, yes, no)) = command.get::<Builtin>() else {
                out.push((row(RowKind::Command), path.len(), format!("{command:?}")));
                continue;
            };
            out.push((row(RowKind::Command), path.len(), format!("If {flag:?}")));
            for (second, branch) in [(false, yes), (true, no)] {
                if second {
                    out.push((row(RowKind::Else), path.len(), "Else".to_owned()));
                }
                let mut inner = path.clone();
                inner.push((index, second));
                walk(branch, &inner, out);
            }
            out.push((row(RowKind::End), path.len(), "End".to_owned()));
        }
    }
    let mut out = Vec::new();
    walk(commands, &Vec::new(), &mut out);
    out
}

/// Moves the command of a row to another place. False, with no change: the place is
/// in the command itself (an `If` cannot go into one of its own lists).
pub fn move_command(
    commands: &mut Vec<Command>,
    from: &Row,
    to: (ListPath, usize),
    registry: &CommandRegistry,
) -> bool {
    let (mut to_list, mut to_index) = to;
    let in_itself = to_list.len() > from.list.len()
        && to_list[..from.list.len()] == from.list[..]
        && to_list[from.list.len()].0 == from.index;
    if in_itself {
        return false;
    }
    let Some(command) = edit_list(commands, &from.list, registry, |list| {
        list.remove(from.index)
    }) else {
        return false;
    };
    // The removal moved each place behind it in the same list one step up: the
    // index in that list, or the index of an `If` on the way to a deeper list.
    if to_list == from.list && to_index > from.index {
        to_index -= 1;
    } else if to_list.len() > from.list.len()
        && to_list[..from.list.len()] == from.list[..]
        && to_list[from.list.len()].0 > from.index
    {
        to_list[from.list.len()].0 -= 1;
    }
    let done = edit_list(commands, &to_list, registry, |list| {
        list.insert(to_index.min(list.len()), command)
    });
    done.is_some()
}

/// One place where a flag or a variable is read or written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Use {
    /// The name of the room file.
    pub room: String,
    pub event: String,
    /// From 1, as the panel shows the pages.
    pub page: usize,
    /// The condition or the command.
    pub text: String,
    /// True: the place sets the value. False: it reads it.
    pub writes: bool,
}

/// The flags and the variables of a game: each name, with each place that uses it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Names {
    pub flags: BTreeMap<String, Vec<Use>>,
    pub variables: BTreeMap<String, Vec<Use>>,
}

impl Names {
    fn add(&mut self, room: &str, events: &[EventDef]) {
        for event in events {
            for (page_index, page) in event.pages.iter().enumerate() {
                let mut place = |variable: bool, name: &str, text: String, writes: bool| {
                    let names = if variable {
                        &mut self.variables
                    } else {
                        &mut self.flags
                    };
                    names.entry(name.to_owned()).or_default().push(Use {
                        room: room.to_owned(),
                        event: event.name.clone(),
                        page: page_index + 1,
                        text,
                        writes,
                    });
                };
                for condition in &page.conditions {
                    let text = format!("if {condition:?}");
                    match condition {
                        Condition::Flag(name) | Condition::NotFlag(name) => {
                            place(false, name, text, false)
                        }
                        Condition::VariableAtLeast(name, _)
                        | Condition::VariableEquals(name, _) => {
                            place(true, name, text, false);
                        }
                    }
                }
                walk(&page.commands, &mut |command| match command {
                    Builtin::SetFlag(name, _) => place(false, name, format!("{command:?}"), true),
                    Builtin::If(name, ..) => place(false, name, format!("If {name:?}"), false),
                    Builtin::SetVariable(name, _) | Builtin::AddVariable(name, _) => {
                        place(true, name, format!("{command:?}"), true);
                    }
                    _ => {}
                });
            }
        }
    }
}

/// Calls `visit` for each built-in command of a list, also in the lists of an `If`.
fn walk(commands: &[Command], visit: &mut dyn FnMut(&Builtin)) {
    for command in commands {
        let Some(builtin) = command.get::<Builtin>() else {
            continue;
        };
        visit(builtin);
        if let Builtin::If(_, yes, no) = builtin {
            walk(yes, visit);
            walk(no, visit);
        }
    }
}

/// What the editor knows of another room file of the game.
#[derive(Debug, Clone, PartialEq)]
pub struct OtherRoom {
    /// The file name: what a `Transfer` names.
    pub file: String,
    pub spawns: BTreeSet<String>,
    pub events: Vec<EventDef>,
}

/// The other room files in the directory of a room file. A file that does not read
/// as a room is left out.
pub fn other_rooms(room: &Path, registry: &CommandRegistry) -> Vec<OtherRoom> {
    let directory = room.parent().unwrap_or(Path::new(""));
    let entries = std::fs::read_dir(directory).into_iter().flatten().flatten();
    let mut rooms = Vec::new();
    for path in entries.map(|entry| entry.path()) {
        let is_room = path.extension().is_some_and(|extension| extension == "ron");
        if !is_room || path.file_name() == room.file_name() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let (Ok(def), Ok(events)) = (maki_scene::parse_room(&text), registry.parse_events(&text))
        else {
            continue;
        };
        rooms.push(OtherRoom {
            file: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            spawns: def.spawns.into_keys().collect(),
            events,
        });
    }
    rooms.sort_by(|a, b| a.file.cmp(&b.file));
    rooms
}

/// The name of the room file of a document.
pub fn file_name(document: &Document) -> String {
    let path = &document.path;
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

/// The names that the open room and the other rooms use.
pub fn names(document: &Document, others: &[OtherRoom]) -> Names {
    let mut names = Names::default();
    names.add(&file_name(document), &document.events);
    for room in others {
        names.add(&room.file, &room.events);
    }
    names
}

/// Something wrong in the events of the open room, that the room loader does not
/// find: the room loads, and the event does nothing or gives an error when it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The index of the event.
    pub event: usize,
    /// `Some`: the page, and the text of the command row. `None`: the event itself.
    pub row: Option<(usize, String)>,
    pub text: String,
}

/// The checks of the editor: two events with one name, the name of a light, a door,
/// or a mover that the room does not have, and a `Transfer` to a room file or a
/// spawn point that does not exist.
pub fn problems(document: &Document, others: &[OtherRoom]) -> Vec<Problem> {
    let room = &document.room;
    let named = |names: Vec<&Option<String>>, name: &str| {
        names
            .into_iter()
            .any(|other| other.as_deref() == Some(name))
    };
    let light = |name: &str| named(room.lights.iter().map(|light| &light.name).collect(), name);
    let mover = |name: &str| {
        let doors = room.doors.iter().map(|door| &door.name);
        named(
            doors
                .chain(room.movers.iter().map(|mover| &mover.name))
                .collect(),
            name,
        )
    };
    let own = file_name(document);
    let mut problems = Vec::new();
    for (index, event) in document.events.iter().enumerate() {
        let twice = document.events[..index]
            .iter()
            .any(|other| other.name == event.name);
        if twice {
            let text = format!("another event has the name `{}`", event.name);
            problems.push(Problem {
                event: index,
                row: None,
                text,
            });
        }
        for (page_index, page) in event.pages.iter().enumerate() {
            walk(&page.commands, &mut |command| {
                let text = match command {
                    Builtin::LightOn(name)
                    | Builtin::LightOff(name)
                    | Builtin::LightIntensity(name, _)
                        if !light(name) =>
                    {
                        format!("the room has no light `{name}`")
                    }
                    Builtin::OpenDoor(name) | Builtin::CloseDoor(name) if !mover(name) => {
                        format!("the room has no door or mover `{name}`")
                    }
                    Builtin::Transfer(file, spawn) => {
                        let spawns = match others.iter().find(|room| room.file == *file) {
                            Some(room) => Some(room.spawns.contains(spawn)),
                            None if *file == own => Some(room.spawns.contains_key(spawn)),
                            None => None,
                        };
                        match spawns {
                            Some(true) => return,
                            Some(false) => format!("`{file}` has no spawn point `{spawn}`"),
                            None => format!("no room file `{file}`"),
                        }
                    }
                    _ => return,
                };
                let row = match command {
                    Builtin::If(name, ..) => format!("If {name:?}"),
                    command => format!("{command:?}"),
                };
                problems.push(Problem {
                    event: index,
                    row: Some((page_index, row)),
                    text,
                });
            });
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use maki_scene::RoomValidators;

    use super::*;

    fn registry() -> CommandRegistry {
        let mut app = maki_core::App::new(maki_core::ConfigFile::default()).unwrap();
        app.add_module(maki_platform::PlatformModule)
            .add_module(maki_scene::SceneModule)
            .add_module(maki_events::EventsModule);
        app.world().resource::<CommandRegistry>().clone()
    }

    fn document(registry: &CommandRegistry, name: &str, events: &str) -> Document {
        let text = format!(
            r#"(width: 8.0, depth: 8.0, height: 3.0,
                doorways: [(wall: NegZ, offset: 0.0, width: 1.0, height: 2.0, depth: 1.0)],
                doors: [(name: "door", doorway: 0)],
                lights: [(name: "lamp", position: (0.0, 2.0, 0.0), radius: 0.1, color: (1.0, 1.0, 1.0), intensity: 5.0)],
                spawns: {{"start": (0.0, 0.0, 0.0)}},
                events: [{events}])"#
        );
        Document::parse(Path::new(name), &text, registry, &RoomValidators::default()).unwrap()
    }

    fn texts(commands: &[Command]) -> Vec<String> {
        rows(commands)
            .into_iter()
            .map(|(_, depth, text)| format!("{}{text}", "  ".repeat(depth)))
            .collect()
    }

    #[test]
    fn the_rows_of_a_page_show_the_lists_of_an_if_and_a_change_in_them_makes_the_if_again() {
        let registry = registry();
        let event = r#"(name: "door", pages: [(trigger: Interact, commands: [
            SetFlag("seen", true),
            If("open", [OpenDoor("door"), If("late", [], [Wait(1.0)])], [Message("Locked.")]),
            FadeIn(0.5),
        ])])"#;
        let mut document = document(&registry, "a.ron", event);
        let commands = &mut document.events[0].pages[0].commands;
        assert_eq!(
            texts(commands),
            [
                r#"SetFlag("seen", true)"#,
                r#"If "open""#,
                r#"  OpenDoor("door")"#,
                r#"  If "late""#,
                "  Else",
                "    Wait(1.0)",
                "  End",
                "Else",
                r#"  Message("Locked.")"#,
                "End",
                "FadeIn(0.5)",
            ]
        );
        let all = rows(commands);
        let row = |text: &str| all.iter().find(|row| row.2 == text).unwrap().0.clone();

        // A new command after `Wait`, in the second list of the inner `If`.
        let wait = row("Wait(1.0)");
        assert_eq!(wait.list, [(1, false), (1, true)]);
        let (list, index) = wait.insert_place(commands);
        let light = registry
            .command(Builtin::LightOff("lamp".to_owned()))
            .unwrap();
        edit_list(commands, &list, &registry, |list| list.insert(index, light)).unwrap();
        assert_eq!(texts(commands)[6], r#"    LightOff("lamp")"#);
        // The first line of an `If`: into its first list. `Else`: into the second.
        let inner = row(r#"If "late""#);
        assert_eq!(
            inner.insert_place(commands),
            (vec![(1, false), (1, false)], 0)
        );
        let outer_else = all
            .iter()
            .find(|row| row.0.kind == RowKind::Else && row.1 == 0)
            .unwrap();
        assert_eq!(outer_else.0.insert_place(commands), (vec![(1, true)], 0));

        // The first command moves into the first list of the outer `If`: the `If`
        // is at index 0 then.
        assert!(move_command(
            commands,
            &row(r#"SetFlag("seen", true)"#),
            (vec![(1, false)], 0),
            &registry
        ));
        assert_eq!(
            texts(commands)[..2],
            [r#"If "open""#, r#"  SetFlag("seen", true)"#]
        );
        // An `If` does not go into itself.
        let outer = Row {
            list: Vec::new(),
            index: 0,
            kind: RowKind::Command,
        };
        let before = texts(commands);
        assert!(!move_command(
            commands,
            &outer,
            (vec![(0, true)], 0),
            &registry
        ));
        assert_eq!(texts(commands), before);
        // What was changed is written and read as the same commands.
        let text = document.text();
        assert_eq!(registry.parse_events(&text).unwrap(), document.events);
    }

    #[test]
    fn the_names_of_all_rooms_are_found_with_their_places_and_wrong_names_are_problems() {
        let registry = registry();
        let directory = std::env::temp_dir().join(format!("maki-names-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let other = r#"(width: 4, depth: 4, height: 3, spawns: {"west": (0, 0, 0)}, events: [
            (name: "switch", pages: [(trigger: Interact, commands: [SetFlag("power_off", true), AddVariable("presses", 1)])]),
        ])"#;
        std::fs::write(directory.join("b.ron"), other).unwrap();
        std::fs::write(directory.join("notes.ron"), "not a room").unwrap();
        let events = r#"
            (name: "door", pages: [
                (trigger: Interact, commands: [If("power_off", [OpenDoor("door")], [LightOff("lamp")])]),
                (conditions: [Flag("power_off"), VariableAtLeast("presses", 2)], trigger: Autorun,
                    commands: [Transfer("b.ron", "west")]),
            ]),
            (name: "door", pages: [(trigger: Touch, commands: [
                LightOn("lamb"), If("x", [CloseDoor("gate")], []),
                Transfer("b.ron", "east"), Transfer("c.ron", "west"), Transfer("a.ron", "start"),
            ])]),"#;
        let mut document = document(&registry, "a.ron", events);
        document.path = directory.join("a.ron");
        let others = other_rooms(&document.path, &registry);
        assert_eq!(
            others
                .iter()
                .map(|room| room.file.as_str())
                .collect::<Vec<_>>(),
            ["b.ron"]
        );

        let names = names(&document, &others);
        assert_eq!(names.flags.keys().collect::<Vec<_>>(), ["power_off", "x"]);
        let uses: Vec<_> = names.flags["power_off"]
            .iter()
            .map(|u| (u.room.as_str(), u.page, u.writes))
            .collect();
        assert_eq!(
            uses,
            [("a.ron", 1, false), ("a.ron", 2, false), ("b.ron", 1, true)]
        );
        assert_eq!(names.variables["presses"].len(), 2);

        let problems = problems(&document, &others);
        let found: Vec<_> = problems
            .iter()
            .map(|problem| (problem.event, problem.text.as_str()))
            .collect();
        assert_eq!(
            found,
            [
                (1, "another event has the name `door`"),
                (1, "the room has no light `lamb`"),
                (1, "the room has no door or mover `gate`"),
                (1, "`b.ron` has no spawn point `east`"),
                (1, "no room file `c.ron`"),
            ]
        );
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
