//! What an event is: data, read from the room file.
//!
//! An event belongs to a room and has pages. Each page has conditions, a trigger, and
//! a list of commands. The active page of an event is the last page in the list whose
//! conditions all hold, so a later page replaces an earlier one when the game state
//! changes (a locked door becomes an open one). An event with no active page does
//! nothing.

use serde::{Deserialize, Serialize};

use crate::GameState;
use crate::command::Command;

/// One event of a room.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventDef {
    /// For the debug panel and the log.
    pub name: String,
    /// Where the event is, for the triggers `Interact` and `Touch`.
    #[serde(default, skip_serializing_if = "Area::is_empty")]
    pub area: Area,
    pub pages: Vec<Page>,
}

/// A rectangle on the floor, with sides along X and Z.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Area {
    /// X and Z of the center, in meters.
    pub center: [f32; 2],
    /// Length along X and along Z.
    pub size: [f32; 2],
}

impl Area {
    /// True for the default: an event with no place. Such an area is not written to
    /// a room file.
    fn is_empty(&self) -> bool {
        *self == Area::default()
    }

    /// The distance on the floor from a point to the rectangle. 0.0 in it.
    pub fn distance(&self, x: f32, z: f32) -> f32 {
        let beside = |at: f32, axis: usize| {
            ((at - self.center[axis]).abs() - self.size[axis] / 2.0).max(0.0)
        };
        beside(x, 0).hypot(beside(z, 1))
    }

    /// The point of the rectangle that is nearest to a point: X and Z.
    pub fn nearest(&self, x: f32, z: f32) -> [f32; 2] {
        let clamp = |at: f32, axis: usize| {
            let half = self.size[axis] / 2.0;
            at.clamp(self.center[axis] - half, self.center[axis] + half)
        };
        [clamp(x, 0), clamp(z, 1)]
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    /// All must hold. None: the page is always possible.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
    pub trigger: Trigger,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<Command>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Condition {
    /// The flag is set.
    Flag(String),
    /// The flag is not set.
    NotFlag(String),
    /// The variable has this value or a larger one.
    VariableAtLeast(String, i32),
    VariableEquals(String, i32),
}

impl Condition {
    pub fn holds(&self, state: &GameState) -> bool {
        match self {
            Condition::Flag(name) => state.flag(name),
            Condition::NotFlag(name) => !state.flag(name),
            Condition::VariableAtLeast(name, value) => state.variable(name) >= *value,
            Condition::VariableEquals(name, value) => state.variable(name) == *value,
        }
    }
}

/// What starts the commands of a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Trigger {
    /// The player is at the area of the event, faces it, and presses interact.
    Interact,
    /// The player walks into the area of the event.
    Touch,
    /// The page becomes the active page (also when the room loads). Runs one time.
    Autorun,
    /// As `Autorun`, but the commands start again each time they end, and the player
    /// keeps control.
    Parallel,
}

/// The index of the active page: the last one whose conditions hold.
pub fn active_page(event: &EventDef, state: &GameState) -> Option<usize> {
    let holds = |page: &Page| {
        page.conditions
            .iter()
            .all(|condition| condition.holds(state))
    };
    event.pages.iter().rposition(holds)
}

/// The part of a room file that this crate reads. The fields of the scene crate are
/// skipped. Read with `CommandRegistry::parse_events`.
#[derive(Deserialize)]
pub(crate) struct RoomFile {
    #[serde(default)]
    pub events: Vec<EventDef>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Builtin, CommandRegistry};

    fn page(conditions: &[Condition]) -> Page {
        Page {
            conditions: conditions.to_vec(),
            trigger: Trigger::Interact,
            commands: Vec::new(),
        }
    }

    fn event(pages: Vec<Page>) -> EventDef {
        EventDef {
            name: "test".to_owned(),
            area: Area::default(),
            pages,
        }
    }

