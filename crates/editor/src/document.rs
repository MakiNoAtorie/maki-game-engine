//! The document: the room that the editor has open. It is the content of one room
//! file: the room, its events, and its comments. The panels change the document, the
//! world is built from it, and a save writes it. The world is never written back.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use glam::{Quat, Vec3};
use maki_assets::Doorway;
use maki_core::Fields;
use maki_core::fields::{Value, read};
use maki_events::{Area, CommandRegistry, EventDef};
use maki_scene::{
    ComponentTypes, DoorDef, LightDef, MoverDef, ObjectDef, PoseDef, PropDef, RoomDef,
    RoomValidators, check_room, ron_options,
};

use crate::ron_text::{Notes, pretty, read_notes};

/// One thing of a room that the outliner lists and the property panel shows.
#[derive(Debug, Clone, PartialEq, Eq, Default, Hash)]
pub enum Item {
    /// The size and the textures of the room.
    #[default]
    Room,
    Doorway(usize),
    Door(usize),
    /// An object of the list `objects`.
    Object(usize),
    Prop(usize),
    Mover(usize),
    Light(usize),
    /// A prefab file that the room names.
    Prefab(usize),
    /// By name.
    Spawn(String),
    Event(usize),
}

/// The part of an item that tells where it is: what a gizmo changes.
pub enum Place<'a> {
    /// A prop, a prefab, or one of the two poses of a mover.
    Pose(&'a mut PoseDef),
    /// A light, a spawn point.
    Point(&'a mut [f32; 3]),
    /// The rectangle of an event on the floor.
    Area(&'a mut Area),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// The room file. It need not exist: a new room has a path and no file.
    pub path: PathBuf,
    pub room: RoomDef,
    /// `RoomDef` does not hold them: the events crate reads them from the same file.
    pub events: Vec<EventDef>,
    /// The comments of the file.
    pub notes: Notes,
    /// True: the document is not what the file holds.
    pub dirty: bool,
}

impl Item {
    /// True: the item has a name that the user gives it.
    pub fn has_name(&self) -> bool {
        !matches!(self, Item::Room | Item::Doorway(_))
    }
}

impl Document {
    /// An empty room of 8 x 8 x 3 m that is not saved. It has what a room needs to
    /// show and to be played: one lamp (with no light the picture is black) and one
    /// spawn point.
    pub fn new_room(path: PathBuf) -> Self {
        let lamp = LightDef {
            name: Some("lamp".to_owned()),
            position: [0.0, 2.4, 0.0],
            radius: 0.15,
            color: [1.0, 0.9, 0.8],
            intensity: 12.0,
            on: true,
            direction: None,
            cone: None,
            sound: None,
        };
        Self {
            path,
            room: RoomDef {
                width: 8.0,
                depth: 8.0,
                height: 3.0,
                objects: vec![lamp.into_object()],
                spawns: [("start".to_owned(), [0.0; 3])].into(),
                ..RoomDef::default()
            },
            events: Vec::new(),
            notes: Notes::default(),
            dirty: true,
        }
    }

    /// Reads a room file.
    pub fn read(path: &Path, registry: &CommandRegistry) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read room file {}", path.display()))?;
        Self::parse(path, &text, registry)
    }

    /// The document of the text of a room file. `path` is the file that a save writes.
    // Without the checks of the other crates (`RoomValidators`): a room with a wrong
    // name in an event must open, so that the name can be set right. The editor
    // shows what those checks find (`problems`).
    pub fn parse(path: &Path, text: &str, registry: &CommandRegistry) -> Result<Self> {
        let room = check_room(path, text, &RoomValidators::default())?;
        let events = registry
            .parse_events(text)
            .map_err(|error| anyhow!("{}:{error}", path.display()))?;
        Ok(Self {
            path: path.to_owned(),
            room,
            events,
            notes: read_notes(text),
            dirty: false,
        })
    }

    /// The document as the text of a room file.
    pub fn text(&self) -> String {
        let write = |result: Result<String, _>| -> String {
            result.expect("a room has no value that RON cannot write")
        };
        let mut compact = write(ron_options().to_string(&self.room));
        if !self.events.is_empty() {
            // `RoomDef` does not write its `events` field: it goes in as the last one.
            let events = write(ron_options().to_string(&self.events));
            compact.pop();
            compact.push_str(&format!(",events:{events})"));
        }
        pretty(&compact, &self.notes)
    }

    /// Writes the room file.
    pub fn save(&mut self) -> Result<()> {
        std::fs::write(&self.path, self.text())
            .with_context(|| format!("cannot write room file {}", self.path.display()))?;
        self.dirty = false;
        Ok(())
    }

    /// The fields of an item as a tree, and the name of its type for the table of
    /// its fields (`maki_core::FieldTypes`). `None`: the document has no such item,
    /// or the item has rows of its own (a spawn point, an event).
    pub fn tree(&self, item: &Item) -> Option<(&'static str, Value)> {
        let room = &self.room;
        Some(match item {
            Item::Room => ("room", room.shell().tree()),
            Item::Doorway(index) => ("doorway", room.doorways.get(*index)?.tree()),
            Item::Door(index) => ("door item", room.doors.get(*index)?.tree()),
            // Its name, pose, and parent. `component_tree` gives each component.
            Item::Object(index) => ("object", room.objects.get(*index)?.tree()),
            Item::Prop(index) => ("prop item", room.props.get(*index)?.tree()),
            Item::Mover(index) => ("mover item", room.movers.get(*index)?.tree()),
            Item::Light(index) => ("light item", room.lights.get(*index)?.tree()),
            Item::Prefab(index) => ("prefab", room.prefabs.get(*index)?.tree()),
            Item::Spawn(_) | Item::Event(_) => return None,
        })
    }

    /// Reads a changed tree of [`Document::tree`] back into its item. An error: the
    /// tree is not one of the item, and the item is as before.
    pub fn set_tree(&mut self, item: &Item, tree: Value) -> Result<()> {
        fn set<T: serde::de::DeserializeOwned>(slot: Option<&mut T>, tree: Value) -> Result<()> {
            *slot.context("the room has no such item")? = read(tree)?;
            Ok(())
        }
        let room = &mut self.room;
        match item {
            Item::Room => room.set_shell(read(tree)?),
            Item::Doorway(index) => set(room.doorways.get_mut(*index), tree)?,
            Item::Door(index) => set(room.doors.get_mut(*index), tree)?,
            Item::Object(index) => {
                let object = room
                    .objects
                    .get_mut(*index)
                    .context("the room has no such item")?;
                // The tree has no components: they stay.
                let components = std::mem::take(&mut object.components);
                let new: Result<ObjectDef> = read(tree);
                object.components = components;
                let new = new?;
                (object.name, object.pose, object.parent) = (new.name, new.pose, new.parent);
            }
            Item::Prop(index) => set(room.props.get_mut(*index), tree)?,
            Item::Mover(index) => set(room.movers.get_mut(*index), tree)?,
            Item::Light(index) => set(room.lights.get_mut(*index), tree)?,
            Item::Prefab(index) => set(room.prefabs.get_mut(*index), tree)?,
            Item::Spawn(_) | Item::Event(_) => return Err(anyhow!("the item has no tree")),
        }
        Ok(())
    }

    /// The tree of the component `name` of object `index`, from its text through
    /// the registry. `None`: no such component. `Err`: no type of the name, or the
    /// type does not read the text.
    pub fn component_tree(
        &self,
        index: usize,
        name: &str,
        types: &ComponentTypes,
    ) -> Option<Result<Value, String>> {
        let text = self.room.objects.get(index)?.components.get(name)?;
        let Some(component_type) = types.get(name) else {
            return Some(Err(format!("no module registered the type \"{name}\"")));
        };
        Some((component_type.tree)(text))
    }

    /// Writes a changed tree of [`Document::component_tree`] back as the text of its
    /// component. An error: the component is as before.
    pub fn set_component_tree(
        &mut self,
        index: usize,
        name: &str,
        tree: &Value,
        types: &ComponentTypes,
    ) -> Result<()> {
        let component_type = types
            .get(name)
            .with_context(|| format!("no module registered the type \"{name}\""))?;
        let text = (component_type.text)(tree).map_err(|error| anyhow!(error))?;
        let object = self
            .room
            .objects
            .get_mut(index)
            .context("the room has no such object")?;
        let slot = object
            .components
            .get_mut(name)
            .context("the object has no such component")?;
        *slot = text;
        Ok(())
    }

    /// Moves the props and the lights of the old lists into the list of objects,
    /// each with its notes, behind the objects that the room has. For the migration
    /// of stage 48.
    pub fn items_to_objects(&mut self) {
        // A doorway and its door are one object, with the notes of the two.
        let doorways = std::mem::take(&mut self.room.doorways);
        let mut doors = Vec::new();
        for (index, door) in std::mem::take(&mut self.room.doors).into_iter().enumerate() {
            doors.push(Some((door, self.notes.of("doors", index).to_vec())));
            self.notes.set("doors", index, Vec::new());
        }
        let mut objects = Vec::new();
        for (index, doorway) in doorways.into_iter().enumerate() {
            let mut object = ObjectDef::default();
            object.set_component("doorway", &doorway);
            let of_doorway = |door: &&mut Option<(DoorDef, Vec<String>)>| {
                door.as_ref().is_some_and(|(door, _)| door.doorway == index)
            };
            if let Some((door, notes)) = doors.iter_mut().find(of_doorway).and_then(Option::take) {
                object.name = door.name.clone();
                object.set_component("door", &door.leaf());
                let mut all = self.notes.of("doorways", index).to_vec();
                all.extend(notes);
                self.notes.set("doorways", index, all);
            }
            objects.push(object);
        }
        self.move_to_objects("doorways", objects.into_iter());
        let movers = std::mem::take(&mut self.room.movers);
        self.move_to_objects("movers", movers.into_iter().map(MoverDef::into_object));
        let props = std::mem::take(&mut self.room.props);
        self.move_to_objects("props", props.into_iter().map(PropDef::into_object));
        let lights = std::mem::take(&mut self.room.lights);
        self.move_to_objects("lights", lights.into_iter().map(LightDef::into_object));
    }

    fn move_to_objects(&mut self, list: &str, objects: impl Iterator<Item = ObjectDef>) {
        for (index, object) in objects.enumerate() {
            let notes = self.notes.of(list, index).to_vec();
            self.notes.set(list, index, Vec::new());
            self.notes.set("objects", self.room.objects.len(), notes);
            self.room.objects.push(object);
        }
    }

    /// The light of an item: a light of the old list, or an object with the
    /// component `light`.
    pub fn light(&self, item: &Item) -> Option<LightDef> {
        match item {
            Item::Light(index) => self.room.lights.get(*index).cloned(),
            Item::Object(index) => self.room.objects.get(*index)?.light()?.ok(),
            _ => None,
        }
    }

    /// The doorway of an item: of the old list, or an object with a `doorway`.
    pub fn doorway(&self, item: &Item) -> Option<Doorway> {
        match item {
            Item::Doorway(index) => self.room.doorways.get(*index).copied(),
            Item::Object(index) => self.room.objects.get(*index)?.doorway()?.ok(),
            _ => None,
        }
    }

    /// Writes the doorway of an item that [`Document::doorway`] gave.
    pub fn set_doorway(&mut self, item: &Item, doorway: Doorway) {
        match item {
            Item::Doorway(index) => self.room.doorways[*index] = doorway,
            Item::Object(index) => self.room.objects[*index].set_component("doorway", &doorway),
            _ => {}
        }
    }

    /// The mover of an item: of the old list, or an object with a `mover`.
    pub fn mover(&self, item: &Item) -> Option<MoverDef> {
        match item {
            Item::Mover(index) => self.room.movers.get(*index).cloned(),
            Item::Object(index) => self.room.objects.get(*index)?.mover()?.ok(),
            _ => None,
        }
    }

    /// Changes the second pose of a mover. False: the item is not a mover.
    pub fn with_mover_b(&mut self, item: &Item, change: impl FnOnce(&mut PoseDef)) -> bool {
        let Some(mut mover) = self.mover(item) else {
            return false;
        };
        change(&mut mover.b);
        match item {
            Item::Mover(index) => self.room.movers[*index] = mover,
            Item::Object(index) => self.room.objects[*index].set_mover(mover),
            _ => unreachable!("`mover` gives a mover of these two only"),
        }
        true
    }

    /// Writes the light of an item that [`Document::light`] gave.
    pub fn set_light(&mut self, item: &Item, light: LightDef) {
        match item {
            Item::Light(index) => self.room.lights[*index] = light,
            Item::Object(index) => self.room.objects[*index].set_light(light),
            _ => {}
        }
    }

    /// Where an item is, to change it. `second`: for a mover, its pose `b`. `None`:
    /// the item has no place of its own (the room, a doorway, a door).
    pub fn place(&mut self, item: &Item, second: bool) -> Option<Place<'_>> {
        let room = &mut self.room;
        match item {
            Item::Object(index) => room
                .objects
                .get_mut(*index)
                .filter(|object| !object.components.contains_key("doorway"))
                .map(|object| Place::Pose(&mut object.pose)),
            Item::Prop(index) => room
                .props
                .get_mut(*index)
                .map(|prop| Place::Pose(&mut prop.pose)),
            Item::Mover(index) => {
                let mover = room.movers.get_mut(*index)?;
                Some(Place::Pose(if second {
                    &mut mover.b
                } else {
                    &mut mover.a
                }))
            }
            Item::Light(index) => room
                .lights
                .get_mut(*index)
                .map(|light| Place::Point(&mut light.position)),
            Item::Prefab(index) => room
                .prefabs
                .get_mut(*index)
                .map(|prefab| Place::Pose(&mut prefab.pose)),
            Item::Spawn(name) => room.spawns.get_mut(name).map(Place::Point),
            Item::Event(index) => {
                let event = self.events.get_mut(*index)?;
                event.area.as_mut().map(Place::Area)
            }
            Item::Room | Item::Doorway(_) | Item::Door(_) => None,
        }
    }

    /// The origin and the rotation of an item. `None` as for [`Document::place`], and
    /// for an event with no area.
    pub fn pose_of(&self, item: &Item, second: bool) -> Option<(Vec3, Quat)> {
        let room = &self.room;
        let of_pose = |pose: &PoseDef| (Vec3::from(pose.position), pose.transform().rotation);
        let of_point = |point: &[f32; 3]| (Vec3::from(*point), Quat::IDENTITY);
        match item {
            Item::Object(index) => {
                let object = room.objects.get(*index)?;
                // A doorway has its place in the wall, not a pose.
                if object.components.contains_key("doorway") {
                    return None;
                }
                match object.mover() {
                    Some(Ok(mover)) if second => Some(of_pose(&mover.b)),
                    _ => Some(of_pose(&object.pose)),
                }
            }
            Item::Prop(index) => room.props.get(*index).map(|prop| of_pose(&prop.pose)),
            Item::Mover(index) => {
                let mover = room.movers.get(*index)?;
                Some(of_pose(if second { &mover.b } else { &mover.a }))
            }
            Item::Light(index) => room
                .lights
                .get(*index)
                .map(|light| of_point(&light.position)),
            Item::Prefab(index) => room.prefabs.get(*index).map(|prefab| of_pose(&prefab.pose)),
            Item::Spawn(name) => room.spawns.get(name).map(of_point),
            Item::Event(index) => {
                let [x, z] = self.events.get(*index)?.area?.center;
                Some((Vec3::new(x, 0.0, z), Quat::IDENTITY))
            }
            Item::Room | Item::Doorway(_) | Item::Door(_) => None,
        }
    }

    /// The list of the notes that an item has its notes in, and its index there.
    /// `None` for the room, and for an item that the document does not have.
    pub fn notes_key(&self, item: &Item) -> Option<(&'static str, usize)> {
        Some(match item {
            Item::Room => return None,
            Item::Doorway(index) => ("doorways", *index),
            Item::Door(index) => ("doors", *index),
            Item::Object(index) => ("objects", *index),
            Item::Prop(index) => ("props", *index),
            Item::Mover(index) => ("movers", *index),
            Item::Light(index) => ("lights", *index),
            Item::Prefab(index) => ("prefabs", *index),
            // The file has the spawn points in the order of their names.
            Item::Spawn(name) => (
                "spawns",
                self.room.spawns.keys().position(|key| key == name)?,
            ),
            Item::Event(index) => ("events", *index),
        })
    }

    /// The notes of an item: the comment lines before it in the file.
    pub fn notes_of(&self, item: &Item) -> &[String] {
        match self.notes_key(item) {
            Some((list, index)) => self.notes.of(list, index),
            None if *item == Item::Room => &self.notes.room,
            None => &[],
        }
    }

    pub fn set_notes(&mut self, item: &Item, lines: Vec<String>) {
        match self.notes_key(item) {
            Some((list, index)) => self.notes.set(list, index, lines),
            None if *item == Item::Room => self.notes.room = lines,
            None => {}
        }
    }

    /// Gives a spawn point another name. Its notes go with it. False, with no change,
    /// if the new name is empty or in use.
    pub fn rename_spawn(&mut self, old: &str, new: &str) -> bool {
        if new.is_empty() || self.room.spawns.contains_key(new) {
            return false;
        }
        let item = Item::Spawn(old.to_owned());
        let Some((list, index)) = self.notes_key(&item) else {
            return false;
        };
        let notes = self.notes.of(list, index).to_vec();
        self.notes.shift(list, index, -1);
        let position = self.room.spawns.remove(old).expect("the key was found");
        self.room.spawns.insert(new.to_owned(), position);
        let item = Item::Spawn(new.to_owned());
        let (list, index) = self.notes_key(&item).expect("put in above");
        self.notes.shift(list, index, 1);
        self.notes.set(list, index, notes);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_room_is_written_and_read_back() {
        let registry = CommandRegistry::default();
        let mut document = Document::new_room(PathBuf::from("new.ron"));
        let text = document.text();
        assert!(
            text.starts_with(
                "(
    width: 8.0,
"
            ),
            "{text}"
        );
        document.room.spawns.insert("b".to_owned(), [1.0, 0.0, 0.0]);
        document.room.spawns.insert("d".to_owned(), [2.0, 0.0, 0.0]);
        document.set_notes(
            &Item::Spawn("d".to_owned()),
            vec!["At the door.".to_owned()],
        );
        document.set_notes(&Item::Room, vec!["A test room.".to_owned()]);

        // The spawn point is first in the file with its new name.
        assert!(!document.rename_spawn("d", "b"));
        assert!(document.rename_spawn("d", "a"));
        let text = document.text();
        assert!(text.starts_with("// A test room.\n("), "{text}");
        assert!(
            text.contains("        // At the door.\n        \"a\": (2.0, 0.0, 0.0),"),
            "{text}"
        );
        assert_eq!(maki_scene::parse_room(&text).unwrap(), document.room);
        assert_eq!(registry.parse_events(&text).unwrap(), document.events);
        assert_eq!(read_notes(&text), document.notes);
    }
}
