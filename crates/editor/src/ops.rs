//! Changes of the document that are more than one value: add, delete, and duplicate
//! an item, names and the events that use them, and the history for undo and redo.
//!
//! Names: events find a light, a door, and a mover by name, and a `Transfer` names a
//! spawn point. So a new or changed name is made unique in its group (lights; doors
//! and movers together, because `OpenDoor` finds the two; events), and a change of a
//! name changes the built-in commands of this room that use it. A command of another
//! module, and a `Transfer` in another room file, are not changed: see
//! [`spawn_users`].

use std::path::{Path, PathBuf};

use glam::Vec3;
use maki_assets::{Doorway, Wall};
use maki_audio::AudioCommand;
use maki_events::{Area, Builtin, Command, CommandRegistry, CommandSet, EventDef, Page, Trigger};
use maki_scene::{DoorDef, LightDef, MeshDef, MoverDef, PoseDef, PropDef, RoomDef};

use crate::ron_text::Notes;
use crate::{Document, Item};

/// A thing that "Add" makes.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Cube,
    Cuboid,
    Sphere,
    /// A glTF file, from the directory of the room file.
    Model(PathBuf),
    Light,
    Spawn,
    Doorway,
    /// In the first doorway that has no door.
    Door,
    Mover,
    /// An event with an area on the floor and one empty page.
    Event,
}

impl Kind {
    /// The kinds with no file, with the text of the menu.
    pub const MENU: [(&'static str, Kind); 9] = [
        ("Cube", Kind::Cube),
        ("Cuboid", Kind::Cuboid),
        ("Sphere", Kind::Sphere),
        ("Light", Kind::Light),
        ("Spawn point", Kind::Spawn),
        ("Doorway", Kind::Doorway),
        ("Door", Kind::Door),
        ("Mover", Kind::Mover),
        ("Event area", Kind::Event),
    ];
}

/// The offset of a duplicate from its source, in meters along X and Z.
const DUPLICATE_OFFSET: f32 = 0.5;

/// `name`, or `name_2`, `name_3`, ... : the first that `taken` does not have.
fn free_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(name) {
        return name.to_owned();
    }
    // The number of a name that has one goes up: `lamp_2` gives `lamp_3`.
    let stem = match name.rsplit_once('_') {
        Some((stem, number)) if number.parse::<u32>().is_ok() => stem,
        _ => name,
    };
    let mut names = (2..).map(|number| format!("{stem}_{number}"));
    names
        .find(|name| !taken(name))
        .expect("the range has no end")
}

fn in_floor(position: Vec3) -> [f32; 3] {
    [position.x, 0.0, position.z]
}

impl Document {
    /// True if the document has the item.
    pub fn has(&self, item: &Item) -> bool {
        let room = &self.room;
        match item {
            Item::Room => true,
            Item::Doorway(index) => *index < room.doorways.len(),
            Item::Door(index) => *index < room.doors.len(),
            Item::Prop(index) => *index < room.props.len(),
            Item::Mover(index) => *index < room.movers.len(),
            Item::Light(index) => *index < room.lights.len(),
            Item::Spawn(name) => room.spawns.contains_key(name),
            Item::Event(index) => *index < self.events.len(),
        }
    }

