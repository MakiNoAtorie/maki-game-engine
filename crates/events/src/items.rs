//! Item kinds and the inventory. An item kind is a data asset (`<name>.item.ron` in
//! `<rooms>/data/`); the inventory has a count for each kind, by the id of its file,
//! so a new name or a move of the file keeps the count.

use std::collections::BTreeMap;
use std::path::Path;

use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::ResMut;
use bevy_ecs::world::World;
use maki_assets::{DataAsset, DataRef, FileId, FileRef, add_data_type, read_data};
use maki_core::{App, Fields, ResetPlay};
use maki_scene::CurrentRoom;
use serde::{Deserialize, Serialize};

use crate::CommandRegistry;
use crate::check::Finding;
use crate::command::Step;
use crate::text::TextKey;

/// A kind of item: a key, a coin.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
pub struct ItemKind {
    /// The keys of the texts in the text tables.
    pub name: TextKey,
    pub description: TextKey,
    #[field(file = "picture")]
    pub icon: FileRef,
}

impl DataAsset for ItemKind {
    const NAME: &'static str = "item";
}

/// The count of each item kind that the player has, by the id of the item file. A
/// resource: it stays when the room changes, and the save has it.
#[derive(Resource, Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Inventory(pub BTreeMap<FileId, i32>);

impl Inventory {
    /// The count of an item. An item that was never given: 0.
    pub fn count(&self, item: FileId) -> i32 {
        self.0.get(&item).copied().unwrap_or(0)
    }

    /// The count of an item by its file (from the directory of the room file). A
    /// file with no id: 0.
    pub fn count_of(&self, file: &FileRef, directory: &Path) -> i32 {
        item_id(file, directory).map_or(0, |item| self.count(item))
    }

    /// Adds to the count of an item (a negative number takes). The count does not go
    /// below 0.
    pub fn add(&mut self, item: FileId, count: i32) {
        let count = self.count(item).saturating_add(count).max(0);
        match count {
            0 => self.0.remove(&item),
            _ => self.0.insert(item, count),
        };
    }
}

/// The id of an item file. The id in the reference first: a condition is tested at
/// each step, and this reads no file then.
pub fn item_id(file: &FileRef, directory: &Path) -> Option<FileId> {
    file.id.or_else(|| FileId::read(&file.resolve(directory)))
}

/// The commands of the inventory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
pub enum ItemCommand {
    /// Gives the player some of an item.
    GiveItem(
        #[field(label = "item")] DataRef<ItemKind>,
        #[field(label = "count")] i32,
    ),
    /// Takes some of an item from the player. The count does not go below 0.
    TakeItem(
        #[field(label = "item")] DataRef<ItemKind>,
        #[field(label = "count")] i32,
    ),
}

pub(crate) fn add(app: &mut App, registry: &CommandRegistry) {
    add_data_type::<ItemKind>(app);
    app.init_resource::<Inventory>()
        .add_systems(ResetPlay, |mut inventory: ResMut<Inventory>| {
            inventory.set_if_neq(Inventory::default());
        });
    registry.add(run);
    registry.add_texts(row_text, help);
    registry.add_fields::<ItemCommand>();
    registry.add_defaults(|name, _| {
        Some(match name {
            "GiveItem" => ItemCommand::GiveItem(DataRef::default(), 1),
            "TakeItem" => ItemCommand::TakeItem(DataRef::default(), 1),
            _ => return None,
        })
    });
    registry.add_check(|command: &ItemCommand, context| {
        item_problem(&item_of(command).file, context.directory).map(Finding::error)
    });
}

/// What is wrong with a reference to an item file: no file, not an item, no id.
pub(crate) fn item_problem(file: &FileRef, directory: &Path) -> Option<String> {
    let path = file.resolve(directory);
    let name = file.display();
    if !path.is_file() {
        return Some(format!("no item file `{name}`"));
    }
    if let Err(error) = read_data::<ItemKind>(&path) {
        return Some(error);
    }
    FileId::read(&path)
        .is_none()
        .then(|| format!("the item file `{name}` has no id: the inventory names an item by its id"))
}

fn item_of(command: &ItemCommand) -> &DataRef<ItemKind> {
    let (ItemCommand::GiveItem(item, _) | ItemCommand::TakeItem(item, _)) = command;
    item
}

/// The directory of the room file: a file of a command starts there.
pub(crate) fn room_directory(world: &World) -> &Path {
    let room = world.get_resource::<CurrentRoom>();
    let directory = room.and_then(|room| room.path.parent());
    directory.unwrap_or(Path::new(""))
}

fn run(command: &ItemCommand, step: &mut Step, world: &mut World) -> bool {
    let item = item_of(command);
    let Some(id) = item_id(&item.file, room_directory(world)) else {
        let file = item.file.display();
        tracing::error!(
            event = step.event,
            "{command:?}: the item file `{file}` has no id"
        );
        return true;
    };
    let mut inventory = world.resource_mut::<Inventory>();
    match command {
        ItemCommand::GiveItem(_, count) => inventory.add(id, *count),
        ItemCommand::TakeItem(_, count) => inventory.add(id, count.saturating_neg()),
    }
    true
}

fn row_text(command: &ItemCommand) -> Option<String> {
    Some(match command {
        ItemCommand::GiveItem(item, count) => format!("Give {count} {}", item.file.display()),
        ItemCommand::TakeItem(item, count) => format!("Take {count} {}", item.file.display()),
    })
}

fn help(name: &str) -> Option<&'static str> {
    Some(match name {
        "GiveItem" => "Gives the player some of an item.",
        "TakeItem" => "Takes some of an item from the player. The count does not go below 0.",
        _ => return None,
    })
}
