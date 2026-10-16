//! Event commands: an open set. A module adds its commands to the [`CommandRegistry`];
//! this crate adds the built-in ones (`builtin` module) in the same way.
//!
//! A module writes its commands as one enum, as it would for serde:
//!
//! ```ignore
//! #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
//! enum MyCommand {
//!     ShakeCamera(f32, f32),
//! }
//! registry.add(|command: &MyCommand, step: &mut Step, world: &mut World| { ...; true });
//! ```
//!
//! A room file writes `ShakeCamera(0.5, 0.1)`. The name of the variant selects the
//! enum, and the enum reads its arguments: the room file does not tell which module
//! a command comes from.
//!
//! How the name finds its enum: [`Command`] reads the variant name itself, looks for
//! the enum that has it in the registry, and gives the name and the arguments to the
//! serde code of that enum. The registry holds enums of types that this crate does
//! not know, so that last call goes through `erased-serde`.

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, RwLock};

use bevy_ecs::entity::Entity;
use bevy_ecs::resource::Resource;
use bevy_ecs::world::World;
use maki_core::Fields;
use maki_core::fields::{Kind, Value};
use maki_scene::{ComponentText, ObjectDef, RoomDef};
use serde::de::value::EnumAccessDeserializer;
use serde::de::{
    DeserializeOwned, DeserializeSeed, EnumAccess, Error as _, IntoDeserializer, VariantAccess,
    Visitor,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::GameState;
use crate::check::{CheckContext, Finding};
use crate::model::{EventDef, EventPages, RoomFile};
use crate::script::ScriptContext;

/// An enum of commands. Each type with these traits is one: derive them.
pub trait CommandSet:
    fmt::Debug + Clone + PartialEq + Serialize + DeserializeOwned + Send + Sync + 'static
{
}

impl<T> CommandSet for T where
    T: fmt::Debug + Clone + PartialEq + Serialize + DeserializeOwned + Send + Sync + 'static
{
}

/// A value of a command enum, with its type hidden.
trait AnyCommand: erased_serde::Serialize + fmt::Debug + Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn same(&self, other: &dyn Any) -> bool;
}

impl<T: CommandSet> AnyCommand for T {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn Any) -> bool {
        other.downcast_ref::<T>() == Some(self)
    }
}

erased_serde::serialize_trait_object!(AnyCommand);

/// Runs one step of a command. True: the command has ended. False: it waits, and
/// runs again in the next fixed step.
type Handler = Arc<dyn Fn(&dyn Any, &mut Step, &mut World) -> bool + Send + Sync>;

/// One step of an event: a value of the command enum of some module, and the handler
/// of that enum. It is written to a room file and read from it as the enum value.
#[derive(Clone)]
pub struct Command {
    value: Arc<dyn AnyCommand>,
    run: Handler,
    /// The number of the command in the text that it was read from: 1 for the
    /// first one, in the order of the text. 0: code made the command. A check finds
    /// the line of a command with it (`CommandRegistry::line`).
    pub(crate) order: u32,
}

impl Command {
    /// The command as a value of `T`. `None`: it is of another enum.
    pub fn get<T: CommandSet>(&self) -> Option<&T> {
        self.value.as_any().downcast_ref()
    }

    pub(crate) fn run(&self, step: &mut Step, world: &mut World) -> bool {
        (self.run)(self.value.as_any(), step, world)
    }

    pub(crate) fn as_any(&self) -> &dyn Any {
        self.value.as_any()
    }
}

/// As the enum value: `SetFlag("power_off", true)`.
impl fmt::Debug for Command {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        self.value.fmt(formatter)
    }
}

impl PartialEq for Command {
    fn eq(&self, other: &Self) -> bool {
        self.value.same(other.value.as_any())
    }
}

impl Serialize for Command {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.value.serialize(serializer)
    }
}

/// What a handler gets for one step of its command, beside the world.
pub struct Step<'a> {
    /// True in the first step of the command.
    pub start: bool,
    /// The name of the event, for log lines.
    pub event: &'a str,
    /// The id of the object of the event (`RunningEvent::id`).
    pub object: Option<maki_scene::ObjectId>,
    /// The entity of the object of the event, or of the script that runs the
    /// commands. `None`: a tool started them with no object.
    pub entity: Option<Entity>,
    pub(crate) memory: &'a mut Option<Box<dyn Any + Send + Sync>>,
    pub(crate) opened: &'a mut Option<(Vec<Command>, Option<Scope>)>,
    /// The variables of the routine that runs the command (`Step::call`). `None`: no
    /// routine.
    pub(crate) scope: Option<&'a mut Scope>,
    pub(crate) jump: &'a mut Jump,
}