    /// The names of the group of an item, without the name of the item itself.
    /// `None`: items of this kind have no name that must be unique.
    fn other_names(&self, item: &Item) -> Option<Vec<&str>> {
        let room = &self.room;
        fn named<'a>(names: Vec<(Item, &'a Option<String>)>, item: &Item) -> Vec<&'a str> {
            let others = names.into_iter().filter(|(other, _)| other != item);
            others.filter_map(|(_, name)| name.as_deref()).collect()
        }
        let lights = room.lights.iter().enumerate();
        let doors = room.doors.iter().enumerate();
        let movers = room.movers.iter().enumerate();
        Some(match item {
            Item::Prop(_) => {
                let props = room.props.iter().enumerate();
                let props = props.map(|(i, prop)| (Item::Prop(i), &prop.name));
                named(props.collect(), item)
            }
            Item::Light(_) => {
                let lights = lights.map(|(i, light)| (Item::Light(i), &light.name));
                named(lights.collect(), item)
            }
            Item::Door(_) | Item::Mover(_) => {
                let doors = doors.map(|(i, door)| (Item::Door(i), &door.name));
                let movers = movers.map(|(i, mover)| (Item::Mover(i), &mover.name));
                named(doors.chain(movers).collect(), item)
            }
            Item::Event(index) => {
                let events = self.events.iter().enumerate();
                let others = events.filter(|(other, _)| other != index);
                others.map(|(_, event)| event.name.as_str()).collect()
            }
            Item::Spawn(name) => {
                let names = room.spawns.keys().map(String::as_str);
                names.filter(|other| other != name).collect()
            }
            Item::Room | Item::Doorway(_) => return None,
        })
    }

    /// A name for an item that no other item of its group has: `name`, or `name`
    /// with a number.
    pub fn unique_name(&self, item: &Item, name: &str) -> String {
        match self.other_names(item) {
            Some(others) => free_name(name, |name| others.contains(&name)),
            None => name.to_owned(),
        }
    }

    /// Call after the name of a light, a door, a mover, or an event was changed
    /// from `old`. The new name is made unique, and the commands of the events of
    /// this room that name the item (built-in, and of the audio module) get the new
    /// name. Returns a text for
    /// the user if something more than the name itself was changed.
    pub fn renamed(
        &mut self,
        item: &Item,
        old: Option<String>,
        registry: &CommandRegistry,
    ) -> String {
        let name = match item {
            Item::Light(index) => &mut self.room.lights[*index].name,
            Item::Door(index) => &mut self.room.doors[*index].name,
            Item::Mover(index) => &mut self.room.movers[*index].name,
            Item::Event(index) => {
                let typed = std::mem::take(&mut self.events[*index].name);
                self.events[*index].name = self.unique_name(item, &typed);
                let changed = self.events[*index].name != typed;
                return if changed {
                    "The name was in use: a number was added".to_owned()
                } else {
                    String::new()
                };
            }
            _ => return String::new(),
        };
        let Some(typed) = name.take() else {
            // Events that still name the item give a warning when they run.
            return String::new();
        };
        let new = self.unique_name(item, &typed);
        let mut status = String::new();
        if new != typed {
            status = "The name was in use: a number was added. ".to_owned();
        }
        let users = match (&old, item) {
            (Some(old), Item::Light(_)) => {
                let light = self.rename_in_events(registry, |command: &Builtin| match command {
                    Builtin::LightOn(name) if name == old => Some(Builtin::LightOn(new.clone())),
                    Builtin::LightOff(name) if name == old => Some(Builtin::LightOff(new.clone())),
                    Builtin::LightIntensity(name, value) if name == old => {
                        Some(Builtin::LightIntensity(new.clone(), *value))
                    }
                    _ => None,
                });
                // The sound of the light has the name of the light.
                let sound = |command: &AudioCommand| match command {
                    AudioCommand::SoundOn(name) if name == old => {
                        Some(AudioCommand::SoundOn(new.clone()))
                    }
                    AudioCommand::SoundOff(name) if name == old => {
                        Some(AudioCommand::SoundOff(new.clone()))
                    }
                    _ => None,
                };
                light + self.rename_in_events(registry, sound)
            }
            (Some(old), _) => self.rename_in_events(registry, |command: &Builtin| match command {
                Builtin::OpenDoor(name) if name == old => Some(Builtin::OpenDoor(new.clone())),
                Builtin::CloseDoor(name) if name == old => Some(Builtin::CloseDoor(new.clone())),
                _ => None,
            }),
            (None, _) => 0,
        };
        if users > 0 {
            status += &format!("{users} event commands now name `{new}`");
        }
        let name = match item {
            Item::Light(index) => &mut self.room.lights[*index].name,
            Item::Door(index) => &mut self.room.doors[*index].name,
            _ => &mut self.room.movers[item_index(item)].name,
        };
        *name = Some(new);
        status
    }

    /// Replaces each command of the enum `T` in the events for which `change` gives
    /// a new one. Returns the number of the commands that were replaced.
    pub(crate) fn rename_in_events<T: CommandSet>(
        &mut self,
        registry: &CommandRegistry,
        change: impl Fn(&T) -> Option<T>,
    ) -> usize {
        fn walk<T: CommandSet>(
            commands: &mut [Command],
            registry: &CommandRegistry,
            change: &dyn Fn(&T) -> Option<T>,
        ) -> usize {
            let mut count = 0;
            for command in commands {
                // The lists of a command hold commands too.
                let lists = registry.lists(command);
                let mut lists: Vec<Vec<Command>> =
                    lists.into_iter().map(|list| list.1.to_vec()).collect();
                let inner: usize = lists
                    .iter_mut()
                    .map(|list| walk(list, registry, change))
                    .sum();
                count += inner;
                let new = if inner > 0 {
                    registry.with_lists(command, lists)
                } else {
                    let new = command.get::<T>().and_then(change);
                    new.inspect(|_| count += 1)
                        .and_then(|new| registry.command(new))
                };
                if let Some(new) = new {
                    *command = new;
                }
            }
            count
        }
        let pages = self.events.iter_mut().flat_map(|event| &mut event.pages);
        pages
            .map(|page| walk(&mut page.commands, registry, &change))
            .sum()
    }

    /// Adds an item at a point of the floor. Returns the new item. `Err`: why not.
    pub fn add(&mut self, kind: Kind, at: Vec3) -> Result<Item, String> {
        let room = &mut self.room;
        let on_floor = |height: f32| PoseDef {
            position: [at.x, height, at.z],
            ..PoseDef::default()
        };
        let prop = |mesh: MeshDef, pose: PoseDef| PropDef {
            mesh,
            pose,
            ..PropDef::default()
        };
        let item = match kind {
            Kind::Cube | Kind::Cuboid | Kind::Sphere | Kind::Model(_) => {
                room.props.push(match kind {
                    Kind::Cube => prop(MeshDef::Cube(1.0), on_floor(0.5)),
                    Kind::Cuboid => prop(
                        MeshDef::Cuboid([-0.5, 0.0, -0.5], [0.5, 1.0, 0.5]),
                        on_floor(0.0),
                    ),
                    Kind::Sphere => {
                        let (radius, segments, rings) = (0.5, 32, 16);
                        prop(
                            MeshDef::Sphere {
                                radius,
                                segments,
                                rings,
                            },
                            on_floor(0.5),
                        )
                    }
                    Kind::Model(path) => prop(MeshDef::File(path), on_floor(0.0)),
                    _ => unreachable!("the outer arm has the four kinds of a prop"),
                });
                // A name of its own: the outliner lists the prop by it, and a
                // label by the index would change with each delete before it.
                let item = Item::Prop(room.props.len() - 1);
                let kind = match &self.room.props.last().expect("pushed above").mesh {
                    MeshDef::Cube(_) => "cube".to_owned(),
                    MeshDef::Cuboid(..) => "cuboid".to_owned(),
                    MeshDef::Sphere { .. } => "sphere".to_owned(),
                    MeshDef::File(path) => {
                        let stem = path.file_stem().unwrap_or_default();
                        stem.to_string_lossy().into_owned()
                    }
                };
                let name = self.unique_name(&item, &kind);
                self.room.props.last_mut().expect("pushed above").name = Some(name);
                item
            }
            Kind::Light => {
                let item = Item::Light(room.lights.len());
                room.lights.push(LightDef {
                    name: None,
                    position: [at.x, (room.height - 0.6).max(0.5), at.z],
                    radius: 0.15,
                    color: [1.0, 0.9, 0.8],
                    intensity: 8.0,
                    on: true,
                    direction: None,
                    cone: None,
                    sound: None,
                });
                let name = self.unique_name(&item, "light");
                self.room.lights.last_mut().expect("pushed above").name = Some(name);
                item
            }
            Kind::Spawn => {
                let name = free_name("spawn", |name| room.spawns.contains_key(name));
                room.spawns.insert(name.clone(), in_floor(at));
                let item = Item::Spawn(name);
                // The spawn points are in name order: the notes of those behind it move.
                let (list, index) = self.notes_key(&item).expect("put in above");
                self.notes.shift(list, index, 1);
                item
            }
            Kind::Doorway => {
                room.doorways.push(Doorway {
                    wall: Wall::NegZ,
                    offset: at.x.clamp(-room.width / 2.0 + 1.0, room.width / 2.0 - 1.0),
                    width: 1.0,
                    height: 2.1,
                    depth: 1.0,
                });
                Item::Doorway(room.doorways.len() - 1)
            }
            Kind::Door => {
                let used = |doorway: &usize| room.doors.iter().any(|door| door.doorway == *doorway);
                let Some(doorway) = (0..room.doorways.len()).find(|doorway| !used(doorway)) else {
                    return Err("Each doorway has a door: add a doorway first".to_owned());
                };
                let item = Item::Door(room.doors.len());
                room.doors.push(DoorDef {
                    name: None,
                    doorway,
                    seconds: 1.2,
                    color: [0.35, 0.2, 0.1],
                    open: false,
                });
                let name = self.unique_name(&item, "door");
                self.room.doors.last_mut().expect("pushed above").name = Some(name);
                item
            }
            Kind::Mover => {
                let item = Item::Mover(room.movers.len());
                let (a, mut b) = (on_floor(0.5), on_floor(0.5));
                b.position[1] += 1.0;
                room.movers.push(MoverDef {
                    name: None,
                    mesh: MeshDef::Cube(1.0),
                    look: Default::default(),
                    a,
                    b,
                    seconds: 2.0,
                });
                let name = self.unique_name(&item, "mover");
                self.room.movers.last_mut().expect("pushed above").name = Some(name);
                item
            }
            Kind::Event => {
                let item = Item::Event(self.events.len());
                self.events.push(EventDef {
                    name: String::new(),
                    area: Some(Area {
                        center: [at.x, at.z],
                        size: [1.0, 1.0],
                    }),
                    pages: vec![Page {
                        conditions: Vec::new(),
                        trigger: Trigger::Interact,
                        commands: Vec::new(),
                    }],
                });
                let name = self.unique_name(&item, "event");
                self.events.last_mut().expect("pushed above").name = name;
                item
            }
        };
        self.dirty = true;
        Ok(item)
    }

    /// Removes items. The room itself is not removed. A doorway takes its door with
    /// it, and the doors of the doorways behind it get their new index. Returns a
    /// text for the user if more than the items was changed.
    pub fn delete(&mut self, items: &[Item]) -> String {
        // From the last index down: a removal does not move an item that is still
        // to remove.
        let mut items: Vec<&Item> = items.iter().filter(|item| self.has(item)).collect();
        items.sort_by_key(|item| std::cmp::Reverse(item_index(item)));
        let mut doors_gone = 0;
        for item in items {
            if let Some((list, index)) = self.notes_key(item) {
                self.notes.shift(list, index, -1);
            }
            let room = &mut self.room;
            match item {
                Item::Room => continue,
                Item::Doorway(index) => {
                    room.doorways.remove(*index);
                    let mut kept = 0;
                    room.doors.retain_mut(|def| {
                        let keep = def.doorway != *index;
                        if keep {
                            def.doorway -= usize::from(def.doorway > *index);
                            kept += 1;
                        } else {
                            // The notes of the doors behind it move up.
                            self.notes.shift("doors", kept, -1);
                            doors_gone += 1;
                        }
                        keep
                    });
                }
                Item::Door(index) => drop(room.doors.remove(*index)),
                Item::Prop(index) => drop(room.props.remove(*index)),
                Item::Mover(index) => drop(room.movers.remove(*index)),
                Item::Light(index) => drop(room.lights.remove(*index)),
                Item::Spawn(name) => drop(room.spawns.remove(name)),
                Item::Event(index) => drop(self.events.remove(*index)),
            }
            self.dirty = true;
        }
        match doors_gone {
            0 => String::new(),
            count => format!("{count} door of a removed doorway was removed too"),
        }
    }

    /// Makes a copy of each item, a little beside it, with a name of its own.
    /// Returns the copies. The room and a door have no copy (a doorway holds one door).
    pub fn duplicate(&mut self, items: &[Item]) -> Vec<Item> {
        let shift = |position: &mut [f32; 3]| {
            position[0] += DUPLICATE_OFFSET;
            position[2] += DUPLICATE_OFFSET;
        };
        let mut copies = Vec::new();
        for item in items {
            if !self.has(item) {
                continue;
            }
            let notes = self.notes_of(item).to_vec();
            let room = &mut self.room;
            let copy = match item {
                Item::Room | Item::Door(_) => continue,
                Item::Doorway(index) => {
                    let mut doorway = room.doorways[*index];
                    doorway.offset += doorway.width + DUPLICATE_OFFSET;
                    room.doorways.push(doorway);
                    Item::Doorway(room.doorways.len() - 1)
                }
                Item::Prop(index) => {
                    let mut prop = room.props[*index].clone();
                    shift(&mut prop.pose.position);
                    room.props.push(prop);
                    Item::Prop(room.props.len() - 1)
                }
                Item::Mover(index) => {
                    let mut mover = room.movers[*index].clone();
                    shift(&mut mover.a.position);
                    shift(&mut mover.b.position);
                    let copy = Item::Mover(room.movers.len());
                    room.movers.push(mover);
                    let name = self.room.movers[*index].name.clone();
                    let name = name.map(|name| self.unique_name(&copy, &name));
                    self.room.movers.last_mut().expect("pushed above").name = name;
                    copy
                }
                Item::Light(index) => {
                    let mut light = room.lights[*index].clone();
                    shift(&mut light.position);
                    let copy = Item::Light(room.lights.len());
                    room.lights.push(light);
                    let name = self.room.lights[*index].name.clone();
                    let name = name.map(|name| self.unique_name(&copy, &name));
                    self.room.lights.last_mut().expect("pushed above").name = name;
                    copy
                }
                Item::Spawn(name) => {
                    let mut position = room.spawns[name];
                    shift(&mut position);
                    let new = free_name(name, |name| room.spawns.contains_key(name));
                    room.spawns.insert(new.clone(), position);
                    let copy = Item::Spawn(new);
                    let (list, at) = self.notes_key(&copy).expect("put in above");
                    self.notes.shift(list, at, 1);
                    copy
                }
                Item::Event(index) => {
                    let mut event = self.events[*index].clone();
                    if let Some(area) = &mut event.area {
                        area.center[0] += DUPLICATE_OFFSET;
                        area.center[1] += DUPLICATE_OFFSET;
                    }
                    let copy = Item::Event(self.events.len());
                    event.name = String::new();
                    self.events.push(event);
                    let name = self.unique_name(&copy, &self.events[*index].name);
                    self.events.last_mut().expect("pushed above").name = name;
                    copy
                }
            };
            self.set_notes(&copy, notes);
            copies.push(copy);
            self.dirty = true;
        }
        copies
    }

    /// What undo and redo keep: all of the document but the path.
    fn content(&self) -> Content {
        (self.room.clone(), self.events.clone(), self.notes.clone())
    }

    fn is(&self, content: &Content) -> bool {
        self.room == content.0 && self.events == content.1 && self.notes == content.2
    }

    fn set_content(&mut self, content: Content) {
        (self.room, self.events, self.notes) = content;
        self.dirty = true;
    }
}

