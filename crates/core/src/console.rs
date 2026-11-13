//! The commands of the console: a text line such as `flag door_open on`.
//!
//! Each module adds its commands with [`add_console_command`], as it adds its settings
//! and its event commands; a tool (the log panel) reads a line and calls [`run_line`].

use std::collections::BTreeMap;
use std::sync::Arc;

use bevy_ecs::resource::Resource;
use bevy_ecs::world::World;

use crate::App;

/// Runs a command with its words after the name. `Ok`: what it did, for the log.
pub type ConsoleFn = fn(&mut World, &[&str]) -> Result<String, String>;

pub struct ConsoleCommand {
    /// The words after the name, for the help: `NAME on|off`.
    pub usage: &'static str,
    pub help: &'static str,
    pub run: ConsoleFn,
}

/// The commands by name. An `Arc`: a command runs with the world, out of the resource.
#[derive(Resource, Default, Clone)]
pub struct ConsoleCommands(pub Arc<BTreeMap<&'static str, Arc<ConsoleCommand>>>);

/// False: no game runs (a tool edits its document), and each command but `help` is
/// refused: a command changes the game. A tool sets it; a game leaves it true.
#[derive(Resource)]
pub struct ConsoleGame(pub bool);

impl Default for ConsoleGame {
    fn default() -> Self {
        Self(true)
    }
}

/// Adds a command. A name added two times is a bug of a module.
pub fn add_console_command(
    app: &mut App,
    name: &'static str,
    usage: &'static str,
    help: &'static str,
    run: ConsoleFn,
) {
    if app.world().get_resource::<ConsoleCommands>().is_none() {
        app.init_resource::<ConsoleCommands>();
    }
    let mut commands = app.resource_mut::<ConsoleCommands>();
    let command = Arc::new(ConsoleCommand { usage, help, run });
    let added = Arc::make_mut(&mut commands.0).insert(name, command);
    assert!(added.is_none(), "console command `{name}` added two times");
}

/// Runs one line: the first word names the command. `help` lists the commands.
pub fn run_line(world: &mut World, line: &str) -> Result<String, String> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let Some((&name, args)) = words.split_first() else {
        return Ok(String::new());
    };
    let commands = world
        .get_resource::<ConsoleCommands>()
        .cloned()
        .unwrap_or_default();
    if name == "help" {
        let lines: Vec<String> = commands
            .0
            .iter()
            .map(|(name, command)| format!("{name} {}: {}", command.usage, command.help))
            .collect();
        return Ok(lines.join("\n"));
    }
    let command = commands
        .0
        .get(name)
        .ok_or_else(|| format!("no command `{name}`: `help` lists them"))?;
    if world
        .get_resource::<ConsoleGame>()
        .is_some_and(|game| !game.0)
    {
        return Err("no game runs: play it first".to_owned());
    }
    (command.run)(world, args).map_err(|error| format!("{error} (usage: {name} {})", command.usage))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ConfigFile;

    #[derive(Resource, Default)]
    struct Value(i32);

    #[test]
    fn a_line_runs_its_command_with_its_words() {
        let mut app = App::new(ConfigFile::default()).unwrap();
        add_console_command(&mut app, "set", "N", "Sets the value", |world, args| {
            let [n] = args else {
                return Err("one number".into());
            };
            world.resource_mut::<Value>().0 = n.parse().map_err(|_| "not a number")?;
            Ok(format!("value {n}"))
        });
        let mut world = World::new();
        world.insert_resource(app.world().resource::<ConsoleCommands>().clone());
        world.init_resource::<Value>();
        let world = &mut world;
        assert_eq!(run_line(world, "  set 7 "), Ok("value 7".into()));
        assert_eq!(world.resource::<Value>().0, 7);
        assert_eq!(
            run_line(world, "set"),
            Err("one number (usage: set N)".into())
        );
        assert!(run_line(world, "nope").unwrap_err().contains("no command"));
        world.insert_resource(ConsoleGame(false));
        assert!(run_line(world, "set 1").unwrap_err().contains("no game"));
        assert!(
            run_line(world, "help")
                .unwrap()
                .contains("set N: Sets the value")
        );
    }
}