/// What a command of the flow asks of the open lists when it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Jump {
    #[default]
    None,
    /// The opened list runs again each time it ends (`Loop`).
    Loop,
    /// The lists close up to the innermost loop, and that loop too (`Break`).
    Break,
}

/// The variables of a call of a routine: its arguments, by name.
pub(crate) type Scope = BTreeMap<String, i32>;

impl Step<'_> {
    /// A value that the command keeps from one of its steps to the next: the seconds
    /// that are left of a wait. The default value in the first step.
    pub fn memory<M: Default + Send + Sync + 'static>(&mut self) -> &mut M {
        if !self.memory.as_ref().is_some_and(|memory| memory.is::<M>()) {
            *self.memory = Some(Box::<M>::default());
        }
        let memory = self.memory.as_mut().expect("set above");
        memory.downcast_mut().expect("tested above")
    }

    /// When this command ends, the event runs these commands, then the command after
    /// this one. This is how `If` runs one of its two lists.
    pub fn open(&mut self, commands: Vec<Command>) {
        *self.opened = Some((commands, None));
    }

    /// As [`Self::open`], with variables of the call: in the list they hide the
    /// variables of the game of the same name. This is how a routine runs.
    pub fn call(&mut self, commands: Vec<Command>, variables: BTreeMap<String, i32>) {
        *self.opened = Some((commands, Some(variables)));
    }

    /// The flags and variables as the command sees them: in a routine, its
    /// arguments in place of the variables of the same name. A command that reads a
    /// condition or a variable reads it here.
    pub fn state<'w>(&self, world: &'w World) -> std::borrow::Cow<'w, GameState> {
        let state = world.resource::<GameState>();
        match self.scope.as_deref() {
            Some(scope) if !scope.is_empty() => {
                let mut state = state.clone();
                state.variables.extend(scope.clone());
                std::borrow::Cow::Owned(state)
            }
            _ => std::borrow::Cow::Borrowed(state),
        }
    }

    /// Sets a variable: an argument of the routine that runs, else a variable of the
    /// game.
    pub fn set_variable(&mut self, world: &mut World, name: &str, value: i32) {
        match self
            .scope
            .as_deref_mut()
            .and_then(|scope| scope.get_mut(name))
        {
            Some(argument) => *argument = value,
            None => world.resource_mut::<GameState>().set_variable(name, value),
        }
    }
}

/// One command enum in the registry.
#[derive(Clone)]
struct Set {
    /// Of the enum.
    type_id: TypeId,
    /// The names of the variants.
    names: &'static [&'static str],
    /// Reads a value of the enum.
    read: fn(&mut dyn erased_serde::Deserializer) -> ReadResult,
    run: Handler,
    /// The check of the module for a command of the enum (`add_check`).
    check: Option<Checker>,
    /// The texts of the module for a tool (`add_texts`).
    texts: Option<Texts>,
    /// The command lists that the commands of the enum hold (`add_lists`).
    lists: Option<Lists>,
    /// A new command for a name of the enum (`add_defaults`).
    defaults: Option<Defaults>,
    /// The field table of the enum, for the widgets of a tool (`add_fields`).
    fields: Option<FieldTable>,
}

/// The field table of a command enum (`#[derive(Fields)]`): its kind, the tree of a
/// value, and a value read from a tree.
#[derive(Clone)]
struct FieldTable {
    kind: Kind,
    tree: fn(&dyn Any) -> Value,
    read: fn(Value) -> Option<Arc<dyn AnyCommand>>,
}

/// A new command for a name, with arguments from the room. `None`: the module gives
/// none for the name.
type Defaults = Arc<dyn Fn(&str, &RoomDef) -> Option<Arc<dyn AnyCommand>> + Send + Sync>;

/// The command lists in a command, with the label of each, and the command made
/// again with other lists.
#[derive(Clone)]
struct Lists {
    get: GetLists,
    set: SetLists,
}

