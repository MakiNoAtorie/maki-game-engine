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
    ComponentTypes, LightDef, MoverDef, ObjectDef, PoseDef, PrefabUse, RoomDef, RoomValidators,
    check_room, ron_options,
};

use crate::ron_text::{Notes, pretty, read_notes};

/// One thing of a room that the outliner lists and the property panel shows.
#[derive(Debug, Clone, PartialEq, Eq, Default, Hash)]
pub enum Item {
    /// The size and the textures of the room.
    #[default]
    Room,
    /// An object of the list `objects`.
    Object(usize),
    Event(usize),
}

/// The part of an item that tells where it is: what a gizmo changes.
pub enum Place<'a> {
    /// The pose of an object. The second pose of a mover: [`Document::with_mover_b`].
    Pose(&'a mut PoseDef),
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
        !matches!(self, Item::Room)
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
                objects: vec![
                    lamp.into_object(),
                    ObjectDef::spawn_point("start".to_owned(), [0.0; 3]),
                ],
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
    /// or the item has rows of its own (an event).
    pub fn tree(&self, item: &Item) -> Option<(&'static str, Value)> {
        let room = &self.room;
        Some(match item {
            Item::Room => ("room", room.shell().tree()),
            // Its name, pose, and parent. `component_tree` gives each component.
            Item::Object(index) => ("object", room.objects.get(*index)?.tree()),
            Item::Event(_) => return None,
        })
    }

    /// Reads a changed tree of [`Document::tree`] back into its item. An error: the
    /// tree is not one of the item, and the item is as before.
    pub fn set_tree(&mut self, item: &Item, tree: Value) -> Result<()> {
        let room = &mut self.room;
        match item {
            Item::Room => room.set_shell(read(tree)?),
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
            Item::Event(_) => return Err(anyhow!("the item has no tree")),
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
        let object = self
            .room
            .objects
            .get_mut(index)
            .context("the room has no such object")?;
        let slot = object
            .components
            .get_mut(name)
            .context("the object has no such component")?;
        *slot = (component_type.text)(slot, tree).map_err(|error| anyhow!(error))?;
        Ok(())
    }

    /// The object of an item. `None`: not an object, or not in the room.
    pub fn object(&self, item: &Item) -> Option<&ObjectDef> {
        match item {
            Item::Object(index) => self.room.objects.get(*index),
            _ => None,
        }
    }

    fn object_mut(&mut self, item: &Item) -> Option<&mut ObjectDef> {
        match item {
            Item::Object(index) => self.room.objects.get_mut(*index),
            _ => None,
        }
    }

    /// The light of an item: an object with the component `light`.
    pub fn light(&self, item: &Item) -> Option<LightDef> {
        self.object(item)?.light()?.ok()
    }

    /// Writes the light of an item that [`Document::light`] gave.
    pub fn set_light(&mut self, item: &Item, light: LightDef) {
        if let Some(object) = self.object_mut(item) {
            object.set_light(light);
        }
    }

    /// The doorway of an item: an object with the component `doorway`.
    pub fn doorway(&self, item: &Item) -> Option<Doorway> {
        self.object(item)?.doorway()?.ok()
    }

    /// Writes the doorway of an item that [`Document::doorway`] gave.
    pub fn set_doorway(&mut self, item: &Item, doorway: Doorway) {
        if let Some(object) = self.object_mut(item) {
            object.set_component("doorway", &doorway);
        }
    }

    /// The mover of an item: an object with the component `mover`.
    pub fn mover(&self, item: &Item) -> Option<MoverDef> {
        self.object(item)?.mover()?.ok()
    }

    /// Changes the second pose of a mover. False: the item is not a mover.
    pub fn with_mover_b(&mut self, item: &Item, change: impl FnOnce(&mut PoseDef)) -> bool {
        let Some(mut mover) = self.mover(item) else {
            return false;
        };
        change(&mut mover.b);
        let object = self.object_mut(item).expect("`mover` found it");
        object.set_mover(mover);
        true
    }

    /// The prefab use of an item: an object with the component `prefab`.
    pub fn prefab_use(&self, item: &Item) -> Option<PrefabUse> {
        self.object(item)?.prefab()?.ok()
    }

    /// Where an item is, to change it. `None`: the item has no place of its own (the
    /// room, a doorway).
    pub fn place(&mut self, item: &Item) -> Option<Place<'_>> {
        match item {
            Item::Object(index) => self
                .room
                .objects
                .get_mut(*index)
                .filter(|object| !object.components.contains_key("doorway"))
                .map(|object| Place::Pose(&mut object.pose)),
            Item::Event(index) => {
                let event = self.events.get_mut(*index)?;
                event.area.as_mut().map(Place::Area)
            }
            Item::Room => None,
        }
    }

    /// The origin and the rotation of an item. `second`: for a mover, its pose `b`.
    /// `None` as for [`Document::place`], and for an event with no area.
    pub fn pose_of(&self, item: &Item, second: bool) -> Option<(Vec3, Quat)> {
        let of_pose = |pose: &PoseDef| (Vec3::from(pose.position), pose.transform().rotation);
        match item {
            Item::Object(index) => {
                let object = self.room.objects.get(*index)?;
                // A doorway has its place in the wall, not a pose.
                if object.components.contains_key("doorway") {
                    return None;
                }
                match object.mover() {
                    Some(Ok(mover)) if second => Some(of_pose(&mover.b)),
                    _ => Some(of_pose(&object.pose)),
                }
            }
            Item::Event(index) => {
                let [x, z] = self.events.get(*index)?.area?.center;
                Some((Vec3::new(x, 0.0, z), Quat::IDENTITY))
            }
            Item::Room => None,
        }
    }

    /// The list of the notes that an item has its notes in, and its index there.
    /// `None` for the room, and for an item that the document does not have.
    pub fn notes_key(&self, item: &Item) -> Option<(&'static str, usize)> {
        Some(match item {
            Item::Room => return None,
            Item::Object(index) => ("objects", *index),
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
        // The spawn point of the new room is object 1.
        document.set_notes(&Item::Object(1), vec!["At the door.".to_owned()]);
        document.set_notes(&Item::Room, vec!["A test room.".to_owned()]);
        let text = document.text();
        assert!(text.starts_with("// A test room.\n("), "{text}");
        assert!(
            text.contains("        // At the door.\n        (name: \"start\""),
            "{text}"
        );
        assert_eq!(maki_scene::parse_room(&text).unwrap(), document.room);
        assert_eq!(registry.parse_events(&text).unwrap(), document.events);
        assert_eq!(read_notes(&text), document.notes);
    }
}
