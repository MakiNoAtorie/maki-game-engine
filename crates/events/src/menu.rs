//! Menus: a data asset (`data/menus/<name>.menu.ron`) with a title, a place on the
//! picture, and rows; and the stack of the open menus ([`Menus`]).
//!
//! ```ron
//! (
//!     title: "menu.pause",
//!     place: Center,
//!     rows: [
//!         Commands(text: "menu.back", commands: [CloseMenu]),
//!         Commands(text: "menu.key", conditions: [Flag("has_key", true)], commands: [Message("key")]),
//!     ],
//! )
//! ```
//!
//! The commands of a row are read with the registry, as those of a routine: a menu
//! file is read by [`read_menu`], not by `DataAssets`. The command `OpenMenu` keeps
//! the files that it read in [`MenuFiles`].
//!
//! The input actions go to the top menu with focus: up and down move the focus, over
//! the rows whose conditions do not hold; interact picks the row, and its commands
//! run as an event that the player started; back closes the menu if `back` is on.
//! While a message shows, the message has the input. The UI of the game draws the
//! menus from [`shown_menus`]; it runs no command.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy_ecs::change_detection::{DetectChanges, DetectChangesMut};
use bevy_ecs::entity::Entity;
use bevy_ecs::message::MessageReader;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Local, Res, ResMut};
use bevy_ecs::world::World;
use maki_assets::{AssetChanged, AssetWatch, DataAsset, DataRef};
use maki_core::{App, Fields, PlayState, ResetPlay, Time, Update};
use maki_platform::{ActionState, FixedActionState};
use maki_scene::ObjectId;
use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};

use crate::check::{CheckContext, Finding};
use crate::command::{Command, CommandRegistry, Step};
use crate::interpreter::{RunningEvent, Start};
use crate::items::room_directory;
use crate::model::{Condition, Facts, all_hold};
use crate::text::TextKey;
use crate::{BACK, CHOICE_DOWN, CHOICE_UP, EventRunner, GameState, INTERACT, MessageBox};

/// A menu: what a menu file holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
#[fields(default)]
pub struct Menu {
    /// The key of the title text. None: no title.
    pub title: Option<TextKey>,
    /// Where the menu shows on the picture.
    pub place: MenuPlace,
    /// True: the menu takes the input, and the player does not walk while it is open.
    /// False: a display (a HUD), with no input.
    pub focus: bool,
    /// True: the back action closes the menu.
    pub back: bool,
    /// The rows hold commands, which are not fields: a tool edits them as the rows
    /// of an event.
    #[field(skip)]
    pub rows: Vec<MenuRow>,
}

impl Default for Menu {
    fn default() -> Self {
        Self {
            title: None,
            place: MenuPlace::Center,
            focus: true,
            back: true,
            rows: Vec::new(),
        }
    }
}

impl DataAsset for Menu {
    const NAME: &'static str = "menu";
}

/// The place of a menu on the picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Fields)]
pub enum MenuPlace {
    #[default]
    Center,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// A row of a menu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MenuRow {
    /// A text that shows while all conditions hold. A pick runs the commands.
    Commands {
        text: TextKey,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        conditions: Vec<Condition>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        commands: Vec<Command>,
    },
}

impl MenuRow {
    /// The conditions of the row: it shows and takes the focus while they hold.
    pub fn conditions(&self) -> &[Condition] {
        match self {
            MenuRow::Commands { conditions, .. } => conditions,
        }
    }
}

impl CommandRegistry {
    /// A menu from the text of its file. The error has the line and the column.
    pub fn parse_menu(&self, text: &str) -> Result<Menu, ron::error::SpannedError> {
        self.parse(text, None)
    }
}

/// The text of a menu file.
pub fn menu_text(menu: &Menu) -> String {
    let config = ron::ser::PrettyConfig::default();
    let text = maki_scene::ron_options().to_string_pretty(menu, config);
    text.expect("a menu has no value that RON cannot write")
}

/// Reads a menu file.
pub fn read_menu(path: &Path, registry: &CommandRegistry) -> Result<Menu, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("no menu file {}: {error}", path.display()))?;
    registry
        .parse_menu(&text)
        .map_err(|error| format!("{}:{error}", path.display()))
}

