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
use std::fmt;
use std::sync::{Arc, RwLock};

use bevy_ecs::resource::Resource;
use bevy_ecs::world::World;
use serde::de::value::EnumAccessDeserializer;
use serde::de::{
    DeserializeOwned, DeserializeSeed, EnumAccess, Error as _, IntoDeserializer, VariantAccess,
    Visitor,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::model::{EventDef, RoomFile};

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
}

impl Command {
    /// The command as a value of `T`. `None`: it is of another enum.
    pub fn get<T: CommandSet>(&self) -> Option<&T> {
        self.value.as_any().downcast_ref()
    }

    pub(crate) fn run(&self, step: &mut Step, world: &mut World) -> bool {
        (self.run)(self.value.as_any(), step, world)
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
    pub(crate) memory: &'a mut Option<Box<dyn Any + Send + Sync>>,
    pub(crate) opened: &'a mut Option<Vec<Command>>,
}

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
        *self.opened = Some(commands);
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
}

type ReadResult = Result<Arc<dyn AnyCommand>, erased_serde::Error>;

fn read<T: CommandSet>(deserializer: &mut dyn erased_serde::Deserializer) -> ReadResult {
    Ok(Arc::new(erased_serde::deserialize::<T>(deserializer)?))
}

/// All event commands of the game. `EventsModule` adds the resource.
// Shared, with a lock: the check of the room files (`maki_scene::RoomValidators`)
// holds a clone, and sees the commands that modules add after it was made.
#[derive(Resource, Clone, Default)]
pub struct CommandRegistry(Arc<RwLock<Vec<Set>>>);

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
        let mut sets = self.0.write().expect("no writer panics");
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
        });
    }

    /// A command from a value of its enum: for a tool that makes or changes events
    /// (an editor). `None`: no module added the enum `T`.
    pub fn command<T: CommandSet>(&self, value: T) -> Option<Command> {
        let sets = self.0.read().expect("no writer panics");
        let set = sets.iter().find(|set| set.type_id == TypeId::of::<T>())?;
        Some(Command {
            value: Arc::new(value),
            run: set.run.clone(),
        })
    }

    /// The names of all commands, in the order in which they were added.
    pub fn names(&self) -> Vec<&'static str> {
        let sets = self.0.read().expect("no writer panics");
        sets.iter().flat_map(|set| set.names).copied().collect()
    }

    fn find(&self, name: &str) -> Option<Set> {
        let sets = self.0.read().expect("no writer panics");
        sets.iter().find(|set| set.names.contains(&name)).cloned()
    }

    /// The events in the text of a room file. The error has the line and the column,
    /// also for a command that no module added and for a bad argument.
    pub fn parse_events(&self, text: &str) -> Result<Vec<EventDef>, ron::error::SpannedError> {
        let before = PARSING.replace(Some(self.clone()));
        let file = maki_scene::ron_options().from_str::<RoomFile>(text);
        PARSING.set(before);
        Ok(file?.events)
    }

    /// One command from its text in the form of a room file: `Wait(0.5)`. For a
    /// tool that lets the user type a command of a module that it does not know.
    pub fn parse_command(&self, text: &str) -> Result<Command, ron::error::SpannedError> {
        let before = PARSING.replace(Some(self.clone()));
        let command = maki_scene::ron_options().from_str::<Command>(text);
        PARSING.set(before);
        command
    }

    /// A check for `maki_scene::RoomValidators`: a room with a bad event is not loaded.
    pub fn validate_events(&self, text: &str) -> Result<(), String> {
        self.parse_events(text)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

thread_local! {
    /// The registry of the `parse_events` call that runs on this thread. serde gives
    /// no way to hand a value down to the `Deserialize` code of a nested type (the
    /// commands in the lists of an `If`), so `Command` finds the registry here.
    static PARSING: RefCell<Option<CommandRegistry>> = const { RefCell::new(None) };
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
        let registry = PARSING.with_borrow(Clone::clone).ok_or_else(|| {
            A::Error::custom("commands are read with `CommandRegistry::parse_events`")
        })?;
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