    #[test]
    fn the_active_page_is_the_last_one_whose_conditions_hold() {
        let flag = |name: &str| Condition::Flag(name.to_owned());
        let door = event(vec![
            page(&[]),
            page(&[flag("key")]),
            page(&[
                flag("key"),
                Condition::VariableAtLeast("turns".to_owned(), 2),
            ]),
        ]);
        let mut state = GameState::default();
        assert_eq!(active_page(&door, &state), Some(0));
        state.set_flag("key", true);
        assert_eq!(active_page(&door, &state), Some(1));
        // All conditions of a page must hold.
        state.set_variable("turns", 1);
        assert_eq!(active_page(&door, &state), Some(1));
        state.set_variable("turns", 2);
        assert_eq!(active_page(&door, &state), Some(2));
        // A later page that does not hold lets an earlier one be active.
        state.set_flag("key", false);
        assert_eq!(active_page(&door, &state), Some(0));

        // No page holds: the event does nothing.
        let hidden = event(vec![page(&[Condition::NotFlag("gone".to_owned())])]);
        assert_eq!(active_page(&hidden, &state), Some(0));
        state.set_flag("gone", true);
        assert_eq!(active_page(&hidden, &state), None);
        assert_eq!(active_page(&event(Vec::new()), &state), None);
        // A variable that was never set is 0.
        let zero = event(vec![page(&[Condition::VariableEquals(
            "new".to_owned(),
            0,
        )])]);
        assert_eq!(active_page(&zero, &state), Some(0));
    }

    #[test]
    fn events_are_read_from_a_room_file_and_the_rest_is_skipped() {
        let text = r#"(
            width: 4, depth: 4, height: 3,
            props: [(mesh: Cube(1.0))],
            events: [
                (
                    name: "door",
                    area: (center: (3.0, -3.3), size: (1.0, 0.8)),
                    pages: [
                        (trigger: Interact, commands: [Message("Locked.")]),
                        (
                            conditions: [Flag("power")],
                            trigger: Interact,
                            commands: [
                                If("seen", [Wait(0.5)], [SetFlag("seen", true)]),
                                OpenDoor("door"),
                                Transfer("room_b.ron", "west"),
                            ],
                        ),
                    ],
                ),
            ],
        )"#;
        let registry = CommandRegistry::default();
        registry.add(crate::builtin::run);
        let events = registry.parse_events(text).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].area.size, [1.0, 0.8]);
        assert_eq!(events[0].pages[1].commands.len(), 3);
        assert_eq!(
            events[0].pages[0].commands[0].get(),
            Some(&Builtin::Message("Locked.".to_owned()))
        );
        // The scene crate reads the same text.
        assert!(maki_scene::parse_room(text).is_ok());
        // A room with no events.
        let none = registry.parse_events("(width: 4, depth: 4, height: 3)");
        assert_eq!(none.unwrap(), []);
        // A command that does not exist: the error has the line.
        let bad = "(\n events: [(name: \"x\", pages: [(trigger: Touch,\n commands: [Explode])])])";
        let error = registry.validate_events(bad).unwrap_err();
        assert!(
            error.starts_with("3:") && error.contains("Explode"),
            "{error}"
        );
        // A bad argument of a command that exists.
        let bad =
            "(\n events: [(name: \"x\", pages: [(trigger: Touch,\n commands: [Wait(\"long\")])])])";
        let error = registry.validate_events(bad).unwrap_err();
        assert!(error.starts_with("3:"), "{error}");

        // A command that code makes is the command that the file gives.
        let made = registry.command(Builtin::Message("Locked.".to_owned()));
        assert_eq!(made.as_ref(), Some(&events[0].pages[0].commands[0]));
        assert!(registry.command(crate::Trigger::Touch).is_none());

        // What is written is read as the same events: the editor saves room files.
        let written = ron::to_string(&events).unwrap();
        let read = registry
            .parse_events(&format!("(events: {written})"))
            .unwrap();
        assert_eq!(read, events);
    }

    #[test]
    fn the_distance_to_an_area_is_zero_in_it() {
        let area = Area {
            center: [2.0, -1.0],
            size: [2.0, 1.0],
        };
        assert_eq!(area.distance(2.5, -1.2), 0.0);
        assert_eq!(area.distance(4.0, -1.0), 1.0);
        // From a corner: 3 along X and 4 along Z.
        assert_eq!(area.distance(6.0, 3.5), 5.0);
        assert_eq!(area.nearest(6.0, 3.5), [3.0, -0.5]);
    }
}
