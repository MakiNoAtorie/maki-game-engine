//! Events as the event panel works with them: the commands of a page as rows, the
//! names that the events of all rooms use, and what the checks of the events crate
//! find in them.
//!
//! A command is a value with a hidden type (`maki_events::Command`), and the two
//! lists of an `If` are in that value. So a change in a list of an `If` makes the
//! `If` again, up to the list of the page: [`edit_list`] does that. The rows of the
//! panel are the commands in the order of the file, with the lists of an `If` under
//! it, as RPG Maker shows them:
//!
//! ```text
//! SetFlag("seen", true)
//! If Flag("open")
//!     OpenDoor("door")
//! Else
//!     Message("Locked.")
//! End
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use maki_events::{Builtin, CheckContext, Command, CommandRegistry, EventDef, Problem};

use crate::Document;

/// A list of commands in a page: for each command with lists on the way down from
/// the list of the page, its index and the index of its list (`CommandRegistry::lists`:
/// 1 is the "else" of an `If`, or the second answer of a question).
pub type ListPath = Vec<(usize, usize)>;

/// A row of the command list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Row {
    pub list: ListPath,
    /// In the list. For `Branch` and `End`: the index of their command.
    pub index: usize,
    pub kind: RowKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RowKind {
    /// A command. A command with lists is the row of its first line.
    #[default]
    Command,
    /// The line before a list of a command, with the label of the list: `Else`, or
    /// an answer of a question. The number is the index of the list.
    Branch(usize),
    /// The line after a command with lists.
    End,
}

impl Row {
    /// Where a new command goes when this row is selected: after a command, at the
    /// start of the first list of a command with lists, at the start of its list
    /// for a `Branch`, and after the command for `End`.
    pub fn insert_place(
        &self,
        commands: &[Command],
        registry: &CommandRegistry,
    ) -> (ListPath, usize) {
        let command = self.command(commands, registry);
        let has_lists = command.is_some_and(|command| !registry.lists(command).is_empty());
        let branch = |list_index: usize| {
            let mut list = self.list.clone();
            list.push((self.index, list_index));
            (list, 0)
        };
        match self.kind {
            RowKind::Command if has_lists => branch(0),
            RowKind::Branch(list) => branch(list),
            RowKind::Command | RowKind::End => (self.list.clone(), self.index + 1),
        }
    }

    /// The command of the row. For `Branch` and `End`: their command.
    pub fn command<'a>(
        &self,
        commands: &'a [Command],
        registry: &CommandRegistry,
    ) -> Option<&'a Command> {
        list(commands, &self.list, registry)?.get(self.index)
    }
}

/// The list at a path. `None`: the path does not lead to a list of a command.
pub fn list<'a>(
    commands: &'a [Command],
    path: &[(usize, usize)],
    registry: &CommandRegistry,
) -> Option<&'a [Command]> {
    let Some(((index, list_index), rest)) = path.split_first() else {
        return Some(commands);
    };
    let lists = registry.lists(commands.get(*index)?);
    list(lists.get(*list_index)?.1, rest, registry)
}

/// Changes the list at a path, and makes each command on the way again with its new
/// list (`CommandRegistry::with_lists`). `None`, with no change: the path does not
/// lead to a list of a command.
pub fn edit_list<R>(
    commands: &mut Vec<Command>,
    path: &[(usize, usize)],
    registry: &CommandRegistry,
    change: impl FnOnce(&mut Vec<Command>) -> R,
) -> Option<R> {
    let Some(((index, list_index), rest)) = path.split_first() else {
        return Some(change(commands));
    };
    let command = commands.get(*index)?;
    let lists = registry.lists(command);
    let mut lists: Vec<Vec<Command>> = lists.into_iter().map(|list| list.1.to_vec()).collect();
    let result = edit_list(lists.get_mut(*list_index)?, rest, registry, change)?;
    commands[*index] = registry.with_lists(command, lists)?;
    Some(result)
}

/// The text of a command in a list of rows or of uses: the line that its module
/// gives for a reader, else the command as a room file has it.
pub fn command_text(command: &Command, registry: &CommandRegistry) -> String {
    let text = registry.row_text(command);
    text.unwrap_or_else(|| format!("{command:?}"))
}

