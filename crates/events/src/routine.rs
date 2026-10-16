//! Routines: a command list in a file of its own (`<name>.routine.ron`), with
//! arguments. A row `CallRoutine(file, arguments)` and a script
//! (`ScriptContext::routine`) run it as an opened list.
//!
//! ```ron
//! (
//!     arguments: ["amount"],
//!     commands: [If([Variable("amount", AtLeast, 5)], [Message("Many.")], [])],
//! )
//! ```
//!
//! In the routine, an argument is a variable of the call: it hides a variable of the
//! game of the same name, for the conditions and for `SetVariable` and `AddVariable`.
//! The variable of the game does not change.
//!
//! The files load as the prefab files do: at the first call, and again after the
//! watch tells of a change (`maki_assets::AssetChanged`).

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};

use bevy_ecs::change_detection::DetectChanges;
use bevy_ecs::message::MessageReader;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Res, ResMut};
use bevy_ecs::world::World;
use maki_assets::{AssetChanged, AssetWatch, FileRef};
use maki_core::fields::{Kind, Value};
use maki_core::{App, Fields, Update};
use maki_scene::CurrentRoom;
use serde::de::{IgnoredAny, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::check::{CheckContext, Finding};
use crate::command::{Command, CommandRegistry, Step};

/// The end of the name of a routine file.
pub const ROUTINE_EXTENSION: &str = "routine.ron";

/// The content of a routine file.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutineDef {
    /// The names of the arguments, in the order of a call. Each is a whole number.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<Command>,
}

impl CommandRegistry {
    /// A routine from the text of its file. The error has the line and the column.
    pub fn parse_routine(&self, text: &str) -> Result<RoutineDef, ron::error::SpannedError> {
        self.parse(text, None)
    }
}

/// The text of a routine file.
pub fn routine_text(routine: &RoutineDef) -> String {
    let config = ron::ser::PrettyConfig::default();
    let text = maki_scene::ron_options().to_string_pretty(routine, config);
    text.expect("a routine has no value that RON cannot write")
}

/// The command of this module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
pub enum RoutineCommand {
    /// Runs a routine file (from the directory of the room file) with a number for
    /// each of its arguments.
    CallRoutine(
        #[field(label = "routine file", file = "routine")] FileRef,
        #[field(label = "arguments")] Arguments,
    ),
}

/// The numbers of a call, in the order of the arguments of the routine. A room file
/// writes a list (`[3, 5]`); a tool edits a text (`3, 5`).
// ponytail: a text widget, as the fields have no list kind; a list kind if more types need one.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Arguments(pub Vec<i32>);

impl Serialize for Arguments {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Arguments {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Numbers;

        impl<'de> Visitor<'de> for Numbers {
            type Value = Arguments;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                write!(formatter, "a list of whole numbers")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Arguments, A::Error> {
                let mut numbers = Vec::new();
                while let Some(number) = seq.next_element()? {
                    numbers.push(number);
                }
                Ok(Arguments(numbers))
            }

            // The text of a tool: numbers with commas or spaces between them.
            fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<Arguments, E> {
                let parts = text.split([',', ' ']).filter(|part| !part.is_empty());
                let numbers = parts.map(|part| part.parse::<i32>().map_err(E::custom));
                Ok(Arguments(numbers.collect::<Result<_, _>>()?))
            }
        }

        deserializer.deserialize_any(Numbers)
    }
}

impl Fields for Arguments {
    fn kind() -> Kind {
        Kind::Text
    }

    fn tree(&self) -> Value {
        let numbers: Vec<String> = self.0.iter().map(i32::to_string).collect();
        Value::String(numbers.join(", "))
    }
}

/// The routine files that were read, by path. A file that changed on the disk is
/// read again at its next call.
#[derive(Resource, Default)]
pub struct Routines(HashMap<PathBuf, RoutineDef>);

impl Routines {
    /// The routine of a file: from the store, else read now.
    fn get(&mut self, path: &Path, registry: &CommandRegistry) -> Result<RoutineDef, String> {
        if let Some(routine) = self.0.get(path) {
            return Ok(routine.clone());
        }
        let routine = read_routine(path, registry)?;
        self.0.insert(path.to_owned(), routine.clone());
        Ok(routine)
    }
}

/// Reads a routine file.
pub fn read_routine(path: &Path, registry: &CommandRegistry) -> Result<RoutineDef, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("no routine file {}: {error}", path.display()))?;
    registry
        .parse_routine(&text)
        .map_err(|error| format!("{}:{error}", path.display()))
}

/// The check of the watch: the file reads as a routine (the watch has no registry).
fn check_file(path: &Path) -> anyhow::Result<()> {
    #[derive(Deserialize)]
    #[allow(dead_code)]
    struct Light {
        #[serde(default)]
        arguments: Vec<String>,
        #[serde(default)]
        commands: IgnoredAny,
    }
    let text = std::fs::read_to_string(path)?;
    maki_scene::ron_options().from_str::<Light>(&text)?;
    Ok(())
}