/// The check of the watch: the file reads as a menu, with the rows unread (the watch
/// has no registry).
fn check_file(path: &Path) -> anyhow::Result<()> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct Light {
        #[serde(default)]
        title: Option<TextKey>,
        #[serde(default)]
        place: MenuPlace,
        #[serde(default)]
        focus: Option<bool>,
        #[serde(default)]
        back: Option<bool>,
        #[serde(default)]
        rows: IgnoredAny,
    }
    let text = std::fs::read_to_string(path)?;
    maki_scene::ron_options().from_str::<Light>(&text)?;
    Ok(())
}

/// The menu files that were read, by path. A file that changed on the disk is read
/// again at its next open.
#[derive(Resource, Default)]
pub struct MenuFiles(HashMap<PathBuf, Menu>);

impl MenuFiles {
    /// The menu of a file: from the store, else read now.
    pub fn get(&mut self, path: &Path, registry: &CommandRegistry) -> Result<Menu, String> {
        if let Some(menu) = self.0.get(path) {
            return Ok(menu.clone());
        }
        let menu = read_menu(path, registry)?;
        self.0.insert(path.to_owned(), menu.clone());
        Ok(menu)
    }
}

/// An open menu.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenMenu {
    pub menu: Menu,
    /// The index in `menu.rows` of the row with the focus.
    pub row: usize,
    /// The object of the event or the script that opened the menu: the commands of a
    /// row run with it (`CallScript` needs it). `None`: no object.
    pub object: Option<Entity>,
}

/// The open menus, the last one on top. A resource: a menu stays open when the room
/// changes.
#[derive(Resource, Debug, Clone, PartialEq, Default)]
pub struct Menus(pub Vec<OpenMenu>);

impl Menus {
    /// Opens a menu on the top. `object`: see [`OpenMenu::object`].
    pub fn open(&mut self, menu: Menu, object: Option<Entity>) {
        self.0.push(OpenMenu {
            menu,
            row: 0,
            object,
        });
    }

    /// Closes the top menu, with focus or not.
    pub fn close(&mut self) {
        self.0.pop();
    }

    /// Closes all menus with focus. The menus with no focus (a HUD) stay.
    pub fn close_all(&mut self) {
        self.0.retain(|open| !open.menu.focus);
    }

    /// The index of the top menu with focus: the one that has the input.
    pub fn top_with_focus(&self) -> Option<usize> {
        self.0.iter().rposition(|open| open.menu.focus)
    }

    /// True if a menu with focus is open: the player does not walk.
    pub fn has_focus(&self) -> bool {
        self.top_with_focus().is_some()
    }
}

/// The commands of the menus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
pub enum MenuCommand {
    /// Opens a menu file on the top of the open menus.
    OpenMenu(#[field(label = "menu")] DataRef<Menu>),
    /// Closes the top menu.
    CloseMenu,
    /// Closes all menus with focus. A menu with no focus (a HUD) stays.
    CloseMenus,
}

fn run(command: &MenuCommand, step: &mut Step, world: &mut World) -> bool {
    match command {
        MenuCommand::OpenMenu(file) => {
            let path = file.file.resolve(room_directory(world));
            let registry = world.resource::<CommandRegistry>().clone();
            match world.resource_mut::<MenuFiles>().get(&path, &registry) {
                Ok(menu) => world.resource_mut::<Menus>().open(menu, step.entity),
                Err(error) => tracing::error!(event = step.event, "OpenMenu: {error}"),
            }
        }
        MenuCommand::CloseMenu => world.resource_mut::<Menus>().close(),
        MenuCommand::CloseMenus => world.resource_mut::<Menus>().close_all(),
    }
    true
}

/// The check of `OpenMenu`: the file reads as a menu.
fn check(command: &MenuCommand, context: &CheckContext) -> Option<Finding> {
    let MenuCommand::OpenMenu(file) = command else {
        return None;
    };
    let path = file.file.resolve(context.directory);
    if !path.is_file() {
        return Some(Finding::error(format!(
            "no menu file `{}`",
            file.file.display()
        )));
    }
    read_menu(&path, context.registry).err().map(Finding::error)
}

fn row_text(command: &MenuCommand) -> Option<String> {
    Some(match command {
        MenuCommand::OpenMenu(file) => format!("Open the menu {}", file.file.display()),
        MenuCommand::CloseMenu => "Close the top menu".to_owned(),
        MenuCommand::CloseMenus => "Close the menus".to_owned(),
    })
}

fn help(name: &str) -> Option<&'static str> {
    Some(match name {
        "OpenMenu" => "Opens a menu file on the top of the open menus.",
        "CloseMenu" => "Closes the top menu.",
        "CloseMenus" => "Closes all menus with focus. A HUD stays.",
        _ => return None,
    })
}