/// The command lists of a command, each with the text of the line before it.
pub type LabeledLists<'a> = Vec<(String, &'a [Command])>;
type GetLists = Arc<dyn for<'a> Fn(&'a dyn Any) -> LabeledLists<'a> + Send + Sync>;
type SetLists = Arc<dyn Fn(&dyn Any, Vec<Vec<Command>>) -> Arc<dyn AnyCommand> + Send + Sync>;

/// A command of the enum as a line for a reader. `None`: the module gives none.
type RowText = Arc<dyn Fn(&dyn Any) -> Option<String> + Send + Sync>;

/// The text of a command for a reader, and one line of help for a command name.
#[derive(Clone)]
struct Texts {
    row: RowText,
    help: fn(&str) -> Option<&'static str>,
}

/// Looks at one command with the room that it is in. `Some`: what is wrong.
type Checker = Arc<dyn Fn(&dyn Any, &CheckContext) -> Option<Finding> + Send + Sync>;

type ReadResult = Result<Arc<dyn AnyCommand>, erased_serde::Error>;

fn read<T: CommandSet>(deserializer: &mut dyn erased_serde::Deserializer) -> ReadResult {
    Ok(Arc::new(erased_serde::deserialize::<T>(deserializer)?))
}

/// A function of the script crate that a row calls (`CallScript`), with the object of
/// the event.
pub type ScriptFunction = fn(&mut ScriptContext);

/// All event commands of the game, and the script functions that a row calls.
/// `EventsModule` adds the resource.
// Shared, with a lock: the check of the room files (`maki_scene::RoomValidators`)
// holds a clone, and sees the commands that modules add after it was made.
#[derive(Resource, Clone, Default)]
pub struct CommandRegistry {
    /// A copy of `maki_scene::ComponentTypes`, for the check of `SetField`
    /// (`crate::check`).
    pub(crate) component_types: Arc<RwLock<maki_scene::ComponentTypes>>,
    sets: Arc<RwLock<Vec<Set>>>,
    functions: Arc<RwLock<BTreeMap<&'static str, ScriptFunction>>>,
    /// The outputs of each component type, by its name: what a wire joins.
    outputs: Arc<RwLock<BTreeMap<&'static str, &'static [&'static str]>>>,
}

impl CommandRegistry {
    /// Adds the commands of the enum `T`. Call from `Module::build`:
    /// `app.world().resource::<CommandRegistry>().add(handler)`.
    ///
    /// `handler` runs one step of a command in the event interpreter, with the full
    /// world. It returns true when the command has ended, false to run again in the
    /// next fixed step. Write a component or a resource only if the value is new: a
    /// write is a change for the renderer.
    ///
    /// Panics if another enum has a variant of the same name.
    pub fn add<T: CommandSet>(
        &self,
        handler: impl Fn(&T, &mut Step, &mut World) -> bool + Send + Sync + 'static,
    ) {
        let names = variant_names::<T>();
        let mut sets = self.sets.write().expect("no writer panics");
        for name in names {
            assert!(
                sets.iter().all(|set| !set.names.contains(name)),
                "event command `{name}` was added two times"
            );
        }
        sets.push(Set {
            type_id: TypeId::of::<T>(),
            names,
            read: read::<T>,
            run: Arc::new(move |command, step, world| {
                let command = command.downcast_ref().expect("a value of this enum");
                handler(command, step, world)
            }),
            check: None,
            texts: None,
            lists: None,
            defaults: None,
            fields: None,
        });
    }

    /// Gives a tool the field table of the enum `T` (`#[derive(Fields)]`): the event
    /// panel of the editor draws the arguments of its commands from it, with no code
    /// of the editor. A field `ObjectId` gets a pick of the objects.
    /// Call after `add::<T>`.
    ///
    /// Panics if the enum `T` was not added.
    pub fn add_fields<T: CommandSet + Fields>(&self) {
        let mut sets = self.sets.write().expect("no writer panics");
        let set = sets.iter_mut().find(|set| set.type_id == TypeId::of::<T>());
        let set = set.expect("add the commands before their fields");
        set.fields = Some(FieldTable {
            kind: T::kind(),
            tree: |command| {
                let command: &T = command.downcast_ref().expect("a value of this enum");
                command.tree()
            },
            read: |tree| {
                let command = maki_core::fields::read::<T>(tree).ok()?;
                Some(Arc::new(command))
            },
        });
    }

