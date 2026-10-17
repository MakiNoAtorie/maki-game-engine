//! Checks of the events of a room: a name that the room does not have, a `Transfer`
//! to a place that does not exist, two events with one name, a text key that the
//! fallback language does not have. The room loader also checks the text tables.
//!
//! The room loader runs them for each room file (`maki_scene::RoomValidators`), and
//! a tool runs them on events that are not in a file yet: [`CommandRegistry::check_events`]
//! is the same code for the two. A module adds the check of its own commands with
//! [`CommandRegistry::add_check`].

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use bevy_ecs::system::Res;
use maki_assets::read_data;
use maki_core::fields::Value;
use maki_scene::{ComponentTypes, ObjectId, RoomCheck, RoomDef, RoomProblem, RoomValidator};

use crate::builtin::{self, Builtin};
use crate::command::{Command, CommandRegistry, Stop};
use crate::flow::Flow;
use crate::model::{Condition, EventDef, Wire};
use crate::state::condition_problem;
use crate::text::{TextSettings, TextTable};

/// What the check of a command knows of the room that the command is in.
pub struct CheckContext<'a> {
    pub room: &'a RoomDef,
    /// The name of the room file.
    pub file: &'a str,
    /// The directory of the room file: a file name of a command starts there.
    pub directory: &'a Path,
    /// For another room file in the directory of the room, and a spawn point: true if
    /// the file has it. `None`: no such room file.
    pub spawn: &'a dyn Fn(&str, &ObjectId) -> Option<bool>,
    /// The script functions and the outputs of the component types.
    pub registry: &'a CommandRegistry,
}

impl CheckContext<'_> {
    /// What is wrong with an output of an object of the room. `None`: the object has
    /// it.
    pub fn output_problem(&self, object: &ObjectId, output: &str) -> Option<String> {
        let Some((_, found)) = self.room.find(object) else {
            return Some(format!("the room has no object `{object}`"));
        };
        let has = self.registry.outputs_of(found).contains(&output);
        let object = label(found, object);
        (!has).then(|| format!("the object `{object}` has no output `{output}`"))
    }
}

/// The name of an object for a problem, else its id.
fn label(object: &maki_scene::ObjectDef, id: &ObjectId) -> String {
    object.name.clone().unwrap_or_else(|| id.to_string())
}

