//! The commands of a tool: one list. The menus, the tool buttons, the keys, the help
//! panel, and the command search read it, so a command has one name, one key, and
//! one rule for "can run now" at each place.
//!
//! A module adds a command with [`add_command`], as it adds a panel. The key of a
//! command is not written here: the command names an input action, and the binding
//! table has its key.

use std::sync::Arc;

use bevy_ecs::resource::Resource;
use bevy_ecs::world::World;
use maki_core::App;
use maki_platform::{Action, ActionState, Bindings, InputMode};

/// Opens the command search. A tool gives it a key in its input mode.
pub const COMMAND_SEARCH: Action = Action("command_search");

#[derive(Clone)]
pub struct ToolCommand {
    /// Unique, and the same in each version: `editor.save`.
    pub id: &'static str,
    pub name: &'static str,
    /// The menu of the menu row that lists it ([`crate::add_menu`]). Empty: the
    /// command search only.
    pub menu: &'static str,
    /// The action whose key starts the command.
    pub action: Option<Action>,
    /// "Can run now". False: its menu row and its button are grey, and its key
    /// does nothing.
    pub can_run: fn(&World) -> bool,
    pub run: Arc<dyn Fn(&mut World) + Send + Sync>,
}

impl ToolCommand {
    /// A command with no menu and no key. Set the other fields with struct update
    /// syntax.
    pub fn new(
        id: &'static str,
        name: &'static str,
        run: impl Fn(&mut World) + Send + Sync + 'static,
    ) -> Self {
        Self {
            id,
            name,
            menu: "",
            action: None,
            can_run: |_| true,
            run: Arc::new(run),
        }
    }

    /// The key of the command in the input mode of now. Empty: no key.
    pub fn key(&self, world: &World) -> String {
        self.action
            .map_or(String::new(), |action| key_label(world, action))
    }

    /// The hover text of a button of the command: its name and its key.
    pub fn hover(&self, world: &World) -> String {
        match self.key(world).as_str() {
            "" => self.name.to_owned(),
            key => format!("{} ({key})", self.name),
        }
    }
}

/// The command list. Cheap to clone: a panel takes a copy and has the world free.
#[derive(Resource, Default, Clone)]
pub struct ToolCommands(Arc<Vec<ToolCommand>>);

impl ToolCommands {
    pub fn iter(&self) -> impl Iterator<Item = &ToolCommand> {
        self.0.iter()
    }

    pub fn get(&self, id: &str) -> Option<&ToolCommand> {
        self.iter().find(|command| command.id == id)
    }
}

/// Adds a command of a tool. Call from `Module::build`.
pub fn add_command(app: &mut App, command: ToolCommand) {
    app.init_resource::<ToolCommands>();
    let mut commands = app.resource_mut::<ToolCommands>();
    assert!(
        commands.get(command.id).is_none(),
        "command `{}` was added two times",
        command.id
    );
    Arc::make_mut(&mut commands.0).push(command);
}

/// The keys of an action in the input mode of now, as the binding table has them.
/// Empty: the action has no key.
pub fn key_label(world: &World, action: Action) -> String {
    let (Some(bindings), Some(mode)) = (
        world.get_resource::<Bindings>(),
        world.get_resource::<InputMode>(),
    ) else {
        return String::new();
    };
    let of_action =
        |binding: &&maki_platform::Binding| binding.mode == *mode && binding.action == action;
    let keys: Vec<String> = bindings
        .0
        .iter()
        .filter(of_action)
        .map(|binding| binding.chord.label())
        .collect();
    keys.join(" or ")
}

/// Runs a command if it can run now. False: it did not run.
pub fn run_command(world: &mut World, id: &str) -> bool {
    let commands = world.resource::<ToolCommands>().clone();
    let Some(command) = commands.get(id).filter(|command| (command.can_run)(world)) else {
        return false;
    };
    (command.run)(world);
    true
}

/// Commands that a menu or the command search started during the egui frame.
#[derive(Resource, Default)]
pub struct QueuedCommands(Vec<&'static str>);

/// Starts a command from inside the egui frame. `run_ui` holds `Panels` out of the world,
/// so the command runs after the frame (`run_queued`), when the panel list is back.
pub fn queue_command(world: &mut World, id: &'static str) {
    world
        .get_resource_or_insert_with(QueuedCommands::default)
        .0
        .push(id);
}

/// Runs in `Update`, after the egui frame: the commands that `queue_command` kept.
pub fn run_queued(world: &mut World) {
    let Some(mut queued) = world.get_resource_mut::<QueuedCommands>() else {
        return;
    };
    for id in std::mem::take(&mut queued.0) {
        run_command(world, id);
    }
}

/// Runs in `Update`, before the egui frame: the key of a command starts it.
pub fn command_keys(world: &mut World) {
    let Some(commands) = world.get_resource::<ToolCommands>().cloned() else {
        return;
    };
    for command in commands.iter() {
        let pressed = command
            .action
            .is_some_and(|action| world.resource::<ActionState>().just_pressed(action));
        if pressed && (command.can_run)(world) {
            (command.run)(world);
        }
    }
}

/// The row of a command in a menu: its name and its key, grey if it cannot run now.
/// A click runs it and closes the menu.
pub fn command_item(ui: &mut egui::Ui, world: &mut World, command: &ToolCommand) {
    let button = egui::Button::new(command.name).shortcut_text(command.key(world));
    if ui.add_enabled((command.can_run)(world), button).clicked() {
        queue_command(world, command.id);
        ui.close();
    }
}

/// True: each word of the query is a part of the text. Capital letters are as small ones.
pub(crate) fn found(query: &str, text: &str) -> bool {
    let text = text.to_lowercase();
    let query = query.to_lowercase();
    query.split_whitespace().all(|word| text.contains(word))
}