    fn field_table(&self, command: &Command) -> Option<(FieldTable, Handler)> {
        let value = command.as_any();
        let sets = self.sets.read().expect("no writer panics");
        let set = sets.iter().find(|set| set.type_id == value.type_id())?;
        Some((set.fields.clone()?, set.run.clone()))
    }

    /// The field table of the enum of a command, and the command as a tree. `None`:
    /// its module gave no table (`add_fields`).
    pub fn fields(&self, command: &Command) -> Option<(Kind, Value)> {
        let (table, _) = self.field_table(command)?;
        Some((table.kind, (table.tree)(command.as_any())))
    }

    /// A command of the enum of `command`, read from a changed tree. `None`: no
    /// table, or the enum does not read the tree.
    pub fn from_tree(&self, command: &Command, tree: Value) -> Option<Command> {
        let (table, run) = self.field_table(command)?;
        Some(Command {
            value: (table.read)(tree)?,
            run,
            order: 0,
        })
    }

    /// Tells which command lists the commands of the enum `T` hold: an `If` has
    /// two, a question one for each answer. Code that walks or changes all commands
    /// of an event (the checks, a tool) finds them with this, and knows no command
    /// of a module. Call after `add::<T>`.
    ///
    /// `get`: the lists of a command, each with the text of the line before it in a
    /// list of rows (`Else`); an empty text is no line. No list: an empty `Vec`.
    /// `set`: the same command with these lists, in the order of `get`.
    ///
    /// Panics if the enum `T` was not added.
    pub fn add_lists<T: CommandSet>(
        &self,
        get: for<'a> fn(&'a T) -> LabeledLists<'a>,
        set: fn(&T, Vec<Vec<Command>>) -> T,
    ) {
        let mut sets = self.sets.write().expect("no writer panics");
        let found = sets.iter_mut().find(|set| set.type_id == TypeId::of::<T>());
        let found = found.expect("add the commands before their lists");
        fn of<T: CommandSet>(command: &dyn Any) -> &T {
            command.downcast_ref().expect("a value of this enum")
        }
        found.lists = Some(Lists {
            get: Arc::new(move |command| get(of::<T>(command))),
            set: Arc::new(move |command, lists| Arc::new(set(of::<T>(command), lists))),
        });
    }

    fn lists_of(&self, command: &Command) -> Option<(Lists, Handler)> {
        let value = command.as_any();
        let sets = self.sets.read().expect("no writer panics");
        let set = sets.iter().find(|set| set.type_id == value.type_id())?;
        Some((set.lists.clone()?, set.run.clone()))
    }