/// For each row of a menu: true if its conditions hold. The object of the menu is
/// object 0 of the conditions.
fn rows_that_hold(world: &World, open: &OpenMenu) -> Vec<bool> {
    let object = open.object.and_then(|entity| world.get::<ObjectId>(entity));
    let facts = Facts::new(world, object.copied());
    let rows = open.menu.rows.iter();
    rows.map(|row| all_hold(row.conditions(), &facts)).collect()
}

/// The input of one step to the top menu with focus.
#[derive(Debug, Clone, Copy, Default)]
struct MenuInput {
    up: bool,
    down: bool,
    pick: bool,
    back: bool,
}

impl MenuInput {
    fn of(actions: &ActionState) -> Self {
        Self {
            up: actions.just_pressed(CHOICE_UP),
            down: actions.just_pressed(CHOICE_DOWN),
            pick: actions.just_pressed(INTERACT),
            back: actions.just_pressed(BACK),
        }
    }
}

/// Gives the input to the top menu with focus: moves its focus, closes it, or starts
/// the commands of its row as a running event.
fn take_input(world: &mut World, input: MenuInput) {
    // A message over a menu has the input; so has a row that runs.
    let runner = world.resource::<EventRunner>();
    let picked = runner
        .running()
        .iter()
        .any(|event| event.start == Start::Menu);
    if world.resource::<MessageBox>().0.is_some() || picked {
        return;
    }
    let menus = world.resource::<Menus>();
    let Some(top) = menus.top_with_focus() else {
        return;
    };
    let open = &menus.0[top];
    let holds = rows_that_hold(world, open);
    let mut row = open.row;
    // A row that does not hold has no focus: the first row that holds takes it.
    if !holds.get(row).copied().unwrap_or(false) {
        row = holds.iter().position(|holds| *holds).unwrap_or(0);
    }
    // The focus stops at the first and at the last row that holds.
    if input.down
        && let Some(next) = (row + 1..holds.len()).find(|index| holds[*index])
    {
        row = next;
    }
    if input.up
        && let Some(before) = (0..row).rev().find(|index| holds[*index])
    {
        row = before;
    }
    let back = input.back && open.menu.back;
    let pick = (input.pick && holds.get(row).copied().unwrap_or(false))
        .then(|| (open.menu.rows[row].clone(), open.object));
    let mut menus = world.resource_mut::<Menus>();
    if menus.0[top].row != row {
        menus.0[top].row = row;
    }
    if back {
        menus.0.remove(top);
        return;
    }
    let Some((MenuRow::Commands { commands, .. }, object)) = pick else {
        return;
    };
    // The object can be gone: a room change after the menu opened.
    let object = object.filter(|entity| world.get_entity(*entity).is_ok());
    let mut running = RunningEvent::new("menu", Start::Menu, &commands);
    if let Some(entity) = object {
        running = running.of(world, entity);
    }
    world
        .resource_mut::<EventRunner>()
        .bypass_change_detection()
        .running
        .push(running);
}

/// `run_events` calls it before the events: the menu has the input of the step, and
/// the commands of a pick start in the same step. Not a system of its own: each
/// system of `FixedUpdate` has an order to `run_events`.
pub(crate) fn menu_input(world: &mut World) {
    let input = MenuInput::of(world.resource::<FixedActionState>());
    take_input(world, input);
}