/// The index of an item in its list. 0 for the room and for a spawn point.
fn item_index(item: &Item) -> usize {
    match item {
        Item::Room | Item::Spawn(_) => 0,
        Item::Doorway(index)
        | Item::Door(index)
        | Item::Prop(index)
        | Item::Mover(index)
        | Item::Light(index)
        | Item::Event(index) => *index,
    }
}

type Content = (RoomDef, Vec<EventDef>, Notes);
/// A state of the document, and the items that were selected in it.
type Step = (Content, Vec<Item>);

/// The steps of undo and redo. A step is a copy of the content of the document: a
/// room file is small, and a copy cannot forget a field that a change touched.
#[derive(Default)]
pub struct History {
    /// The document after the last step.
    base: Option<Step>,
    undo: Vec<Step>,
    redo: Vec<Step>,
}

/// Older steps are dropped.
const MAX_STEPS: usize = 200;

impl History {
    /// A document was opened: no step.
    pub fn reset(&mut self, document: &Document) {
        *self = Self {
            base: Some((document.content(), Vec::new())),
            ..Self::default()
        };
    }

    /// Call in each frame. A document that differs from the last step is a new step.
    /// `busy`: a change goes on (a drag, a text that is typed): no step now, so all
    /// of the change is one step.
    /// `selection`: the selected items. Undo and redo put back those of the state
    /// that they go to (undo of a delete selects the item again).
    pub fn track(&mut self, document: &Document, selection: &[Item], busy: bool) {
        let Some(base) = &mut self.base else {
            return self.reset(document);
        };
        if busy {
            return;
        }
        if document.is(&base.0) {
            // The same state: its selection is the last one that it had.
            if base.1 != selection {
                base.1 = selection.to_vec();
            }
            return;
        }
        let new = (document.content(), selection.to_vec());
        self.undo.push(std::mem::replace(base, new));
        if self.undo.len() > MAX_STEPS {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// The document goes one step back. False: no step to undo.
    pub fn undo(&mut self, document: &mut Document, selection: &mut Vec<Item>) -> bool {
        self.track(document, selection, false);
        Self::step(
            &mut self.base,
            &mut self.undo,
            &mut self.redo,
            document,
            selection,
        )
    }

    pub fn redo(&mut self, document: &mut Document, selection: &mut Vec<Item>) -> bool {
        self.track(document, selection, false);
        Self::step(
            &mut self.base,
            &mut self.redo,
            &mut self.undo,
            document,
            selection,
        )
    }

    fn step(
        base: &mut Option<Step>,
        from: &mut Vec<Step>,
        to: &mut Vec<Step>,
        document: &mut Document,
        selection: &mut Vec<Item>,
    ) -> bool {
        let Some(step) = from.pop() else {
            return false;
        };
        to.extend(base.replace(step.clone()));
        document.set_content(step.0);
        *selection = step.1;
        true
    }

    /// The numbers of steps that undo and redo can do.
    pub fn steps(&self) -> (usize, usize) {
        (self.undo.len(), self.redo.len())
    }
}

/// The room files of a directory with a `Transfer` to a spawn point of a room file:
/// their names. The editor does not change other room files: a rename of a spawn
/// point that this finds users of gets a warning with the files.
pub fn spawn_users(
    directory: &Path,
    room: &Path,
    spawn: &str,
    registry: &CommandRegistry,
) -> Vec<String> {
    let names = |commands: &[Command], target: (&str, &str)| -> bool {
        let mut found = false;
        registry.each_command(commands, &mut |command| {
            if let Some(Builtin::Transfer(room, spawn)) = command.get() {
                found |= (room.as_str(), spawn.as_str()) == target;
            }
        });
        found
    };
    let Some(room_name) = room.file_name().and_then(|name| name.to_str()) else {
        return Vec::new();
    };
    let files = std::fs::read_dir(directory).into_iter().flatten().flatten();
    let mut users = Vec::new();
    for path in files.map(|entry| entry.path()) {
        let other = path.file_name() != room.file_name();
        let events = std::fs::read_to_string(&path)
            .ok()
            .filter(|_| other && path.extension().is_some_and(|extension| extension == "ron"))
            .and_then(|text| registry.parse_events(&text).ok());
        let pages = events.iter().flatten().flat_map(|event| &event.pages);
        if pages
            .into_iter()
            .any(|page| names(&page.commands, (room_name, spawn)))
        {
            users.push(
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    users.sort();
    users
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> CommandRegistry {
        // The built-in commands, as `EventsModule` adds them.
        let mut app = maki_core::App::new(maki_core::ConfigFile::default()).unwrap();
        app.add_module(maki_platform::PlatformModule)
            .add_module(maki_scene::SceneModule)
            .add_module(maki_events::EventsModule);
        app.world().resource::<CommandRegistry>().clone()
    }

    fn document(registry: &CommandRegistry) -> Document {
        let text = r#"(
            width: 8.0, depth: 8.0, height: 3.0,
            doorways: [
                (wall: NegZ, offset: 0.0, width: 1.0, height: 2.0, depth: 1.0),
                // The east one.
                (wall: PosX, offset: 0.0, width: 1.0, height: 2.0, depth: 1.0),
            ],
            doors: [
                (name: "door", doorway: 0),
                // To the east.
                (name: "east", doorway: 1),
            ],
            lights: [(name: "lamp", position: (0.0, 2.0, 0.0), radius: 0.1, color: (1.0, 1.0, 1.0), intensity: 5.0)],
            spawns: {"start": (0.0, 0.0, 0.0)},
            events: [(name: "switch", pages: [(trigger: Interact, commands: [
                LightOff("lamp"),
                If([Flag("open")], [OpenDoor("east"), LightOn("lamp")], [CloseDoor("door")]),
            ])])],
        )"#;
        Document::parse(Path::new("room.ron"), text, registry).unwrap()
    }

    #[test]
    fn a_rename_changes_the_events_and_a_name_stays_unique() {
        let registry = registry();
        let mut document = document(&registry);
        let lamp = Item::Light(0);

        document.room.lights[0].name = Some("bulb".to_owned());
        let status = document.renamed(&lamp, Some("lamp".to_owned()), &registry);
        assert!(status.contains("2 event commands"), "{status}");
        let text = document.text();
        assert!(text.contains(r#"LightOff("bulb")"#) && text.contains(r#"LightOn("bulb")"#));
        assert!(!text.contains("\"lamp\""), "{text}");

        // A door with the name of the other door gets a number, and the events
        // follow the door that was renamed.
        document.room.doors[1].name = Some("door".to_owned());
        document.renamed(&Item::Door(1), Some("east".to_owned()), &registry);
        assert_eq!(document.room.doors[1].name.as_deref(), Some("door_2"));
        let text = document.text();
        assert!(text.contains(r#"OpenDoor("door_2")"#) && text.contains(r#"CloseDoor("door")"#));

        // A new light and a copy of a light have names of their own.
        let Ok(Item::Light(new)) = document.add(Kind::Light, Vec3::ZERO) else {
            panic!("a light");
        };
        assert_eq!(document.room.lights[new].name.as_deref(), Some("light"));
        let copies = document.duplicate(&[lamp, Item::Light(new)]);
        assert_eq!(copies, [Item::Light(2), Item::Light(3)]);
        let name = |index: usize| document.room.lights[index].name.clone().unwrap();
        assert_eq!(
            (name(2), name(3)),
            ("bulb_2".to_owned(), "light_2".to_owned())
        );
        assert_eq!(free_name("bulb_2", |name| name == "bulb_2"), "bulb_3");
    }

    #[test]
    fn a_removed_doorway_takes_its_door_and_the_other_doors_keep_their_doorway() {
        let registry = registry();
        let mut document = document(&registry);
        let east = document.room.doorways[1];
        assert_eq!(document.notes.of("doors", 1), ["To the east."]);

        let status = document.delete(&[Item::Doorway(0)]);
        assert!(status.contains("1 door"), "{status}");
        assert_eq!(document.room.doorways, [east]);
        // The door of the east doorway is the one door, in doorway 0 now, with its
        // notes; the notes of the doorway moved too.
        assert_eq!(document.room.doors.len(), 1);
        assert_eq!(document.room.doors[0].name.as_deref(), Some("east"));
        assert_eq!(document.room.doors[0].doorway, 0);
        assert_eq!(document.notes.of("doors", 0), ["To the east."]);
        assert_eq!(document.notes.of("doorways", 0), ["The east one."]);
        assert!(maki_scene::parse_room(&document.text()).is_ok());

        // A door goes into the first doorway with no door; with none: an error.
        assert!(document.add(Kind::Door, Vec3::ZERO).is_err());
        let Ok(doorway) = document.add(Kind::Doorway, Vec3::ZERO) else {
            panic!("a doorway");
        };
        assert_eq!(doorway, Item::Doorway(1));
        assert_eq!(document.add(Kind::Door, Vec3::ZERO), Ok(Item::Door(1)));
        assert_eq!(document.room.doors[1].doorway, 1);
        assert_eq!(document.room.doors[1].name.as_deref(), Some("door"));
    }

    #[test]
    fn each_change_is_one_step_of_the_history() {
        let registry = registry();
        let mut document = document(&registry);
        let mut history = History::default();
        history.reset(&document);
        let start = document.text();
        // The selection of each state comes back with the state.
        let mut selection = vec![Item::Light(0)];

        // A drag: many frames with a change, then the button goes up.
        for step in 1..=10 {
            document.room.lights[0].position[0] = step as f32 * 0.1;
            history.track(&document, &selection, true);
        }
        history.track(&document, &selection, false);
        let dragged = document.text();
        selection = vec![document.add(Kind::Cube, Vec3::ZERO).unwrap()];
        history.track(&document, &selection, false);
        // No change: no step.
        history.track(&document, &selection, false);
        assert_eq!(history.steps(), (2, 0));

        assert!(history.undo(&mut document, &mut selection));
        assert_eq!(document.text(), dragged);
        assert_eq!(selection, [Item::Light(0)]);
        assert!(history.undo(&mut document, &mut selection));
        assert_eq!(document.text(), start);
        assert!(!history.undo(&mut document, &mut selection));
        assert!(
            history.redo(&mut document, &mut selection)
                && history.redo(&mut document, &mut selection)
        );
        assert_eq!(document.room.props.len(), 1);
        assert_eq!(selection, [Item::Prop(0)]);
        // A change after an undo drops the steps that were undone.
        history.undo(&mut document, &mut selection);
        document.delete(&[Item::Light(0)]);
        history.track(&document, &selection, false);
        assert_eq!(history.steps(), (2, 0));
        // A change that was not tracked yet is a step of its own before the undo.
        document.room.width = 9.0;
        history.undo(&mut document, &mut selection);
        assert_eq!((document.room.width, document.room.lights.len()), (8.0, 0));
    }

    #[test]
    fn the_users_of_a_spawn_point_in_other_room_files_are_found() {
        let registry = registry();
        let directory = std::env::temp_dir().join(format!("maki-users-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let room = |commands: &str| {
            format!(
                "(width: 4, depth: 4, height: 3, events: [(name: \"go\", pages: [(trigger: Touch, commands: [{commands}])])])"
            )
        };
        std::fs::write(
            directory.join("a.ron"),
            room(r#"Transfer("b.ron", "west")"#),
        )
        .unwrap();
        std::fs::write(
            directory.join("c.ron"),
            room(r#"If([Flag("x")], [], [Transfer("b.ron", "west")])"#),
        )
        .unwrap();
        std::fs::write(
            directory.join("d.ron"),
            room(r#"Transfer("b.ron", "north")"#),
        )
        .unwrap();
        std::fs::write(
            directory.join("b.ron"),
            room(r#"Transfer("b.ron", "west")"#),
        )
        .unwrap();
        let users = spawn_users(&directory, &directory.join("b.ron"), "west", &registry);
        assert_eq!(users, ["a.ron", "c.ron"]);
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
