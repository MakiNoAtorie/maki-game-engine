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

use maki_scene::{RoomCheck, RoomDef, RoomProblem, RoomValidator};

use crate::builtin::Builtin;
use crate::command::{Command, CommandRegistry, Stop};
use crate::model::EventDef;

/// What the check of a command knows of the room that the command is in.
pub struct CheckContext<'a> {
    pub room: &'a RoomDef,
    /// The name of the room file.
    pub file: &'a str,
    /// For another room file in the directory of the room, and a name: true if the
    /// file has a spawn point of that name. `None`: no such room file.
    pub spawn: &'a dyn Fn(&str, &str) -> Option<bool>,
}

impl CheckContext<'_> {
    /// True if the room file has the spawn point. `None`: no such room file. The
    /// file can be the room itself.
    pub fn has_spawn(&self, file: &str, spawn: &str) -> Option<bool> {
        if file == self.file {
            return Some(self.room.spawns.contains_key(spawn));
        }
        (self.spawn)(file, spawn)
    }
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
    /// The index of the event.
    pub event: usize,
    /// `Some`: the index of the page, and the command. `None`: the event itself.
    pub command: Option<(usize, Command)>,
    pub warning: bool,
    pub text: String,
}

fn is_transfer(command: &Command) -> bool {
    matches!(command.get(), Some(Builtin::Transfer(..)))
}

impl CommandRegistry {
    /// The checks of the events of a room: two events with one name (an error: a
    /// `Parallel` event is found by its name), and for each command the check of its
    /// module.
    pub fn check_events(&self, events: &[EventDef], context: &CheckContext) -> Vec<Problem> {
        let mut problems = Vec::new();
        for (index, event) in events.iter().enumerate() {
            if events[..index].iter().any(|other| other.name == event.name) {
                problems.push(Problem {
                    event: index,
                    command: None,
                    warning: false,
                    text: format!("another event has the name `{}`", event.name),
                });
            }
            for (page, list) in event.pages.iter().enumerate() {
                // The commands after a `Transfer` run in another room: its names
                // are not known here.
                let mut left = false;
                self.each_command(&list.commands, &mut |command| {
                    if left {
                        return;
                    }
                    left = is_transfer(command);
                    if let Some(finding) = self.check(command, context) {
                        problems.push(Problem {
                            event: index,
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
                spawn: &spawn,
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
            let spawns = maki_scene::parse_room(&text).ok()?.spawns;
            files.insert(path.to_owned(), (written, spawns.into_keys().collect()));
        }
        Some(files[path].1.contains(spawn))
    }
}
