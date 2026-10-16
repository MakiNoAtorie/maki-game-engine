//! Checks of the events of a room: a name that the room does not have, a `Transfer`
//! to a place that does not exist, two events with one name.
//!
//! The room loader runs them for each room file (`maki_scene::RoomValidators`), and
//! a tool runs them on events that are not in a file yet: [`CommandRegistry::check_events`]
//! is the same code for the two. A module adds the check of its own commands with
//! [`CommandRegistry::add_check`].

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use bevy_ecs::system::Res;
use maki_core::fields::Value;
use maki_scene::{ComponentTypes, RoomCheck, RoomDef, RoomProblem, RoomValidator};

use crate::builtin::Builtin;
use crate::command::{Command, CommandRegistry, Stop};
use crate::model::{EventDef, Wire};

/// What the check of a command knows of the room that the command is in.
pub struct CheckContext<'a> {
    pub room: &'a RoomDef,
    /// The name of the room file.
    pub file: &'a str,
    /// The directory of the room file: a file name of a command starts there.
    pub directory: &'a Path,
    /// For another room file in the directory of the room, and a name: true if the
    /// file has a spawn point of that name. `None`: no such room file.
    pub spawn: &'a dyn Fn(&str, &str) -> Option<bool>,
    /// The script functions and the outputs of the component types.
    pub registry: &'a CommandRegistry,
}

impl CheckContext<'_> {
    /// What is wrong with an output of an object of the room. `None`: the object has
    /// it.
    pub fn output_problem(&self, object: &str, output: &str) -> Option<String> {
        let Some((_, found)) = self.room.object_named(object) else {
            return Some(format!("the room has no object `{object}`"));
        };
        let has = self.registry.outputs_of(found).contains(&output);
        (!has).then(|| format!("the object `{object}` has no output `{output}`"))
    }
}

impl CheckContext<'_> {
    /// True if the room file has the spawn point. `None`: no such room file. The
    /// file can be the room itself.
    pub fn has_spawn(&self, file: &str, spawn: &str) -> Option<bool> {
        if file == self.file {
            return Some(self.room.spawn(spawn).is_some());
        }
        (self.spawn)(file, spawn)
    }
}

impl CheckContext<'_> {
    /// What is wrong with a `SetField` row: an object that the room does not have, a
    /// component that the object (or its prefab) does not have, a field that the
    /// component does not have, a value that is not of the kind of the field. `None`:
    /// the row is right.
    pub fn field_problem(
        &self,
        object: &str,
        component: &str,
        field: &str,
        value: &Value,
    ) -> Option<String> {
        let Some((_, found)) = self.room.object_named(object) else {
            return Some(format!("the room has no object `{object}`"));
        };
        let types = self
            .registry
            .component_types
            .read()
            .expect("no writer panics");
        let components = found.reached_components(self.directory, &types);
        let Some(text) = components.get(component) else {
            return Some(format!(
                "the object `{object}` has no component `{component}`"
            ));
        };
        let problem = types.field_problem(component, text, field, value);
        let fixed = types.is_live(component) && !types.is_live_field(component, field);
        problem.or_else(|| {
            fixed.then(|| format!("the field `{component}.{field}` does not change in the game"))
        })
    }
}

/// Copies the types of components into the registry when they change: the check of
/// a room file has no world.
pub(crate) fn copy_component_types(types: Res<ComponentTypes>, registry: Res<CommandRegistry>) {
    *registry.component_types.write().expect("no writer panics") = types.clone();
}

/// What the check of a command found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// False: an error, and the room does not load. True: the room loads.
    pub warning: bool,
    pub text: String,
}

impl Finding {
    pub fn error(text: String) -> Self {
        Self {
            warning: false,
            text,
        }
    }

    pub fn warning(text: String) -> Self {
        Self {
            warning: true,
            text,
        }
    }
}

/// Something wrong in the events of a room.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    /// The index of the event in the list that was checked.
    pub event: usize,
    /// The index of the object of the event in the room.
    pub object: usize,
    /// `Some`: the index of the page, and the command. `None`: the event itself.
    pub command: Option<(usize, Command)>,
    pub warning: bool,
    pub text: String,
}

fn is_transfer(command: &Command) -> bool {
    matches!(command.get(), Some(Builtin::Transfer(..)))
}

