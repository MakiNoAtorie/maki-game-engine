//! Changes of the document that are more than one value: add, delete, and duplicate
//! an item, names, and the history for undo and redo.
//!
//! Names: a command names an object by its id, so a new name changes no command. A
//! name is made unique among the objects all the same, so that a pick by name is
//! clear.

use std::path::{Path, PathBuf};

use glam::Vec3;
use maki_assets::{Doorway, Wall};
use maki_core::Transform;
use maki_events::{EventPages, Page, Trigger, TriggerArea};
use maki_scene::{
    ComponentText, ComponentTypes, DoorLeaf, LightDef, MeshDef, MoverDef, ObjectDef, PoseDef,
    PrefabDef, PrefabUse, PropDef, RoomDef, instance_objects, pose_out, prefab_text, read_prefab,
};

use crate::ron_text::Notes;
use crate::{Document, Item};

/// Reads a prefab file to change an instance of it. A file with an object that has
/// no id gets the ids and is written: a change of an instance is keyed by the id.
// ponytail: the file is written again from its value, so its comments go; keep them as the notes of a room if a person writes prefab files.
pub(crate) fn prefab_to_edit(path: &Path) -> anyhow::Result<PrefabDef> {
    let mut prefab = read_prefab(path)?;
    if prefab.give_ids() {
        std::fs::write(path, prefab_text(&prefab))?;
        tracing::info!(file = %path.display(), "the objects of the prefab file got ids");
    }
    Ok(prefab)
}

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
    /// A small cube with an area, and a wire on its output `used` that opens the
    /// first door or mover.
    Button,
}