/// One step of `CallRoutine`: opens the routine, with the arguments as variables.
fn run(command: &RoutineCommand, step: &mut Step, world: &mut World) -> bool {
    let RoutineCommand::CallRoutine(file, arguments) = command;
    let current = world.resource::<CurrentRoom>();
    let path = file.resolve(current.path.parent().unwrap_or(Path::new("")));
    let registry = world.resource::<CommandRegistry>().clone();
    let routine = world.resource_mut::<Routines>().get(&path, &registry);
    let routine = match routine {
        Ok(routine) => routine,
        Err(error) => {
            tracing::error!(event = step.event, "CallRoutine: {error}");
            return true;
        }
    };
    if routine.arguments.len() != arguments.0.len() {
        tracing::warn!(
            event = step.event,
            file = %file.display(),
            "CallRoutine: {} arguments for {}; a missing one is 0",
            arguments.0.len(),
            routine.arguments.len()
        );
    }
    let numbers = arguments.0.iter().copied().chain(std::iter::repeat(0));
    let variables: BTreeMap<String, i32> = routine.arguments.into_iter().zip(numbers).collect();
    step.call(routine.commands, variables);
    true
}

/// The check of `CallRoutine`: the file reads, and the count of arguments is right.
fn check(command: &RoutineCommand, context: &CheckContext) -> Option<Finding> {
    let RoutineCommand::CallRoutine(file, arguments) = command;
    let path = file.resolve(context.directory);
    if !path.is_file() {
        return Some(Finding::error(format!(
            "no routine file `{}`",
            file.display()
        )));
    }
    let routine = match read_routine(&path, context.registry) {
        Ok(routine) => routine,
        Err(error) => return Some(Finding::error(error)),
    };
    let (wanted, given) = (routine.arguments.len(), arguments.0.len());
    (wanted != given).then(|| {
        Finding::error(format!(
            "`{}` has {wanted} arguments, the call gives {given}",
            file.display()
        ))
    })
}

fn row_text(command: &RoutineCommand) -> Option<String> {
    let RoutineCommand::CallRoutine(file, arguments) = command;
    let numbers: Vec<String> = arguments.0.iter().map(i32::to_string).collect();
    Some(format!(
        "Call the routine {} ({})",
        file.display(),
        numbers.join(", ")
    ))
}

fn help(name: &str) -> Option<&'static str> {
    (name == "CallRoutine").then_some("Runs the commands of a routine file, with its arguments.")
}

/// Runs in `Update`: a routine file that changed is read again at its next call.
fn forget_changed(mut changed: MessageReader<AssetChanged>, mut routines: ResMut<Routines>) {
    for AssetChanged(path) in changed.read() {
        if routines.0.contains_key(path) {
            routines.0.remove(path);
        }
    }
}

/// Runs in `Update`: tells the watch the routine files that were read.
fn watch_routines(routines: Res<Routines>, watch: Option<ResMut<AssetWatch>>) {
    let Some(mut watch) = watch.filter(|watch| watch.on) else {
        return;
    };
    if routines.is_changed() {
        let check: maki_assets::Check = check_file;
        let files = routines.0.keys().map(|path| (path.clone(), check));
        watch.set("routines", files.collect());
    }
}