impl CommandRegistry {
    /// The checks of the events of a room: two events of one object (an error: the
    /// save point and a test find an event by the id of its object; by its name in a
    /// file with no ids), and for each command the check of its module.
    pub fn check_events(&self, events: &[EventDef], context: &CheckContext) -> Vec<Problem> {
        let mut problems = Vec::new();
        for (index, event) in events.iter().enumerate() {
            let twin = events[..index]
                .iter()
                .find_map(|other| match (other.id, event.id) {
                    (Some(a), Some(b)) => (a == b).then(|| format!("the id {}", a.0)),
                    _ => (other.name == event.name).then(|| format!("the name `{}`", event.name)),
                });
            if let Some(key) = twin {
                problems.push(Problem {
                    event: index,
                    object: event.object,
                    command: None,
                    warning: false,
                    text: format!("another event has {key}"),
                });
            }
            // A wire joins an output that a component of its object has.
            for (wire_index, wire) in event.wires.iter().enumerate() {
                if let Some(text) = context.output_problem(&event.name, &wire.output) {
                    let wire = Wire::label(&event.wires, wire_index);
                    problems.push(Problem {
                        event: index,
                        object: event.object,
                        command: None,
                        warning: false,
                        text: format!("the wire `{wire}`: {text}"),
                    });
                }
            }
            // The command lists: the pages, then the wires.
            let pages = event.pages.iter().map(|page| &page.commands);
            let lists = pages.chain(event.wires.iter().map(|wire| &wire.commands));
            for (page, commands) in lists.enumerate() {
                // The commands after a `Transfer` run in another room: its names
                // are not known here.
                let mut left = false;
                self.each_command(commands, &mut |command| {
                    if left {
                        return;
                    }
                    left = is_transfer(command);
                    if let Some(finding) = self.check(command, context) {
                        problems.push(Problem {
                            event: index,
                            object: event.object,
                            command: Some((page, command.clone())),
                            warning: finding.warning,
                            text: finding.text,
                        });
                    }
                });
            }
        }
        problems
    }

    /// The check of the events for the room loader. Each problem has the line of
    /// its command, or of the name of its event, and the text of `check_events`.
    pub(crate) fn validator(&self) -> RoomValidator {
        let registry = self.clone();
        let neighbors = Neighbors::default();
        Box::new(move |check: &RoomCheck| {
            let events = match registry.parse_events(check.text) {
                Ok(events) => events,
                Err(error) => {
                    return vec![RoomProblem {
                        warning: false,
                        text: error.to_string(),
                    }];
                }
            };
            let file = check.path.file_name().unwrap_or_default();
            let directory = check.path.parent().unwrap_or(Path::new(""));
            let spawn = |file: &str, spawn: &str| neighbors.has_spawn(&directory.join(file), spawn);
            let context = CheckContext {
                room: check.room,
                file: &file.to_string_lossy(),
                directory,
                spawn: &spawn,
                registry: &registry,
            };
            let problems = registry.check_events(&events, &context);
            let with_line = |problem: Problem| {
                let stop = match &problem.command {
                    Some((_, command)) => Stop::Command(command.order),
                    None => Stop::Event(problem.event as u32 + 1),
                };
                let line = registry.line(check.text, stop).unwrap_or(0);
                let event = &events[problem.event].name;
                RoomProblem {
                    warning: problem.warning,
                    text: format!("{line}: event `{event}`: {}", problem.text),
                }
            };
            problems.into_iter().map(with_line).collect()
        })
    }
}

/// The spawn points of the room files that a `Transfer` names.
// The loader checks a room in a frame of the game. A file is read one time, and
// again only after it was written.
#[derive(Default)]
struct Neighbors(Mutex<HashMap<PathBuf, Neighbor>>);

/// When the file was written, and its spawn points.
type Neighbor = (Option<SystemTime>, BTreeSet<String>);

impl Neighbors {
    /// `None`: the file is not there, or it is not a room.
    fn has_spawn(&self, path: &Path, spawn: &str) -> Option<bool> {
        let written = std::fs::metadata(path).ok()?.modified().ok();
        let mut files = self.0.lock().expect("no holder panics");
        if files.get(path).is_none_or(|(known, _)| *known != written) {
            let text = std::fs::read_to_string(path).ok()?;
            let spawns = maki_scene::parse_room(&text).ok()?.spawn_names();
            files.insert(path.to_owned(), (written, spawns.into_iter().collect()));
        }
        Some(files[path].1.contains(spawn))
    }
}