    /// The command lists in a command, each with the text of the line before it.
    /// Empty: the command holds no list.
    pub fn lists<'a>(&self, command: &'a Command) -> LabeledLists<'a> {
        match self.lists_of(command) {
            Some((lists, _)) => (lists.get)(command.as_any()),
            None => Vec::new(),
        }
    }

    /// A command with other lists, in the order of [`Self::lists`]. `None`: the
    /// command holds no list.
    pub fn with_lists(&self, command: &Command, lists: Vec<Vec<Command>>) -> Option<Command> {
        let (of_set, run) = self.lists_of(command)?;
        Some(Command {
            value: (of_set.set)(command.as_any(), lists),
            run,
            order: 0,
        })
    }

    /// Calls `visit` for each command of a list, and for each command in a list of
    /// a command, in the order of the text of a room file.
    pub fn each_command(&self, commands: &[Command], visit: &mut dyn FnMut(&Command)) {
        for command in commands {
            visit(command);
            for (_, list) in self.lists(command) {
                self.each_command(list, visit);
            }
        }
    }

    /// Gives a tool (an editor) the texts of the commands of the enum `T`. `row`:
    /// one command as a line for a reader (`Message: The door is locked.`); `None`:
    /// the tool shows the command as a room file has it. `help`: one line that
    /// tells what the command of a name does. Call after `add::<T>`.
    ///
    /// Panics if the enum `T` was not added.
    pub fn add_texts<T: CommandSet>(
        &self,
        row: impl Fn(&T) -> Option<String> + Send + Sync + 'static,
        help: fn(&str) -> Option<&'static str>,
    ) {
        let mut sets = self.sets.write().expect("no writer panics");
        let set = sets.iter_mut().find(|set| set.type_id == TypeId::of::<T>());
        let set = set.expect("add the commands before their texts");
        let row = Arc::new(move |command: &dyn Any| {
            row(command.downcast_ref().expect("a value of this enum"))
        });
        set.texts = Some(Texts { row, help });
    }

    /// A command as a line for a reader, if its module gave one (`add_texts`).
    pub fn row_text(&self, command: &Command) -> Option<String> {
        let value = command.as_any();
        let sets = self.sets.read().expect("no writer panics");
        let set = sets.iter().find(|set| set.type_id == value.type_id())?;
        let texts = set.texts.clone()?;
        drop(sets);
        (texts.row)(value)
    }

    /// One line of help for the command of a name, if its module gave one.
    pub fn help(&self, name: &str) -> Option<&'static str> {
        let texts = self.find(name)?.texts?;
        (texts.help)(name)
    }

    /// Adds a check for the commands of the enum `T`: a room file is checked with it
    /// when the game loads the room, and a tool shows what it finds. It gets one
    /// command and what the room has (`CheckContext`). An error stops the load of
    /// the room; a warning is logged. Call after `add::<T>`.
    ///
    /// Panics if the enum `T` was not added.
    pub fn add_check<T: CommandSet>(
        &self,
        check: impl Fn(&T, &CheckContext) -> Option<Finding> + Send + Sync + 'static,
    ) {
        let mut sets = self.sets.write().expect("no writer panics");
        let set = sets.iter_mut().find(|set| set.type_id == TypeId::of::<T>());
        let set = set.expect("add the commands before their check");
        set.check = Some(Arc::new(move |command, context| {
            let command = command.downcast_ref().expect("a value of this enum");
            check(command, context)
        }));
    }

    /// Gives a tool (an editor) a new command for the names of the enum `T`: what
    /// its "add a command" list makes. `new` gets the name and the room, and takes
    /// an argument that the room has where it can (the name of a light). `None`
    /// for a name: the tool asks the user for the text of the command. Call after
    /// `add::<T>`.
    ///
    /// Panics if the enum `T` was not added.
    pub fn add_defaults<T: CommandSet>(&self, new: fn(&str, &RoomDef) -> Option<T>) {
        let mut sets = self.sets.write().expect("no writer panics");
        let set = sets.iter_mut().find(|set| set.type_id == TypeId::of::<T>());
        let set = set.expect("add the commands before their defaults");
        set.defaults = Some(Arc::new(move |name, room| {
            new(name, room).map(|command| Arc::new(command) as Arc<dyn AnyCommand>)
        }));
    }

    /// A new command of this name for a room, if its module gave one
    /// (`add_defaults`).
    pub fn new_command(&self, name: &str, room: &RoomDef) -> Option<Command> {
        let set = self.find(name)?;
        Some(Command {
            value: (set.defaults?)(name, room)?,
            run: set.run,
            order: 0,
        })
    }

    /// What the check of its module finds in a command.
    pub(crate) fn check(&self, command: &Command, context: &CheckContext) -> Option<Finding> {
        let value = command.as_any();
        let sets = self.sets.read().expect("no writer panics");
        let set = sets.iter().find(|set| set.type_id == value.type_id())?;
        // The lock is free while the check runs.
        let check = set.check.clone()?;
        drop(sets);
        check(value, context)
    }

    /// A command from a value of its enum: for a tool that makes or changes events
    /// (an editor). `None`: no module added the enum `T`.
    pub fn command<T: CommandSet>(&self, value: T) -> Option<Command> {
        let sets = self.sets.read().expect("no writer panics");
        let set = sets.iter().find(|set| set.type_id == TypeId::of::<T>())?;
        Some(Command {
            value: Arc::new(value),
            run: set.run.clone(),
            order: 0,
        })
    }

    /// Adds a function that the row `CallScript` calls by its name. A name that is in
    /// use gets the new function: a new build of the script DLL adds them again.
    pub fn add_function(&self, name: &'static str, function: ScriptFunction) {
        let mut functions = self.functions.write().expect("no writer panics");
        functions.insert(name, function);
    }

    /// The script function of a name.
    pub fn function(&self, name: &str) -> Option<ScriptFunction> {
        self.functions
            .read()
            .expect("no writer panics")
            .get(name)
            .copied()
    }

    /// The names of the script functions, in the order of the names.
    pub fn functions(&self) -> Vec<&'static str> {
        self.functions
            .read()
            .expect("no writer panics")
            .keys()
            .copied()
            .collect()
    }

    /// Names the outputs of a component type (`add_outputs`).
    pub fn add_outputs(&self, component: &'static str, outputs: &'static [&'static str]) {
        let mut all = self.outputs.write().expect("no writer panics");
        all.insert(component, outputs);
    }

    /// The outputs of an object: those of each of its components.
    pub fn outputs_of(&self, object: &ObjectDef) -> Vec<&'static str> {
        let all = self.outputs.read().expect("no writer panics");
        let names = object.components.keys();
        let outputs = names.filter_map(|name| all.get(name.as_str()));
        outputs
            .flat_map(|outputs| outputs.iter().copied())
            .collect()
    }

    /// The names of all commands, in the order in which they were added.
    pub fn names(&self) -> Vec<&'static str> {
        let sets = self.sets.read().expect("no writer panics");
        sets.iter().flat_map(|set| set.names).copied().collect()
    }

    fn find(&self, name: &str) -> Option<Set> {
        let sets = self.sets.read().expect("no writer panics");
        sets.iter().find(|set| set.names.contains(&name)).cloned()
    }

    /// The events in the text of a room file. The error has the line and the column,
    /// also for a command that no module added and for a bad argument.
    pub fn parse_events(&self, text: &str) -> Result<Vec<EventDef>, ron::error::SpannedError> {
        Ok(self.parse::<RoomFile>(text, None)?.events())
    }

    /// The pages of the text of a component `event`.
    pub fn parse_pages(
        &self,
        text: &ComponentText,
    ) -> Result<EventPages, ron::error::SpannedError> {
        self.parse(text.as_str(), None)
    }

    pub(crate) fn parse<T: DeserializeOwned>(
        &self,
        text: &str,
        stop: Option<Stop>,
    ) -> Result<T, ron::error::SpannedError> {
        let before = PARSING.replace(Some(Parsing {
            registry: self.clone(),
            commands: 0,
            events: 0,
            stop,
        }));
        let value = maki_scene::ron_options().from_str::<T>(text);
        PARSING.set(before);
        value
    }

    /// The line of a command or an event in the text of a room file that reads with
    /// no error.
    // A value that serde made has no place in the text. So the text is read a second
    // time, and the read stops with an error at the command or the event: the
    // parser gives the place of an error.
    pub(crate) fn line(&self, text: &str, stop: Stop) -> Option<usize> {
        let error = self.parse::<RoomFile>(text, Some(stop)).err()?;
        Some(error.span.end.line)
    }

    /// One command from its text in the form of a room file: `Wait(0.5)`. For a
    /// tool that lets the user type a command of a module that it does not know.
    pub fn parse_command(&self, text: &str) -> Result<Command, ron::error::SpannedError> {
        self.parse(text, None)
    }
}