/// Runs in `Update` in a pause, as no fixed step runs then: the input of the frame
/// goes to the menus, and the commands of a pick run here.
fn menu_input_in_pause(world: &mut World, mut tick: Local<Option<u64>>) {
    let fixed_tick = world.resource::<Time>().fixed_tick;
    // A fixed step in this frame (the one that paused the game) had its input.
    let stepped = tick.replace(fixed_tick) != Some(fixed_tick);
    if !stepped {
        let input = MenuInput::of(world.resource::<ActionState>());
        take_input(world, input);
        crate::interpreter::run_menu_events(world);
    }
    // A press in the pause is not a press of the first step after it.
    world.resource_mut::<FixedActionState>().forget_changes();
}

fn paused(state: Res<PlayState>) -> bool {
    *state == PlayState::Pause
}

/// A shown menu, for the UI of the game: the texts in the language of the game.
#[derive(Debug, Clone, PartialEq)]
pub struct ShownMenu {
    pub place: MenuPlace,
    pub title: Option<String>,
    /// The rows whose conditions hold, in order.
    pub rows: Vec<ShownRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShownRow {
    pub text: String,
    /// True: the row has the focus of the menu that has the input.
    pub focus: bool,
}

/// The open menus as the player sees them, the bottom one first. With the
/// `{name}` of the game variables in the texts.
pub fn shown_menus(world: &mut World) -> Vec<ShownMenu> {
    let menus = world.resource::<Menus>().clone();
    let top = menus.top_with_focus();
    let variables = world.resource::<GameState>().variables.clone();
    let variable = |name: &str| variables.get(name).copied().unwrap_or(0);
    let mut shown = Vec::new();
    for (index, open) in menus.0.iter().enumerate() {
        let holds = rows_that_hold(world, open);
        let title = open.menu.title.as_ref();
        let title = title.map(|key| crate::text(world, &key.0, variable));
        let mut rows = Vec::new();
        for (row_index, row) in open.menu.rows.iter().enumerate() {
            let MenuRow::Commands { text, .. } = row;
            if holds[row_index] {
                rows.push(ShownRow {
                    text: crate::text(world, &text.0, variable),
                    focus: Some(index) == top && row_index == open.row,
                });
            }
        }
        shown.push(ShownMenu {
            place: open.menu.place,
            title,
            rows,
        });
    }
    shown
}

/// Runs in `Update`: a menu file that changed is read again at its next open.
fn forget_changed(mut changed: MessageReader<AssetChanged>, mut files: ResMut<MenuFiles>) {
    for AssetChanged(path) in changed.read() {
        if files.0.contains_key(path) {
            files.0.remove(path);
        }
    }
}

/// Runs in `Update`: tells the watch the menu files that were read.
fn watch_menus(files: Res<MenuFiles>, watch: Option<ResMut<AssetWatch>>) {
    let Some(mut watch) = watch.filter(|watch| watch.on) else {
        return;
    };
    if files.is_changed() {
        let check: maki_assets::Check = check_file;
        let paths = files.0.keys().map(|path| (path.clone(), check));
        watch.set("menus", paths.collect());
    }
}

/// Adds the menus: the commands, the stack, the input, and the watch of the files.
pub(crate) fn add(app: &mut App, registry: &CommandRegistry) {
    registry.add(run);
    registry.add_fields::<MenuCommand>();
    registry.add_check(check);
    registry.add_texts(row_text, help);
    registry.add_defaults(|name, _| {
        Some(match name {
            "OpenMenu" => MenuCommand::OpenMenu(DataRef::default()),
            "CloseMenu" => MenuCommand::CloseMenu,
            "CloseMenus" => MenuCommand::CloseMenus,
            _ => return None,
        })
    });
    app.init_resource::<Menus>()
        .init_resource::<MenuFiles>()
        // Any order in the frame: in a pause no other system of the events runs.
        .add_systems(
            Update,
            menu_input_in_pause.run_if(paused).ambiguous_with_all(),
        )
        .add_systems(Update, (forget_changed, watch_menus).chain())
        .add_systems(ResetPlay, |mut menus: ResMut<Menus>| {
            menus.set_if_neq(Menus::default());
        });
}