impl Kind {
    /// The kinds with no file, with the text of the menu.
    pub const MENU: [(&'static str, Kind); 10] = [
        ("Cube", Kind::Cube),
        ("Cuboid", Kind::Cuboid),
        ("Sphere", Kind::Sphere),
        ("Light", Kind::Light),
        ("Spawn point", Kind::Spawn),
        ("Doorway", Kind::Doorway),
        ("Door", Kind::Door),
        ("Mover", Kind::Mover),
        ("Event area", Kind::Event),
        ("Button", Kind::Button),
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

    /// Call after the name of an object was changed: the name is made unique in the
    /// room. A command names the object by its id, so nothing else changes. Returns a
    /// text for the user if the name was changed.
    pub fn keep_name_unique(&mut self, item: &Item) -> String {
        let Item::Object(index) = item else {
            return String::new();
        };
        let Some(typed) = self.room.objects[*index].name.take() else {
            return String::new();
        };
        let new = self.unique_name(item, &typed);
        let status = match new == typed {
            true => String::new(),
            false => "The name was in use: a number was added.".to_owned(),
        };
        self.room.objects[*index].name = Some(new);
        status
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
                    Kind::Model(path) => prop(MeshDef::File(path.into()), on_floor(0.0)),
                    _ => unreachable!("the outer arm has the four kinds of a prop"),
                };
                // A name of its own: the outliner lists the object by it, and a
                // label by the index would change with each delete before it.
                let kind = match &prop.mesh {
                    MeshDef::Cube(_) => "cube".to_owned(),
                    MeshDef::Cuboid(..) => "cuboid".to_owned(),
                    MeshDef::Sphere { .. } => "sphere".to_owned(),
                    MeshDef::Capsule { .. } => "capsule".to_owned(),
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
                    file: file.into(),
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
            // An object with an area and an event on it.
            Kind::Event => {
                let item = Item::Object(room.objects.len());
                let mut object = ObjectDef {
                    pose: on_floor(0.0),
                    ..ObjectDef::default()
                };
                object.set_component("area", &TriggerArea::default());
                let page = Page {
                    conditions: Vec::new(),
                    trigger: Trigger::Interact,
                    repeat: false,
                    commands: Vec::new(),
                };
                let pages = EventPages {
                    pages: vec![page],
                    wires: Vec::new(),
                };
                object.set_component("event", &pages);
                room.objects.push(object);
                let name = self.unique_name(&item, "event");
                self.room.objects.last_mut().expect("pushed above").name = Some(name);
                item
            }
            Kind::Button => {
                let moves = |object: &&ObjectDef| {
                    object.components.contains_key("door")
                        || object.components.contains_key("mover")
                };
                let door = room.objects.iter().find(moves);
                let door = door.and_then(|door| door.id);
                let mut pose = on_floor(0.15);
                pose.scale = [0.3; 3];
                let mut object = PropDef {
                    mesh: MeshDef::Cube(1.0),
                    pose,
                    ..PropDef::default()
                }
                .into_object();
                object.set_component(
                    "area",
                    &TriggerArea {
                        width: 0.8,
                        depth: 0.8,
                    },
                );
                // The text of the event: the commands of a wire need the registry to be
                // made as values.
                let door = door.unwrap_or_default();
                let used = TriggerArea::USED;
                let wire = format!("(wires: [(output: {used:?}, commands: [OpenDoor({door:?})])])");
                let wire = ComponentText::parse(&wire).expect("the text is RON");
                object.components.insert("event".to_owned(), wire);
                let item = Item::Object(room.objects.len());
                room.objects.push(object);
                let name = self.unique_name(&item, "button");
                self.room.objects.last_mut().expect("pushed above").name = Some(name);
                item
            }
        };
        self.room.give_ids();
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
            }
            self.dirty = true;
        }
    }

    /// Makes a copy of each item, a little beside it, with a name of its own.
    /// Returns the copies. The room has no copy.
    pub fn duplicate(&mut self, items: &[Item]) -> Vec<Item> {
        let source = self.clone();
        self.copy_from(&source, items, DUPLICATE_OFFSET, false)
    }

    /// Makes in this document a copy of each of `items` of the document `source`
    /// (another room, or this one), `offset` meters beside its place, with a name
    /// that this room does not have and with its notes. Returns the copies. The
    /// room has no copy. `cut`: the items were cut from `source`; in that room they
    /// keep their ids.
    pub fn copy_from(
        &mut self,
        source: &Document,
        items: &[Item],
        offset: f32,
        cut: bool,
    ) -> Vec<Item> {
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
                    // A paste of a cut object into its own room keeps the id.
                    let keep = cut
                        && source.path == self.path
                        && room.objects.iter().all(|other| other.id != object.id);
                    if !keep || object.id.is_none() {
                        object.id = Some(room.new_id());
                    }
                    let copy = Item::Object(room.objects.len());
                    let name = object.name.take();
                    room.objects.push(object);
                    let name = name.map(|name| self.unique_name(&copy, &name));
                    self.room.objects.last_mut().expect("pushed above").name = name;
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
        let mut prefab = PrefabDef {
            objects: objects
                .map(|object| into_space(object.clone(), &space))
                .collect(),
            ..PrefabDef::default()
        };
        prefab.give_ids();
        let path = self.path.parent().unwrap_or(Path::new("")).join(file);
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new("")))
            .and_then(|()| std::fs::write(&path, prefab_text(&prefab)))
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;

        self.delete(&parts);
        let item = Item::Object(self.room.objects.len());
        let stem = file.file_stem().unwrap_or_default().to_string_lossy();
        let used = PrefabUse {
            name: None,
            // Forward slashes: the same room file on each system. An old prefab file keeps its id.
            file: maki_assets::FileRef {
                path: PathBuf::from(file.to_string_lossy().replace('\\', "/")),
                id: maki_assets::FileId::read(&path),
            },
            pose: outer,
            changes: Default::default(),
        };
        self.room.objects.push(used.into_object());
        self.room.give_ids();
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
        let path = used
            .file
            .resolve(self.path.parent().unwrap_or(Path::new("")));
        let prefab = prefab_to_edit(&path).map_err(|error| format!("{error:#}"))?;
        let mut items = Vec::new();
        // The first object keeps the id of the instance, so a command still names it.
        let mut id = self.object(item).and_then(|instance| instance.id);
        for mut object in instance_objects(&used, &prefab, types) {
            object.id = id.take().or_else(|| Some(self.room.new_id()));
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
        Ok((items, used.file.path, used.pose))
    }

    /// What undo and redo keep: all of the document but the path.
    fn content(&self) -> Content {
        (self.room.clone(), self.notes.clone())
    }

    fn is(&self, content: &Content) -> bool {
        self.room == content.0 && self.notes == content.1
    }

    fn set_content(&mut self, content: Content) {
        (self.room, self.notes) = content;
        self.dirty = true;
    }
}