/// Where the read of a text stops (`CommandRegistry::line`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stop {
    /// At the command with this `Command::order`.
    Command(u32),
    /// At the name of the event with this number, from 1.
    Event(u32),
}

/// The read of a text that runs on this thread.
struct Parsing {
    registry: CommandRegistry,
    /// The commands and the events that were read.
    commands: u32,
    events: u32,
    stop: Option<Stop>,
}

const STOPPED: &str = "the read stops here";

/// Counts an event: the reader is at the component `event` of an object.
pub(crate) fn event_starts<E: serde::de::Error>() -> Result<(), E> {
    let stop = PARSING.with_borrow_mut(|parsing| {
        parsing.as_mut().is_some_and(|parsing| {
            parsing.events += 1;
            parsing.stop == Some(Stop::Event(parsing.events))
        })
    });
    if stop {
        return Err(E::custom(STOPPED));
    }
    Ok(())
}

thread_local! {
    /// The `parse_events` call that runs on this thread. serde gives no way to hand
    /// a value down to the `Deserialize` code of a nested type (the commands in the
    /// lists of an `If`), so `Command` finds the registry here.
    static PARSING: RefCell<Option<Parsing>> = const { RefCell::new(None) };
}

impl<'de> Deserialize<'de> for Command {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // The list of variants is for error texts of serde. The set is open: no list.
        deserializer.deserialize_enum("Command", &[], CommandVisitor)
    }
}