impl CheckContext<'_> {
    /// True if the object, or the prefab of an instance, has one of the components.
    pub fn reaches(&self, object: &ObjectId, components: &[&str]) -> bool {
        let Some((_, found)) = self.room.find(object) else {
            return false;
        };
        let types = self
            .registry
            .component_types
            .read()
            .expect("no writer panics");
        let reached = found.reached_components(self.directory, &types);
        components
            .iter()
            .any(|component| reached.contains_key(*component))
    }

    /// True if the room file has the spawn point. `None`: no such room file. The
    /// file can be the room itself.
    pub fn has_spawn(&self, file: &str, spawn: &ObjectId) -> Option<bool> {
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
        object: &ObjectId,
        component: &str,
        field: &str,
        value: &Value,
    ) -> Option<String> {
        let Some((_, found)) = self.room.find(object) else {
            return Some(format!("the room has no object `{object}`"));
        };
        let types = self
            .registry
            .component_types
            .read()
            .expect("no writer panics");
        let components = found.reached_components(self.directory, &types);
        let Some(text) = components.get(component) else {
            let object = label(found, object);
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

/// The conditions of a row: of an `If` and of a "wait until".
fn conditions_of(command: &Command) -> &[Condition] {
    match (command.get(), command.get()) {
        (Some(Builtin::If(conditions, ..)), _) | (_, Some(Flow::WaitUntil(conditions))) => {
            conditions
        }
        _ => &[],
    }
}

impl CommandRegistry {
    /// The checks of the events of a room: two events of one object (an error: the
    /// save point and a test find an event by the id of its object; by its name in a
    /// file with no ids), and for each command the check of its module.
    pub fn check_events(&self, events: &[EventDef], context: &CheckContext) -> Vec<Problem> {
        let mut problems = Vec::new();
        let mut texts = self.texts.lock().expect("no holder panics");
        texts.read(context.directory);
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
                // An object with no id: no command can name it, so no wire is checked.
                let Some(object) = event.id else { continue };
                if let Some(text) = context.output_problem(&object, &wire.output) {
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
            // The conditions of the pages: object 0 is the object of the event.
            for condition in event.pages.iter().flat_map(|page| &page.conditions) {
                if let Some(text) = condition_problem(condition, event.id, context) {
                    problems.push(Problem {
                        event: index,
                        object: event.object,
                        command: None,
                        warning: false,
                        text,
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
                    let conditions = conditions_of(command).iter();
                    let state = conditions
                        .filter_map(|condition| condition_problem(condition, event.id, context));
                    let findings = self.check(command, context).into_iter();
                    let keys = command.get().map(builtin::text_keys).unwrap_or_default();
                    let keys = keys.into_iter().filter_map(|key| texts.key_problem(&key.0));
                    let findings = findings.chain(keys.map(Finding::warning));
                    for finding in findings.chain(state.map(Finding::error)) {
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
            let spawn =
                |file: &str, spawn: &ObjectId| neighbors.has_spawn(&directory.join(file), spawn);
            let context = CheckContext {
                room: check.room,
                file: &file.to_string_lossy(),
                directory,
                spawn: &spawn,
                registry: &registry,
            };
            let problems = registry.check_events(&events, &context);
            // The errors of the text tables: no row has them.
            let texts = registry.texts.lock().expect("no holder panics");
            let tables = texts.problems.iter().map(|text| RoomProblem {
                warning: false,
                text: text.clone(),
            });
            let tables: Vec<RoomProblem> = tables.collect();
            drop(texts);
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
            tables
                .into_iter()
                .chain(problems.into_iter().map(with_line))
                .collect()
        })
    }
}

/// The spawn points of the room files that a `Transfer` names.
// The loader checks a room in a frame of the game. A file is read one time, and
// again only after it was written.
#[derive(Default)]
struct Neighbors(Mutex<HashMap<PathBuf, Neighbor>>);

/// When the file was written, and the room.
type Neighbor = (Option<SystemTime>, RoomDef);

impl Neighbors {
    /// `None`: the file is not there, or it is not a room.
    fn has_spawn(&self, path: &Path, spawn: &ObjectId) -> Option<bool> {
        let written = std::fs::metadata(path).ok()?.modified().ok();
        let mut files = self.0.lock().expect("no holder panics");
        if files.get(path).is_none_or(|(known, _)| *known != written) {
            let text = std::fs::read_to_string(path).ok()?;
            let room = maki_scene::parse_room(&text).ok()?;
            files.insert(path.to_owned(), (written, room));
        }
        Some(files[path].1.spawn(spawn).is_some())
    }
}

/// The texts of the fallback language under a directory of room files: what the key
/// pick of a tool offers.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FallbackTexts {
    pub language: String,
    /// Each key with its text, in the order of the files.
    pub texts: Vec<(String, String)>,
    /// The first table of the language, from the directory. `None`: no table yet.
    pub table: Option<PathBuf>,
}

impl CommandRegistry {
    /// The texts of the fallback language under `directory/data`. The tables are read
    /// again only after a change of a table file.
    pub fn fallback_texts(&self, directory: &Path) -> FallbackTexts {
        let mut texts = self.texts.lock().expect("no holder panics");
        texts.read(directory);
        FallbackTexts {
            language: texts.fallback.clone(),
            texts: texts.texts.clone(),
            table: texts.table.clone(),
        }
    }
}

/// What the check knows of the text tables of a directory of room files. The editor
/// checks the rooms in each frame: the tables are read again only when a table file
/// is new, gone, or written.
pub(crate) struct TextCheck {
    /// The language whose keys each row must have (`[text] fallback`).
    pub(crate) fallback: String,
    directory: PathBuf,
    /// Each table file, from the directory, and when it was written.
    files: Vec<(PathBuf, Option<SystemTime>)>,
    /// The keys of the tables of the fallback language.
    keys: HashSet<String>,
    /// The same keys with their texts, in the order of the files.
    texts: Vec<(String, String)>,
    /// The first table of the fallback language, from the directory.
    table: Option<PathBuf>,
    /// A table with no language, a key in two tables of one language.
    problems: Vec<String>,
}

impl Default for TextCheck {
    fn default() -> Self {
        Self {
            fallback: TextSettings::default().fallback,
            directory: PathBuf::new(),
            files: Vec::new(),
            keys: HashSet::new(),
            texts: Vec::new(),
            table: None,
            problems: Vec::new(),
        }
    }
}

impl TextCheck {
    /// Reads the tables under `directory/data` if they are not the ones of before.
    fn read(&mut self, directory: &Path) {
        let files: Vec<(PathBuf, Option<SystemTime>)> = crate::text::table_files(directory)
            .into_iter()
            .map(|path| {
                let metadata = std::fs::metadata(directory.join(&path));
                (path, metadata.and_then(|data| data.modified()).ok())
            })
            .collect();
        if self.directory == directory && self.files == files {
            return;
        }
        self.keys.clear();
        self.texts.clear();
        self.table = None;
        self.problems.clear();
        // The first table of each key of each language.
        let mut first: HashMap<(String, String), PathBuf> = HashMap::new();
        for (path, _) in &files {
            // A table that does not read: the texts log it, and the keys show.
            let Ok(table) = read_data::<TextTable>(&directory.join(path)) else {
                continue;
            };
            let file = path.display();
            if table.language == self.fallback && self.table.is_none() {
                self.table = Some(path.clone());
            }
            if table.language.is_empty() {
                self.problems
                    .push(format!("the text table `{file}` has no language"));
                continue;
            }
            for entry in &table.texts {
                let place = (table.language.clone(), entry.key.clone());
                if let Some(other) = first.get(&place) {
                    self.problems.push(format!(
                        "the key `{}` of the language `{}` is in `{}` and in `{file}`",
                        entry.key,
                        table.language,
                        other.display()
                    ));
                    continue;
                }
                first.insert(place, path.clone());
                if table.language == self.fallback {
                    // An empty text is a text not yet written: the key shows.
                    if !entry.text.is_empty() {
                        self.keys.insert(entry.key.clone());
                    }
                    self.texts.push((entry.key.clone(), entry.text.clone()));
                }
            }
        }
        self.directory = directory.to_owned();
        self.files = files;
    }

    /// `Some`: the fallback language has no text of the key.
    fn key_problem(&self, key: &str) -> Option<String> {
        (!self.keys.contains(key)).then(|| {
            format!(
                "the language `{}` has no text of the key `{key}`: the key shows",
                self.fallback
            )
        })
    }
}
