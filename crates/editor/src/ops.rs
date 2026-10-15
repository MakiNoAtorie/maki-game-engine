//! Changes of the document that are more than one value: add, delete, and duplicate
//! an item, names and the events that use them, and the history for undo and redo.
//!
//! Names: events find an object (a light, a door, a mover, a spawn point) by name.
//! So a new or changed name is made unique in its group (the objects; the events),
//! and a change of a name changes the commands of this room that use it. A command of another
//! module, and a `Transfer` in another room file, are not changed: see
//! [`spawn_users`].

use std::path::{Path, PathBuf};

use glam::Vec3;
use maki_assets::{Doorway, Wall};
use maki_audio::AudioCommand;
use maki_core::Transform;
use maki_events::{Area, Builtin, Command, CommandRegistry, CommandSet, EventDef, Page, Trigger};
use maki_scene::{
    ComponentTypes, DoorLeaf, LightDef, MeshDef, MoverDef, ObjectDef, PoseDef, PrefabDef,
    PrefabUse, PropDef, RoomDef, instance_objects, pose_out, prefab_text, read_prefab,
};

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
    /// A prefab file, from the directory of the room file.
    Prefab(PathBuf),
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
pub(crate) const DUPLICATE_OFFSET: f32 = 0.5;

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

/// A pose with each number at a step of 0.0001: a pose that went into the space of
/// a prefab and back is the number that a person wrote, not 0.99999994.
fn tidy(pose: PoseDef) -> PoseDef {
    // `+ 0.0`: no "-0.0" in a file.
    let round = |values: [f32; 3]| values.map(|value| (value * 1e4).round() / 1e4 + 0.0);
    PoseDef {
        position: round(pose.position),
        rotation: round(pose.rotation),
        scale: round(pose.scale),
    }
}

impl Document {
    /// True if the document has the item.
    pub fn has(&self, item: &Item) -> bool {
        match item {
            Item::Room => true,
            Item::Object(index) => *index < self.room.objects.len(),
            Item::Event(index) => *index < self.events.len(),
        }
    }