struct CommandVisitor;

impl<'de> Visitor<'de> for CommandVisitor {
    type Value = Command;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "an event command")
    }

    fn visit_enum<A: EnumAccess<'de>>(self, access: A) -> Result<Command, A::Error> {
        let (name, variant) = access.variant_seed(VariantName)?;
        // Counted here, before the arguments: a command in a list of an `If` has a
        // number after that of its `If`, as in the text.
        let parsing = PARSING.with_borrow_mut(|parsing| {
            let parsing = parsing.as_mut()?;
            parsing.commands += 1;
            let stop = parsing.stop == Some(Stop::Command(parsing.commands));
            Some((parsing.registry.clone(), parsing.commands, stop))
        });
        let (registry, order, stop) = parsing.ok_or_else(|| {
            A::Error::custom("commands are read with `CommandRegistry::parse_events`")
        })?;
        if stop {
            return Err(A::Error::custom(STOPPED));
        }
        let Some(set) = registry.find(&name) else {
            let known = registry.names().join(", ");
            return Err(A::Error::custom(format!(
                "no command `{name}`. The commands are: {known}"
            )));
        };
        // The serde code of the enum gets the name back, and then the arguments.
        let named = Named {
            name: &name,
            variant,
        };
        let mut erased =
            <dyn erased_serde::Deserializer>::erase(EnumAccessDeserializer::new(named));
        let value = (set.read)(&mut erased).map_err(A::Error::custom)?;
        Ok(Command {
            value,
            run: set.run,
            order,
        })
    }
}

/// Reads the name of an enum variant as a text.
struct VariantName;

impl<'de> DeserializeSeed<'de> for VariantName {
    type Value = String;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<String, D::Error> {
        deserializer.deserialize_identifier(self)
    }
}

impl Visitor<'_> for VariantName {
    type Value = String;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "the name of a command")
    }

    fn visit_str<E>(self, name: &str) -> Result<String, E> {
        Ok(name.to_owned())
    }
}

/// An enum value whose variant name was read before: the name, and the arguments
/// that are still in the file.
struct Named<'a, V> {
    name: &'a str,
    variant: V,
}

impl<'de, V: VariantAccess<'de>> EnumAccess<'de> for Named<'_, V> {
    type Error = V::Error;
    type Variant = V;

    fn variant_seed<S: DeserializeSeed<'de>>(self, seed: S) -> Result<(S::Value, V), V::Error> {
        let name = seed.deserialize(self.name.into_deserializer())?;
        Ok((name, self.variant))
    }
}

/// The variant names of an enum with a serde derive. The derive gives the list to
/// the deserializer (`deserialize_enum`): this asks for a value from a deserializer
/// that only keeps that list.
fn variant_names<T: DeserializeOwned>() -> &'static [&'static str] {
    struct Probe<'a>(&'a mut &'static [&'static str]);

    impl<'de> Deserializer<'de> for Probe<'_> {
        type Error = serde::de::value::Error;

        fn deserialize_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value, Self::Error> {
            Err(Self::Error::custom("not an enum"))
        }

        fn deserialize_enum<V: Visitor<'de>>(
            self,
            _name: &'static str,
            variants: &'static [&'static str],
            _: V,
        ) -> Result<V::Value, Self::Error> {
            *self.0 = variants;
            Err(Self::Error::custom("the list is all that was asked for"))
        }

        serde::forward_to_deserialize_any! {
            bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string bytes byte_buf
            option unit unit_struct newtype_struct seq tuple tuple_struct map struct
            identifier ignored_any
        }
    }

    let mut names: &[&str] = &[];
    let _ = T::deserialize(Probe(&mut names));
    assert!(
        !names.is_empty(),
        "{} is not an enum with a serde derive",
        std::any::type_name::<T>()
    );
    names
}