/// The rows of a command list, with the depth of each (the number of commands with
/// lists around it) and its text.
pub fn rows(commands: &[Command], registry: &CommandRegistry) -> Vec<(Row, usize, String)> {
    fn walk(
        commands: &[Command],
        path: &ListPath,
        registry: &CommandRegistry,
        out: &mut Vec<(Row, usize, String)>,
    ) {
        for (index, command) in commands.iter().enumerate() {
            let row = |kind| Row {
                list: path.clone(),
                index,
                kind,
            };
            let text = command_text(command, registry);
            out.push((row(RowKind::Command), path.len(), text));
            let lists = registry.lists(command);
            if lists.is_empty() {
                continue;
            }
            for (list_index, (label, list)) in lists.into_iter().enumerate() {
                // An empty label: the list starts with no line (the first of an `If`).
                if !label.is_empty() {
                    out.push((row(RowKind::Branch(list_index)), path.len(), label));
                }
                let mut inner = path.clone();
                inner.push((index, list_index));
                walk(list, &inner, registry, out);
            }
            out.push((row(RowKind::End), path.len(), "End".to_owned()));
        }
    }
    let mut out = Vec::new();
    walk(commands, &Vec::new(), registry, &mut out);
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
    /// The names that an event reads and no event of the game writes: a name with a
    /// wrong letter, or a flag that only a test sets. A hint, not an error.
    pub fn never_written(names: &BTreeMap<String, Vec<Use>>) -> Vec<String> {
        let read_only = |uses: &Vec<Use>| uses.iter().all(|place| !place.writes);
        let names = names.iter().filter(|(_, uses)| read_only(uses));
        names.map(|(name, _)| name.clone()).collect()
    }

    fn add(&mut self, room: &str, events: &[EventDef], registry: &CommandRegistry) {
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
                    place(condition.is_variable(), condition.name(), text, false);
                }
                registry.each_command(&page.commands, &mut |command| match command.get() {
                    Some(Builtin::If(conditions, ..)) => {
                        for condition in conditions {
                            let variable = condition.is_variable();
                            let text = command_text(command, registry);
                            place(variable, condition.name(), text, false);
                        }
                    }
                    Some(Builtin::SetFlag(name, _)) => {
                        place(false, name, format!("{command:?}"), true);
                    }
                    Some(Builtin::SetVariable(name, _) | Builtin::AddVariable(name, _)) => {
                        place(true, name, format!("{command:?}"), true);
                    }
                    _ => {}
                });
            }
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
            spawns: def.spawn_names().into_iter().collect(),
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
pub fn names(document: &Document, others: &[OtherRoom], registry: &CommandRegistry) -> Names {
    let mut names = Names::default();
    names.add(&file_name(document), &document.events(registry), registry);
    for room in others {
        names.add(&room.file, &room.events, registry);
    }
    names
}

/// What the checks of the room loader find in the events of the open room
/// (`CommandRegistry::check_events`): the same code and the same texts. The loader
/// reads the other room files from the disk; the editor gives what it knows of them.
pub fn problems(
    document: &Document,
    others: &[OtherRoom],
    registry: &CommandRegistry,
) -> Vec<Problem> {
    let spawn = |file: &str, spawn: &str| {
        let room = others.iter().find(|room| room.file == file)?;
        Some(room.spawns.contains(spawn))
    };
    let context = CheckContext {
        room: &document.room,
        file: &file_name(document),
        directory: document.path.parent().unwrap_or(Path::new("")),
        spawn: &spawn,
        registry,
    };
    registry.check_events(&document.events(registry), &context)
}