/// Adds the command `CallRoutine`, the loaded files, and their watch.
pub(crate) fn add(app: &mut App, registry: &CommandRegistry) {
    registry.add(run);
    registry.add_fields::<RoutineCommand>();
    registry.add_check(check);
    registry.add_texts(row_text, help);
    registry.add_defaults(|_, _| {
        Some(RoutineCommand::CallRoutine(
            Default::default(),
            Arguments::default(),
        ))
    });
    app.init_resource::<Routines>()
        .add_systems(Update, (forget_changed, watch_routines).chain());
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy_ecs::system::{Commands, ResMut};
    use maki_core::{App, ConfigFile, Interpolated, Startup, Transform};
    use maki_platform::PlatformModule;
    use maki_scene::{ObjectId, Player, RoomRequest, RoomTarget, SceneModule};

    use super::*;
    use crate::{EventsModule, GameState, ScriptContext, add_script_function, short_events};

    const ROUTINE: &str = "routines/count.routine.ron";

    /// A routine that counts a big and a small argument, and writes its argument.
    const COUNT: &str = r#"(arguments: ["amount"], commands: [
        If([Variable("amount", AtLeast, 5)], [AddVariable("big", 1)], [AddVariable("small", 1)]),
        AddVariable("amount", 100),
        If([Variable("amount", AtLeast, 100)], [AddVariable("written", 1)], []),
    ])"#;

    fn directory(test: &str) -> PathBuf {
        let name = format!("maki-routine-{test}-{}", std::process::id());
        let directory = std::env::temp_dir().join(name);
        std::fs::create_dir_all(directory.join("routines")).unwrap();
        directory
    }

    fn room(events: &str) -> String {
        let events = short_events(events);
        let objects = [
            r#"(id: 1, name: "start", components: {"spawn": ()})"#,
            &events,
        ];
        maki_scene::room_text(8.0, 8.0, 3.0, &objects)
    }

    /// A headless game in `a.ron` of `directory`.
    fn game(directory: &Path, events: &str) -> App {
        std::fs::write(directory.join("a.ron"), room(events)).unwrap();
        let path = directory.join("a.ron");
        let mut app = App::new(ConfigFile::from_text("[time]\nfixed_hz = 50.0")).unwrap();
        app.add_module(PlatformModule)
            .add_module(SceneModule)
            .add_module(EventsModule)
            .add_systems(
                Startup,
                move |mut commands: Commands, mut request: ResMut<RoomRequest>| {
                    commands.spawn((Player, Interpolated::at(Transform::IDENTITY)));
                    request.0 = Some(RoomTarget {
                        path: path.clone(),
                        spawn: Some(ObjectId(1)),
                        text: None,
                    });
                },
            );
        app
    }

    fn steps(app: &mut App, count: u32) {
        for _ in 0..count {
            app.advance(Duration::from_millis(20));
        }
    }

    fn variable(app: &App, name: &str) -> i32 {
        app.world().resource::<GameState>().variable(name)
    }

    #[test]
    fn a_routine_with_an_argument_does_the_same_work_from_two_events_and_a_script() {
        let directory = directory("calls");
        std::fs::write(directory.join(ROUTINE), COUNT).unwrap();
        let events = format!(
            r#"(name: "first", pages: [(trigger: Autorun, commands: [
                SetVariable("amount", 1), CallRoutine("{ROUTINE}", [7])])]),
            (name: "second", pages: [(trigger: Autorun, commands: [
                CallRoutine("{ROUTINE}", [2]), CallScript("call")])])"#
        );
        let mut app = game(&directory, &events);
        add_script_function(&mut app, "call", |context: &mut ScriptContext| {
            context.routine(ROUTINE, &[9]);
        });
        app.run(|mut app| {
            steps(&mut app, 8);
            // 7 and 9 are big, 2 is small.
            assert_eq!((variable(&app, "big"), variable(&app, "small")), (2, 1));
            // In the routine the argument was written; the variable of the game was not.
            assert_eq!(variable(&app, "written"), 3);
            assert_eq!(variable(&app, "amount"), 1);
        });
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_changed_routine_file_runs_in_its_new_form() {
        let directory = directory("changed");
        let path = directory.join(ROUTINE);
        std::fs::write(&path, r#"(commands: [AddVariable("old", 1)])"#).unwrap();
        let events = format!(
            r#"(name: "caller", pages: [(trigger: Parallel, commands: [
                CallRoutine("{ROUTINE}", []), Wait(0.04)])])"#
        );
        let app = game(&directory, &events);
        app.run(|mut app| {
            steps(&mut app, 3);
            assert_eq!(variable(&app, "old"), 1);
            std::fs::write(&path, r#"(commands: [AddVariable("new", 1)])"#).unwrap();
            // The watch tells of the change.
            check_file(&path).unwrap();
            app.write_message(AssetChanged(path.clone()));
            steps(&mut app, 8);
            assert_eq!(variable(&app, "old"), 1);
            assert!(variable(&app, "new") >= 1);
        });
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn the_check_reports_a_missing_file_and_a_wrong_count_of_arguments() {
        let directory = directory("check");
        std::fs::write(directory.join(ROUTINE), COUNT).unwrap();
        let mut app = App::new(ConfigFile::default()).unwrap();
        app.add_module(PlatformModule)
            .add_module(SceneModule)
            .add_module(EventsModule);
        app.update();
        let validators = app.world().resource::<maki_scene::RoomValidators>();
        let events = format!(
            r#"(name: "a", pages: [(trigger: Interact, commands: [
                CallRoutine("{ROUTINE}", [1]), CallRoutine("routines/gone.routine.ron", []),
                CallRoutine("{ROUTINE}", [1, 2])])])"#
        );
        let path = directory.join("a.ron");
        let error = maki_scene::check_room(&path, &room(&events), validators)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("no routine file `routines/gone.routine.ron`"),
            "{error}"
        );
        assert!(
            error.contains("has 1 arguments, the call gives 2"),
            "{error}"
        );
        assert_eq!(error.lines().count(), 2, "{error}");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_routine_reads_back_and_its_arguments_read_from_the_text_of_a_tool() {
        let registry = CommandRegistry::default();
        registry.add(crate::builtin::run);
        let routine = registry.parse_routine(COUNT).unwrap();
        assert_eq!(routine.arguments, ["amount"]);
        assert_eq!(
            registry.parse_routine(&routine_text(&routine)).unwrap(),
            routine
        );
        let call = RoutineCommand::CallRoutine(ROUTINE.into(), Arguments(vec![3, -5]));
        let tree = call.tree();
        assert_eq!(tree["CallRoutine"][1], Value::String("3, -5".to_owned()));
        assert_eq!(
            maki_core::fields::read::<RoutineCommand>(tree).unwrap(),
            call
        );
    }
}