    /// The names of the group of an item, without the name of the item itself.
    /// `None`: the room, which has no name.
    fn other_names(&self, item: &Item) -> Option<Vec<&str>> {
        Some(match item {
            Item::Object(index) => {
                let objects = self.room.objects.iter().enumerate();
                let others = objects.filter(|(other, _)| other != index);
                others
                    .filter_map(|(_, object)| object.name.as_deref())
                    .collect()
            }
            Item::Event(index) => {
                let events = self.events.iter().enumerate();
                let others = events.filter(|(other, _)| other != index);
                others.map(|(_, event)| event.name.as_str()).collect()
            }
            Item::Room => return None,
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

    /// Call after the name of an object or an event was changed from `old`. The new
    /// name is made unique, and the commands of the events of this room that name the
    /// object (built-in, and of the audio module) get the new name. Returns a text for
    /// the user if something more than the name itself was changed.
    pub fn renamed(
        &mut self,
        item: &Item,
        old: Option<String>,
        registry: &CommandRegistry,
    ) -> String {
        let index = match item {
            Item::Object(index) => *index,
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
            Item::Room => return String::new(),
        };
        let Some(typed) = self.room.objects[index].name.take() else {
            // Events that still name the object give a warning when they run.
            return String::new();
        };
        let new = self.unique_name(item, &typed);
        let mut status = String::new();
        if new != typed {
            status = "The name was in use: a number was added. ".to_owned();
        }
        let mut users = 0;
        if let Some(old) = &old {
            // A command names the object by the kind that it wants; an object can
            // be each kind.
            users += self.rename_in_events(registry, |command: &Builtin| match command {
                Builtin::LightOn(name) if name == old => Some(Builtin::LightOn(new.clone())),
                Builtin::LightOff(name) if name == old => Some(Builtin::LightOff(new.clone())),
                Builtin::LightIntensity(name, value) if name == old => {
                    Some(Builtin::LightIntensity(new.clone(), *value))
                }
                Builtin::OpenDoor(name) if name == old => Some(Builtin::OpenDoor(new.clone())),
                Builtin::CloseDoor(name) if name == old => Some(Builtin::CloseDoor(new.clone())),
                _ => None,
            });
            users += self.rename_in_events(registry, |command: &AudioCommand| match command {
                AudioCommand::SoundOn(name) if name == old => {
                    Some(AudioCommand::SoundOn(new.clone()))
                }
                AudioCommand::SoundOff(name) if name == old => {
                    Some(AudioCommand::SoundOff(new.clone()))
                }
                _ => None,
            });
            // A `Transfer` of this room to itself names the spawn point.
            if self.room.objects[index].is_spawn() {
                let file = self.path.file_name().unwrap_or_default();
                let file = file.to_string_lossy().into_owned();
                users += self.rename_in_events(registry, |command: &Builtin| match command {
                    Builtin::Transfer(room, to) if *room == file && to == old => {
                        Some(Builtin::Transfer(room.clone(), new.clone()))
                    }
                    _ => None,
                });
            }
        }
        if users > 0 {
            status += &format!("{users} event commands now name `{new}`");
        }
        self.room.objects[index].name = Some(new);
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
                let prop = match kind {
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
                };
                // A name of its own: the outliner lists the object by it, and a
                // label by the index would change with each delete before it.
                let kind = match &prop.mesh {
                    MeshDef::Cube(_) => "cube".to_owned(),
                    MeshDef::Cuboid(..) => "cuboid".to_owned(),
                    MeshDef::Sphere { .. } => "sphere".to_owned(),
                    MeshDef::File(path) => {
                        let stem = path.file_stem().unwrap_or_default();
                        stem.to_string_lossy().into_owned()
                    }
                };
                room.objects.push(prop.into_object());
                let item = Item::Object(room.objects.len() - 1);
                let name = self.unique_name(&item, &kind);
                self.room.objects.last_mut().expect("pushed above").name = Some(name);
                item
            }
            Kind::Prefab(file) => {
                let item = Item::Object(room.objects.len());
                let stem = file.file_stem().unwrap_or_default();
                let stem = stem.to_string_lossy().into_owned();
                let used = PrefabUse {
                    name: None,
                    file,
                    pose: on_floor(0.0),
                    changes: Default::default(),
                };
                room.objects.push(used.into_object());
                let name = self.unique_name(&item, &stem);
                self.room.objects.last_mut().expect("pushed above").name = Some(name);
                item
            }
            Kind::Light => {
                let light = LightDef {
                    name: None,
                    position: [at.x, (room.height - 0.6).max(0.5), at.z],
                    radius: 0.15,
                    color: [1.0, 0.9, 0.8],
                    intensity: 8.0,
                    on: true,
                    direction: None,
                    cone: None,
                    sound: None,
                };
                let item = Item::Object(room.objects.len());
                room.objects.push(light.into_object());
                let name = self.unique_name(&item, "light");
                self.room.objects.last_mut().expect("pushed above").name = Some(name);
                item
            }
            Kind::Spawn => {
                let item = Item::Object(room.objects.len());
                let name = self.unique_name(&item, "spawn");
                let spawn = ObjectDef::spawn_point(name, in_floor(at));
                self.room.objects.push(spawn);
                item
            }
            Kind::Doorway => {
                let mut object = ObjectDef::default();
                object.set_component(
                    "doorway",
                    &Doorway {
                        wall: Wall::NegZ,
                        offset: at.x.clamp(-room.width / 2.0 + 1.0, room.width / 2.0 - 1.0),
                        width: 1.0,
                        height: 2.1,
                        depth: 1.0,
                    },
                );
                room.objects.push(object);
                Item::Object(room.objects.len() - 1)
            }
            Kind::Door => {
                // A door is in the doorway of its object.
                let free =
                    |object: &ObjectDef| object.doorway().is_some() && object.door().is_none();
                let Some(index) = room.objects.iter().position(free) else {
                    return Err("Each doorway has a door: add a doorway first".to_owned());
                };
                let door = DoorLeaf {
                    seconds: 1.2,
                    color: [0.35, 0.2, 0.1],
                    open: false,
                };
                room.objects[index].set_component("door", &door);
                let item = Item::Object(index);
                if room.objects[index].name.is_none() {
                    let name = self.unique_name(&item, "door");
                    self.room.objects[index].name = Some(name);
                }
                item
            }
            Kind::Mover => {
                let (a, mut b) = (on_floor(0.5), on_floor(0.5));
                b.position[1] += 1.0;
                let mover = MoverDef {
                    name: None,
                    mesh: MeshDef::Cube(1.0),
                    look: Default::default(),
                    a,
                    b,
                    seconds: 2.0,
                };
                let item = Item::Object(room.objects.len());
                room.objects.push(mover.into_object());
                let name = self.unique_name(&item, "mover");
                self.room.objects.last_mut().expect("pushed above").name = Some(name);
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

    /// Removes items. The room itself is not removed.
    pub fn delete(&mut self, items: &[Item]) {
        // From the last index down: a removal does not move an item that is still
        // to remove.
        let mut items: Vec<&Item> = items.iter().filter(|item| self.has(item)).collect();
        items.sort_by_key(|item| std::cmp::Reverse(item_index(item)));
        for item in items {
            if let Some((list, index)) = self.notes_key(item) {
                self.notes.shift(list, index, -1);
            }
            match item {
                Item::Room => continue,
                Item::Object(index) => drop(self.room.objects.remove(*index)),
                Item::Event(index) => drop(self.events.remove(*index)),
            }
            self.dirty = true;
        }
    }

    /// Makes a copy of each item, a little beside it, with a name of its own.
    /// Returns the copies. The room has no copy.
    pub fn duplicate(&mut self, items: &[Item]) -> Vec<Item> {
        let source = self.clone();
        self.copy_from(&source, items, DUPLICATE_OFFSET)
    }

    /// Makes in this document a copy of each of `items` of the document `source`
    /// (another room, or this one), `offset` meters beside its place, with a name
    /// that this room does not have and with its notes. Returns the copies. The
    /// room has no copy.
    pub fn copy_from(&mut self, source: &Document, items: &[Item], offset: f32) -> Vec<Item> {
        let shift = |position: &mut [f32; 3]| {
            position[0] += offset;
            position[2] += offset;
        };
        let mut copies = Vec::new();
        for item in items {
            if !source.has(item) {
                continue;
            }
            let notes = source.notes_of(item).to_vec();
            let room = &mut self.room;
            let copy = match item {
                Item::Room => continue,
                Item::Object(index) => {
                    let mut object = source.room.objects[*index].clone();
                    // The place of a light is the pose: it moves with it.
                    shift(&mut object.pose.position);
                    // A doorway is beside its source in the wall.
                    if let (Some(Ok(mut doorway)), true) = (object.doorway(), offset != 0.0) {
                        doorway.offset += doorway.width + offset;
                        object.set_component("doorway", &doorway);
                    }
                    if let Some(Ok(mut mover)) = object.mover() {
                        shift(&mut mover.b.position);
                        object.set_mover(mover);
                    }
                    let copy = Item::Object(room.objects.len());
                    let name = object.name.take();
                    room.objects.push(object);
                    let name = name.map(|name| self.unique_name(&copy, &name));
                    self.room.objects.last_mut().expect("pushed above").name = name;
                    copy
                }
                Item::Event(index) => {
                    let mut event = source.events[*index].clone();
                    if let Some(area) = &mut event.area {
                        area.center[0] += offset;
                        area.center[1] += offset;
                    }
                    let copy = Item::Event(self.events.len());
                    let name = std::mem::take(&mut event.name);
                    self.events.push(event);
                    let name = self.unique_name(&copy, &name);
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

    /// Writes the selected objects as a prefab file, and puts one instance of it in
    /// their place. `file`: from the directory of the room file. `outer`: the pose
    /// that the instance gets in this room; `None`: at the floor point of the first
    /// of the objects, with no turn. A file of that name is replaced: each room that
    /// names it shows the new content. Returns the instance. `Err`: why not.
    // The file is written now, not with the room, and undo does not bring the old
    // file back: a prefab file is not a part of the document.
    pub fn make_prefab(
        &mut self,
        items: &[Item],
        file: &Path,
        outer: Option<PoseDef>,
    ) -> Result<Item, String> {
        // No prefab in a prefab yet: an instance is left out.
        let is_part = |item: &&Item| {
            let object = self.object(item);
            object.is_some_and(|object| !object.components.contains_key("prefab"))
        };
        let parts: Vec<Item> = items.iter().filter(is_part).cloned().collect();
        let Some(first) = parts.first() else {
            return Err(
                "A prefab is made of objects that are not prefabs: select some first".to_owned(),
            );
        };
        let outer = outer.unwrap_or_else(|| {
            let at = self
                .object(first)
                .expect("`parts` has objects")
                .pose
                .position;
            PoseDef {
                position: in_floor(at.into()),
                ..PoseDef::default()
            }
        });
        let space = outer.transform();
        let objects = parts
            .iter()
            .map(|item| self.object(item).expect("`parts` has objects"));
        let prefab = PrefabDef {
            objects: objects
                .map(|object| into_space(object.clone(), &space))
                .collect(),
        };
        let path = self.path.parent().unwrap_or(Path::new("")).join(file);
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new("")))
            .and_then(|()| std::fs::write(&path, prefab_text(&prefab)))
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;

        self.delete(&parts);
        let item = Item::Object(self.room.objects.len());
        let stem = file.file_stem().unwrap_or_default().to_string_lossy();
        let used = PrefabUse {
            name: None,
            // Forward slashes: the same room file on each system.
            file: PathBuf::from(file.to_string_lossy().replace('\\', "/")),
            pose: outer,
            changes: Default::default(),
        };
        self.room.objects.push(used.into_object());
        let name = self.unique_name(&item, &stem);
        self.room.objects.last_mut().expect("pushed above").name = Some(name);
        self.dirty = true;
        Ok(item)
    }

    /// Puts the objects of a prefab instance into the room, each at its place and
    /// with the changes of the instance, in place of the instance: to change them,
    /// and to make the prefab again. `types`: for the changes. Returns the new
    /// objects, and the file and the pose that the instance had.
    pub fn unpack_prefab(
        &mut self,
        item: &Item,
        types: &ComponentTypes,
    ) -> Result<(Vec<Item>, PathBuf, PoseDef), String> {
        let Some(used) = self.prefab_use(item) else {
            return Err("Select a prefab first".to_owned());
        };
        let path = self.path.parent().unwrap_or(Path::new("")).join(&used.file);
        let prefab = read_prefab(&path).map_err(|error| format!("{error:#}"))?;
        let mut items = Vec::new();
        for mut object in instance_objects(&used, &prefab, types) {
            object.pose = tidy(object.pose);
            let item = Item::Object(self.room.objects.len());
            let name = object.name.take();
            self.room.objects.push(object);
            let name = name.map(|name| self.unique_name(&item, &name));
            self.room.objects.last_mut().expect("pushed above").name = name;
            items.push(item);
        }
        // The new objects are behind the instance: their indices go one down.
        self.delete(std::slice::from_ref(item));
        if let Item::Object(gone) = item {
            for new in &mut items {
                if let Item::Object(index) = new
                    && *index > *gone
                {
                    *index -= 1;
                }
            }
        }
        self.dirty = true;
        Ok((items, used.file, used.pose))
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

/// An object of the room in the space of a prefab at `space`: the inverse of the
/// place that `maki_scene::instance_objects` gives it.
fn into_space(mut object: ObjectDef, space: &Transform) -> ObjectDef {
    object.pose = tidy(PoseDef::of(&pose_out(space, &object.pose.transform())));
    if let Some(Ok(mut light)) = object.light() {
        let back = space.rotation.inverse();
        light.direction = light
            .direction
            .map(|direction| (back * Vec3::from(direction)).into());
        object.set_light(light);
    }
    if let Some(Ok(mut mover)) = object.mover() {
        mover.b = tidy(PoseDef::of(&pose_out(space, &mover.b.transform())));
        object.set_mover(mover);
    }
    object
}

/// The index of an item in its list. 0 for the room.
fn item_index(item: &Item) -> usize {
    match item {
        Item::Room => 0,
        Item::Object(index) | Item::Event(index) => *index,
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
            objects: [
                (name: "door", components: {
                    "door": (),
                    "doorway": (wall: NegZ, offset: 0.0, width: 1.0, height: 2.0, depth: 1.0),
                }),
                // The east one.
                // To the east.
                (name: "east", components: {
                    "door": (),
                    "doorway": (wall: PosX, offset: 0.0, width: 1.0, height: 2.0, depth: 1.0),
                }),
                (name: "lamp", pose: (position: (0.0, 2.0, 0.0)), components: {
                    "light": (radius: 0.1, color: (1.0, 1.0, 1.0), intensity: 5.0),
                }),
                (name: "start", components: {"spawn": ()}),
            ],
            events: [(name: "switch", pages: [(trigger: Interact, commands: [
                LightOff("lamp"),
                If([Flag("open")], [OpenDoor("east"), LightOn("lamp")], [CloseDoor("door")]),
                Transfer("room.ron", "start"),
            ])])],
        )"#;
        Document::parse(Path::new("room.ron"), text, registry).unwrap()
    }

    #[test]
    fn a_rename_changes_the_events_and_a_name_stays_unique() {
        let registry = registry();
        let mut document = document(&registry);
        // Objects: two doorways with their doors, the light, the spawn point.
        let lamp = Item::Object(2);

        document.room.objects[2].name = Some("bulb".to_owned());
        let status = document.renamed(&lamp, Some("lamp".to_owned()), &registry);
        assert!(status.contains("2 event commands"), "{status}");
        let text = document.text();
        assert!(text.contains(r#"LightOff("bulb")"#) && text.contains(r#"LightOn("bulb")"#));
        assert!(!text.contains("\"lamp\""), "{text}");

        // A door with the name of the other door gets a number, and the events
        // follow the door that was renamed.
        document.room.objects[1].name = Some("door".to_owned());
        document.renamed(&Item::Object(1), Some("east".to_owned()), &registry);
        assert_eq!(document.room.objects[1].name.as_deref(), Some("door_2"));
        let text = document.text();
        assert!(text.contains(r#"OpenDoor("door_2")"#) && text.contains(r#"CloseDoor("door")"#));

        // A new light and a copy of a light have names of their own.
        let Ok(Item::Object(new)) = document.add(Kind::Light, Vec3::ZERO) else {
            panic!("a light");
        };
        assert_eq!(document.room.objects[new].name.as_deref(), Some("light"));
        assert!(document.light(&Item::Object(new)).is_some());
        let copies = document.duplicate(&[lamp, Item::Object(new)]);
        assert_eq!(copies, [Item::Object(5), Item::Object(6)]);
        let name = |index: usize| document.room.objects[index].name.clone().unwrap();
        assert_eq!(
            (name(5), name(6)),
            ("bulb_2".to_owned(), "light_2".to_owned())
        );
        assert_eq!(free_name("bulb_2", |name| name == "bulb_2"), "bulb_3");

        // A spawn point: the `Transfer` to it in this room follows.
        let start = Item::Object(3);
        assert!(document.room.objects[3].is_spawn());
        document.room.objects[3].name = Some("begin".to_owned());
        let status = document.renamed(&start, Some("start".to_owned()), &registry);
        assert!(status.contains("1 event commands"), "{status}");
        assert!(document.text().contains(r#"Transfer("room.ron", "begin")"#));
    }

    #[test]
    fn a_doorway_and_its_door_are_one_object() {
        let registry = registry();
        let mut document = document(&registry);
        // The notes of the doorway, then of its door.
        let notes = ["The east one.", "To the east."];
        assert_eq!(document.notes.of("objects", 1), notes);
        let east = document.room.objects[1].clone();
        assert_eq!(east.name.as_deref(), Some("east"));
        assert!(east.door().is_some() && east.doorway().is_some());

        // A doorway goes with its door, and the other keeps its notes.
        document.delete(&[Item::Object(0)]);
        assert_eq!(document.room.objects[0], east);
        assert_eq!(document.notes.of("objects", 0), notes);
        assert!(maki_scene::parse_room(&document.text()).is_ok());

        // A door goes into the first doorway with no door; with none: an error.
        assert!(document.add(Kind::Door, Vec3::ZERO).is_err());
        let Ok(doorway) = document.add(Kind::Doorway, Vec3::ZERO) else {
            panic!("a doorway");
        };
        assert_eq!(doorway, Item::Object(3));
        assert_eq!(document.add(Kind::Door, Vec3::ZERO), Ok(Item::Object(3)));
        assert!(document.room.objects[3].door().is_some());
        assert_eq!(document.room.objects[3].name.as_deref(), Some("door"));
    }

    #[test]
    fn each_change_is_one_step_of_the_history() {
        let registry = registry();
        let mut document = document(&registry);
        let mut history = History::default();
        history.reset(&document);
        let start = document.text();
        // The selection of each state comes back with the state.
        let mut selection = vec![Item::Object(2)];

        // A drag: many frames with a change, then the button goes up.
        for step in 1..=10 {
            document.room.objects[2].pose.position[0] = step as f32 * 0.1;
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
        assert_eq!(selection, [Item::Object(2)]);
        assert!(history.undo(&mut document, &mut selection));
        assert_eq!(document.text(), start);
        assert!(!history.undo(&mut document, &mut selection));
        assert!(
            history.redo(&mut document, &mut selection)
                && history.redo(&mut document, &mut selection)
        );
        assert_eq!(document.room.objects.len(), 5);
        assert_eq!(selection, [Item::Object(4)]);
        // A change after an undo drops the steps that were undone.
        history.undo(&mut document, &mut selection);
        document.delete(&[Item::Object(2)]);
        history.track(&document, &selection, false);
        assert_eq!(history.steps(), (2, 0));
        // A change that was not tracked yet is a step of its own before the undo.
        document.room.width = 9.0;
        history.undo(&mut document, &mut selection);
        assert_eq!((document.room.width, document.room.objects.len()), (8.0, 3));
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