#[cfg(test)]
mod tests {
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
                objects: [
                    (name: "door", components: {{"door": (), "doorway": (wall: NegZ, offset: 0.0, width: 1.0, height: 2.0, depth: 1.0)}}),
                    (name: "lamp", pose: (position: (0.0, 2.0, 0.0)), components: {{"light": (radius: 0.1, color: (1.0, 1.0, 1.0), intensity: 5.0)}}),
                    (name: "start", components: {{"spawn": ()}}),
                    {},
                ])"#,
            maki_events::short_events(events)
        );
        Document::parse(Path::new(name), &text, registry).unwrap()
    }

    fn texts(commands: &[Command], registry: &CommandRegistry) -> Vec<String> {
        rows(commands, registry)
            .into_iter()
            .map(|(_, depth, text)| format!("{}{text}", "  ".repeat(depth)))
            .collect()
    }

    #[test]
    fn the_rows_of_a_page_show_the_lists_of_a_command_and_a_change_in_them_makes_it_again() {
        let registry = registry();
        let event = r#"(name: "door", pages: [(trigger: Interact, commands: [
            SetFlag("seen", true),
            If([Flag("open")], [OpenDoor("door"), If([Flag("late"), Variable("turns", Less, 3)], [], [Wait(1.0)])], [Message("Locked.")]),
            FadeIn(0.5),
        ])])"#;
        let mut document = document(&registry, "a.ron", event);
        // The event is on object 3, after the door, the lamp, and the spawn point.
        let mut event = document.event(3, &registry).unwrap().unwrap();
        let commands = &mut event.pages[0].commands;
        let texts = |commands: &[Command]| texts(commands, &registry);
        assert_eq!(
            texts(commands),
            [
                "Flag seen: set",
                "If open",
                "  Open door",
                "  If late and turns < 3",
                "  Else",
                "    Wait 1 s",
                "  End",
                "Else",
                "  Message: Locked.",
                "End",
                "Fade to the picture, 0.5 s",
            ]
        );
        let all = rows(commands, &registry);
        let row = |text: &str| all.iter().find(|row| row.2 == text).unwrap().0.clone();

        // A new command after `Wait`, in the second list of the inner `If`.
        let wait = row("Wait 1 s");
        assert_eq!(wait.list, [(1, 0), (1, 1)]);
        let (list, index) = wait.insert_place(commands, &registry);
        let light = registry
            .command(Builtin::LightOff("lamp".to_owned()))
            .unwrap();
        edit_list(commands, &list, &registry, |list| list.insert(index, light)).unwrap();
        assert_eq!(texts(commands)[6], "    Light lamp: off");
        // The first line of an `If`: into its first list. `Else`: into the second.
        let inner = row("If late and turns < 3");
        assert_eq!(
            inner.insert_place(commands, &registry),
            (vec![(1, 0), (1, 0)], 0)
        );
        let outer_else = all
            .iter()
            .find(|row| row.0.kind == RowKind::Branch(1) && row.1 == 0)
            .unwrap();
        assert_eq!(
            outer_else.0.insert_place(commands, &registry),
            (vec![(1, 1)], 0)
        );

        // The first command moves into the first list of the outer `If`: the `If`
        // is at index 0 then.
        assert!(move_command(
            commands,
            &row("Flag seen: set"),
            (vec![(1, 0)], 0),
            &registry
        ));
        assert_eq!(texts(commands)[..2], ["If open", "  Flag seen: set"]);
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
            (vec![(0, 1)], 0),
            &registry
        ));
        assert_eq!(texts(commands), before);

        // A question: one list for each answer, with the answer as the label of its
        // row. A new command goes into the list of the selected label row.
        let answer = |text: &str| (text.to_owned(), Vec::new());
        let question = Builtin::Choice("Open it?".to_owned(), vec![answer("Yes"), answer("No")]);
        commands.push(registry.command(question).unwrap());
        let all = rows(commands, &registry);
        let no = all.iter().find(|row| row.2 == "Answer: No").unwrap();
        assert_eq!(no.0.kind, RowKind::Branch(1));
        let (list, index) = no.0.insert_place(commands, &registry);
        assert_eq!((list.clone(), index), (vec![(2, 1)], 0));
        let message = registry.command(Builtin::Message("Then not.".to_owned()));
        edit_list(commands, &list, &registry, |list| {
            list.insert(index, message.unwrap())
        })
        .unwrap();
        let last = texts(commands);
        assert_eq!(
            last[last.len() - 5..],
            [
                "Question: Open it?",
                "Answer: Yes",
                "Answer: No",
                "  Message: Then not.",
                "End",
            ]
        );
        // What was changed is written and read as the same commands.
        document.set_event(3, &event);
        let text = document.text();
        assert!(
            text.contains(r#"Choice("Open it?", [("Yes", []), ("No", [Message("Then not.")])])"#)
        );
        let events = registry.parse_events(&text).unwrap();
        assert_eq!(events, document.events(&registry));
        assert_eq!(events[0].pages, event.pages);
    }

    #[test]
    fn the_names_of_all_rooms_are_found_with_their_places_and_wrong_names_are_problems() {
        let registry = registry();
        let directory = std::env::temp_dir().join(format!("maki-names-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let other = r#"(width: 4, depth: 4, height: 3, objects: [(name: "west", components: {"spawn": ()}),
            (name: "switch", components: {"event": (pages: [(trigger: Interact, commands: [SetFlag("power_off", true), AddVariable("presses", 1)])])}),
        ])"#;
        std::fs::write(directory.join("b.ron"), other).unwrap();
        std::fs::write(directory.join("notes.ron"), "not a room").unwrap();
        let events = r#"
            (id: 5, name: "door", pages: [
                (trigger: Interact, commands: [If([Flag("power_off"), Variable("tries", Less, 3)], [OpenDoor("door")], [LightOff("lamp")])]),
                (conditions: [Flag("power_off"), Variable("presses", AtLeast, 2)], trigger: Autorun,
                    commands: [Transfer("b.ron", "west")]),
            ]),
            (id: 5, name: "door", pages: [
                (trigger: Touch, commands: [LightOn("lamb"), If([Flag("x")], [CloseDoor("gate")], []), Transfer("b.ron", "east")]),
                (trigger: Touch, commands: [Transfer("c.ron", "west")]),
                (trigger: Touch, commands: [Transfer("a.ron", "start")]),
            ]),"#;
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

        let names = names(&document, &others, &registry);
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
        // A name that an event reads and no event writes.
        assert_eq!(Names::never_written(&names.flags), ["x"]);
        assert_eq!(Names::never_written(&names.variables), ["tries"]);

        // The checks are those of the room loader: the editor gives the other rooms.
        let problems = problems(&document, &others, &registry);
        let found: Vec<_> = problems
            .iter()
            .map(|problem| (problem.event, problem.warning, problem.text.as_str()))
            .collect();
        assert_eq!(
            found,
            [
                (1, false, "another event has the id 5"),
                (1, false, "the room has no light `lamb`"),
                (1, false, "the room has no door or mover `gate`"),
                (1, true, "`b.ron` has no spawn point `east`"),
                (1, true, "no room file `c.ron`"),
            ]
        );
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
