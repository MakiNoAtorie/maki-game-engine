//! The document: the room that the editor has open. It is the content of one room
//! file: the room, its events, and its comments. The panels change the document, the
//! world is built from it, and a save writes it. The world is never written back.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use maki_events::{CommandRegistry, EventDef};
use maki_scene::{LightDef, RoomDef, RoomValidators, check_room, ron_options};

use crate::ron_text::{Notes, pretty, read_notes};

/// One thing of a room that the outliner lists and the property panel shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Item {
    /// The size and the textures of the room.
    #[default]
    Room,
    Doorway(usize),
    Door(usize),
    Prop(usize),
    Mover(usize),
    Light(usize),
    /// By name.
    Spawn(String),
    Event(usize),
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
        };
        Self {
            path,
            room: RoomDef {
                width: 8.0,
                depth: 8.0,
                height: 3.0,
                lights: vec![lamp],
                spawns: [("start".to_owned(), [0.0; 3])].into(),
                ..RoomDef::default()
            },
            events: Vec::new(),
            notes: Notes::default(),
            dirty: true,
        }
    }

    /// Reads a room file, with the checks that the game does when it loads the room.
    pub fn read(path: &Path, registry: &CommandRegistry, checks: &RoomValidators) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read room file {}", path.display()))?;
        let room = check_room(path, &text, checks)?;
        let events = registry
            .parse_events(&text)
            .map_err(|error| anyhow!("{}:{error}", path.display()))?;
        Ok(Self {
            path: path.to_owned(),
            room,
            events,
            notes: read_notes(&text),
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

    /// The list of the notes that an item has its notes in, and its index there.
    /// `None` for the room, and for an item that the document does not have.
    pub fn notes_key(&self, item: &Item) -> Option<(&'static str, usize)> {
        Some(match item {
            Item::Room => return None,
            Item::Doorway(index) => ("doorways", *index),
            Item::Door(index) => ("doors", *index),
            Item::Prop(index) => ("props", *index),
            Item::Mover(index) => ("movers", *index),
            Item::Light(index) => ("lights", *index),
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