/// An object of the room in the space of a prefab at `space`: the inverse of the
/// place that `maki_scene::instance_objects` gives it.
fn into_space(mut object: ObjectDef, space: &Transform) -> ObjectDef {
    // The ids of a prefab file are of the file, not of a room.
    object.id = None;
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
        Item::Object(index) => *index,
    }
}

type Content = (RoomDef, Notes);
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
    /// The highest `next_id` of the room: undo and redo keep it, so that an id that
    /// was given is not given again.
    next_id: u32,
}

/// Older steps are dropped.
const MAX_STEPS: usize = 200;

impl History {
    /// A document was opened: no step.
    pub fn reset(&mut self, document: &Document) {
        *self = Self {
            base: Some((document.content(), Vec::new())),
            next_id: document.room.next_id,
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
        self.next_id = self.next_id.max(document.room.next_id);
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

    /// True: the document differs from the last step.
    pub fn changed(&self, document: &Document) -> bool {
        self.base.as_ref().is_some_and(|base| !document.is(&base.0))
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
            self.next_id,
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
            self.next_id,
        )
    }

    fn step(
        base: &mut Option<Step>,
        from: &mut Vec<Step>,
        to: &mut Vec<Step>,
        document: &mut Document,
        selection: &mut Vec<Item>,
        next_id: u32,
    ) -> bool {
        let Some(mut step) = from.pop() else {
            return false;
        };
        step.0.0.next_id = next_id;
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

#[cfg(test)]
mod tests {
    use super::*;
    use maki_events::CommandRegistry;

    fn registry() -> CommandRegistry {
        // The built-in commands, as `EventsModule` adds them.
        let mut app = maki_core::App::new(maki_core::ConfigFile::default()).unwrap();
        app.add_module(maki_platform::PlatformModule)
            .add_module(maki_scene::SceneModule)
            .add_module(maki_events::EventsModule)
            .add_module(maki_gameplay::GameModule);
        app.world().resource::<CommandRegistry>().clone()
    }

    fn document(registry: &CommandRegistry) -> Document {
        let objects = [
            r#"(name: "door", components: {
                "door": (),
                "doorway": (wall: NegZ, offset: 0.0, width: 1.0, height: 2.0, depth: 1.0),
            })"#,
            r#"// The east one.
            // To the east.
            (name: "east", components: {
                "door": (),
                "doorway": (wall: PosX, offset: 0.0, width: 1.0, height: 2.0, depth: 1.0),
            })"#,
            r#"(name: "lamp", pose: (position: (0.0, 2.0, 0.0)), components: {
                "light": (radius: 0.1, color: (1.0, 1.0, 1.0), intensity: 5.0),
            })"#,
            r#"(name: "start", components: {"spawn": ()})"#,
            r#"(name: "switch", components: {"event": (pages: [(trigger: Interact, commands: [
                LightOff(3),
                GiveControl(3),
                SetField(3, "light", "intensity", 2.0),
                If([Flag("open")], [OpenDoor(2), LightOn(3)], [CloseDoor(1)]),
                Transfer("room.ron", 4),
            ])])})"#,
        ];
        let text = maki_scene::room_text(8.0, 8.0, 3.0, &objects);
        Document::parse(Path::new("room.ron"), &text, registry).unwrap()
    }

    #[test]
    fn a_rename_changes_the_name_only_and_a_name_stays_unique() {
        let registry = registry();
        let mut document = document(&registry);
        // Objects: two doorways with their doors, the light, the spawn point.
        let lamp = Item::Object(2);

        // The commands name the lamp by its id: one line of the file changes.
        let before = document.text();
        document.room.objects[2].name = Some("bulb".to_owned());
        assert_eq!(document.keep_name_unique(&lamp), "");
        let text = document.text();
        let changed: Vec<(&str, &str)> = before
            .lines()
            .zip(text.lines())
            .filter(|(old, new)| old != new)
            .collect();
        assert_eq!(changed.len(), 1, "{changed:?}");
        assert!(changed[0].1.contains("\"bulb\""), "{changed:?}");
        assert_eq!(before.lines().count(), text.lines().count());
        assert!(text.contains("LightOff(3)") && text.contains("GiveControl(3)"));

        // A door with the name of the other door gets a number.
        document.room.objects[1].name = Some("door".to_owned());
        let status = document.keep_name_unique(&Item::Object(1));
        assert!(status.contains("a number was added"), "{status}");
        assert_eq!(document.room.objects[1].name.as_deref(), Some("door_2"));
        assert!(document.text().contains("OpenDoor(2)"));

        // A new light and a copy of a light have names of their own.
        let Ok(Item::Object(new)) = document.add(Kind::Light, Vec3::ZERO) else {
            panic!("a light");
        };
        assert_eq!(document.room.objects[new].name.as_deref(), Some("light"));
        assert!(document.light(&Item::Object(new)).is_some());
        let copies = document.duplicate(&[lamp, Item::Object(new)]);
        assert_eq!(copies, [Item::Object(6), Item::Object(7)]);
        let name = |index: usize| document.room.objects[index].name.clone().unwrap();
        assert_eq!(
            (name(6), name(7)),
            ("bulb_2".to_owned(), "light_2".to_owned())
        );
        assert_eq!(free_name("bulb_2", |name| name == "bulb_2"), "bulb_3");
    }

    #[test]
    fn a_prefab_file_with_no_ids_gets_them_when_the_editor_reads_it() {
        let file =
            std::env::temp_dir().join(format!("maki-prefab-ids-{}.prefab", std::process::id()));
        let text = r#"(objects: [(name: "a", components: {"prop": (mesh: Cube(1.0))}), (components: {"prop": (mesh: Cube(1.0))})])"#;
        std::fs::write(&file, text).unwrap();
        let ids = |prefab: &PrefabDef| {
            prefab
                .objects
                .iter()
                .map(|object| object.id)
                .collect::<Vec<_>>()
        };
        let given = [Some(maki_scene::ObjectId(1)), Some(maki_scene::ObjectId(2))];
        assert_eq!(ids(&prefab_to_edit(&file).unwrap()), given);
        assert_eq!(ids(&read_prefab(&file).unwrap()), given);
        std::fs::remove_file(&file).unwrap();
    }

    #[test]
    fn an_id_given_before_an_undo_is_not_given_again() {
        let registry = registry();
        let mut document = document(&registry);
        let (mut history, mut selection) = (History::default(), Vec::new());
        history.reset(&document);
        let add = |document: &mut Document| {
            let Ok(Item::Object(cube)) = document.add(Kind::Cube, Vec3::ZERO) else {
                panic!("a cube");
            };
            document.room.objects[cube].id
        };
        let first = add(&mut document);
        history.track(&document, &selection, false);
        assert!(history.undo(&mut document, &mut selection));
        // The undo keeps the redo.
        assert!(history.redo(&mut document, &mut selection));
        assert!(history.undo(&mut document, &mut selection));
        assert_ne!(add(&mut document), first);
    }

    #[test]
    fn an_object_keeps_its_id_and_a_new_object_or_a_copy_gets_a_new_one() {
        let registry = registry();
        let mut document = document(&registry);
        let id = |document: &Document, index: usize| document.room.objects[index].id.unwrap().0;
        let ids = |document: &Document| {
            let ids = document
                .room
                .objects
                .iter()
                .map(|object| object.id.unwrap().0);
            ids.collect::<Vec<_>>()
        };

        // A room with no ids gets them in the order of its objects; a save writes them.
        assert_eq!(ids(&document), [1, 2, 3, 4, 5]);
        let text = document.text();
        assert!(text.contains("next_id: 6"), "{text}");
        let again = Document::parse(Path::new("room.ron"), &text, &registry).unwrap();
        assert_eq!(again.room, document.room);
        assert_eq!(again.text(), text);

        // A rename, a move, and the undo keep the id.
        let (mut history, mut selection) = (History::default(), Vec::new());
        history.reset(&document);
        document.room.objects[2].name = Some("bulb".to_owned());
        document.keep_name_unique(&Item::Object(2));
        document.room.objects[2].pose.position[0] = 1.0;
        history.track(&document, &selection, false);
        assert_eq!(id(&document, 2), 3);
        assert!(history.undo(&mut document, &mut selection));
        assert_eq!(document.room.objects[2].name.as_deref(), Some("lamp"));
        assert_eq!(id(&document, 2), 3);

        // A deleted id is not given again: a new object gets the next one.
        document.delete(&[Item::Object(4)]);
        let Ok(Item::Object(cube)) = document.add(Kind::Cube, Vec3::ZERO) else {
            panic!("a cube");
        };
        assert_eq!(id(&document, cube), 6);
        // A duplicate gets a new id.
        let copies = document.duplicate(&[Item::Object(2)]);
        assert_eq!(copies, [Item::Object(5)]);
        assert_eq!(id(&document, 5), 7);

        // A copy, a delete of the object, and a paste: a new id.
        let source = document.clone();
        document.delete(&[Item::Object(0)]);
        let pasted = document.copy_from(&source, &[Item::Object(0)], DUPLICATE_OFFSET, false);
        assert_eq!(pasted, [Item::Object(5)]);
        assert_eq!(id(&document, 5), 8);
        // A cut and a paste in the same room keep the id.
        let source = document.clone();
        document.delete(&[Item::Object(1)]);
        document.copy_from(&source, &[Item::Object(1)], DUPLICATE_OFFSET, true);
        assert_eq!(id(&document, 5), 3);

        // A paste of a cut into another room gets an id of that room.
        let mut other = Document::new_room(PathBuf::from("other.ron"));
        assert_eq!(ids(&other), [1, 2]);
        other.copy_from(&source, &[Item::Object(1)], 0.0, true);
        assert_eq!(ids(&other), [1, 2, 3]);
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
        assert_eq!(doorway, Item::Object(4));
        assert_eq!(document.add(Kind::Door, Vec3::ZERO), Ok(Item::Object(4)));
        assert!(document.room.objects[4].door().is_some());
        assert_eq!(document.room.objects[4].name.as_deref(), Some("door"));
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

        // The undo keeps the next id: the id of the cube is not given again.
        let given = |text: String| text.replace("next_id: 6", "next_id: 7");
        assert!(history.undo(&mut document, &mut selection));
        assert_eq!(document.text(), given(dragged));
        assert_eq!(selection, [Item::Object(2)]);
        assert!(history.undo(&mut document, &mut selection));
        assert_eq!(document.text(), given(start));
        assert!(!history.undo(&mut document, &mut selection));
        assert!(
            history.redo(&mut document, &mut selection)
                && history.redo(&mut document, &mut selection)
        );
        assert_eq!(document.room.objects.len(), 6);
        assert_eq!(selection, [Item::Object(5)]);
        // A change after an undo drops the steps that were undone.
        history.undo(&mut document, &mut selection);
        document.delete(&[Item::Object(2)]);
        history.track(&document, &selection, false);
        assert_eq!(history.steps(), (2, 0));
        // A change that was not tracked yet is a step of its own before the undo.
        document.room.width = 9.0;
        history.undo(&mut document, &mut selection);
        assert_eq!((document.room.width, document.room.objects.len()), (8.0, 4));
    }

    #[test]
    fn a_button_has_a_wire_and_a_change_of_a_wire_is_one_step_of_the_history() {
        let registry = registry();
        let mut document = document(&registry);
        let mut history = History::default();
        history.reset(&document);
        let mut selection = Vec::new();

        // "Add > Button": a wire on `used` that opens the first door of the room.
        let item = document.add(Kind::Button, Vec3::ZERO).unwrap();
        history.track(&document, &selection, false);
        let Item::Object(index) = item else {
            panic!("an object");
        };
        let mut event = document.event(index, &registry).unwrap().unwrap();
        let texts: Vec<String> = event.wires[0]
            .commands
            .iter()
            .map(|c| format!("{c:?}"))
            .collect();
        assert_eq!(
            (event.wires[0].output.as_str(), texts),
            ("used", vec!["OpenDoor(1)".to_owned()])
        );

        // A change of the output, then a delete: each a step of the history.
        event.wires[0].output = "entered".to_owned();
        document.set_event(index, &event);
        history.track(&document, &selection, false);
        event.wires.clear();
        document.set_event(index, &event);
        history.track(&document, &selection, false);
        assert!(history.undo(&mut document, &mut selection));
        let output = |document: &Document| {
            let event = document.event(index, &registry).unwrap().unwrap();
            event.wires.first().map(|wire| wire.output.clone())
        };
        assert_eq!(output(&document).as_deref(), Some("entered"));
        assert!(history.undo(&mut document, &mut selection));
        assert_eq!(output(&document).as_deref(), Some("used"));
    }
}
