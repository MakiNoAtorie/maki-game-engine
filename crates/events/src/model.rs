//! What an event is: data, read from the room file.
//!
//! An event is a component of an object (`event`, [`EventPages`]), as a script is. It
//! has pages. Each page has conditions, a trigger, and a list of commands. The active
//! page of an event is the last page in the list whose conditions all hold, so a later
//! page replaces an earlier one when the game state changes (a locked door becomes an
//! open one). An event with no active page does nothing.
//!
//! The name of an event is the name of its object. Its area is the component `area`
//! of the object ([`TriggerArea`]) at the pose of the object: the same area as for the
//! hooks of a script.

use std::fmt;

use bevy_ecs::component::Component;
use maki_scene::PoseDef;
use serde::de::{IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::GameState;
use crate::command::Command;
use crate::script::TriggerArea;

/// The component `event` of an object: the pages of its event, and its wires. The
/// commands are an open set: read the text with `CommandRegistry::parse_pages`.
#[derive(Component, Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventPages {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pages: Vec<Page>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wires: Vec<Wire>,
}

impl EventPages {
    /// A command list by its index: the pages, then the wires. The index of a
    /// command list in the problems (`Problem::command`) and in a tool.
    pub fn list_mut(&mut self, index: usize) -> Option<&mut Vec<Command>> {
        let pages = self.pages.iter_mut().map(|page| &mut page.commands);
        let mut lists = pages.chain(self.wires.iter_mut().map(|wire| &mut wire.commands));
        lists.nth(index)
    }
}

/// A wire, as in Source: when an output of the object is sent (the area: `used`,
/// `entered`, `left`; a script or the row `Signal`: its own), the commands run. A
/// command is the input: `OpenDoor("door")`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wire {
    pub output: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<Command>,
}

impl Wire {
    /// The name of wire `index` for a person: its output; with the index from 1 too
    /// if another wire has the same output ("used 1", "used 2").
    pub fn label(wires: &[Wire], index: usize) -> String {
        let output = &wires[index].output;
        let same = |wire: &&Wire| wire.output == *output;
        if wires.iter().filter(same).count() < 2 {
            return output.clone();
        }
        let nth = wires[..index].iter().filter(same).count();
        format!("{output} {}", nth + 1)
    }
}

/// One event of a room, as the checks and the tools see it: the component `event` of
/// an object, with the name and the area of the object.
#[derive(Debug, Clone, PartialEq)]
pub struct EventDef {
    /// The name of the object. Empty: it has none.
    pub name: String,
    /// The index of the object in the list `objects` of the room.
    pub object: usize,
    /// Where the event is, for the triggers `Interact` and `Touch`. `None`: the object
    /// has no `area`, and no player starts the event (`Autorun`, `Parallel`).
    pub area: Option<Area>,
    pub pages: Vec<Page>,
    pub wires: Vec<Wire>,
}

impl EventDef {
    /// The area of an object with the component `area`, at its position.
    pub fn area_at(area: &TriggerArea, position: [f32; 3]) -> Area {
        Area {
            center: [position[0], position[2]],
            size: [area.width, area.depth],
        }
    }
}

/// A rectangle on the floor, with sides along X and Z.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Area {
    /// X and Z of the center, in meters.
    pub center: [f32; 2],
    /// Length along X and along Z.
    pub size: [f32; 2],
}

impl Area {
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
    /// The variable, compared with a number: `Variable("count", AtLeast, 3)`.
    Variable(String, Compare, i32),
}

/// How a variable is compared with a number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Compare {
    Equals,
    NotEquals,
    Less,
    AtLeast,
}

impl Condition {
    pub fn holds(&self, state: &GameState) -> bool {
        match self {
            Condition::Flag(name) => state.flag(name),
            Condition::NotFlag(name) => !state.flag(name),
            Condition::Variable(name, compare, value) => {
                let variable = state.variable(name);
                match compare {
                    Compare::Equals => variable == *value,
                    Compare::NotEquals => variable != *value,
                    Compare::Less => variable < *value,
                    Compare::AtLeast => variable >= *value,
                }
            }
        }
    }

    /// The name of the flag or the variable that the condition reads.
    pub fn name(&self) -> &str {
        match self {
            Condition::Flag(name) | Condition::NotFlag(name) | Condition::Variable(name, ..) => {
                name
            }
        }
    }

    /// True: the name is of a variable. False: of a flag.
    pub fn is_variable(&self) -> bool {
        matches!(self, Condition::Variable(..))
    }
}

/// True if each condition of the list holds. A list with no condition holds.
pub fn all_hold(conditions: &[Condition], state: &GameState) -> bool {
    conditions.iter().all(|condition| condition.holds(state))
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

/// The index of the active page of an event: the last one whose conditions hold.
pub fn active_page(pages: &[Page], state: &GameState) -> Option<usize> {
    let holds = |page: &Page| all_hold(&page.conditions, state);
    pages.iter().rposition(holds)
}

/// Events in a short form, `(name: "x", area: (center: (x, z), size: (x, z)), pages:
/// [...])`, as the objects of a room file: an object with the name, the center of the
/// area as its position, the components `area` and `event`. For tests that write a
/// room. Panics if the text is not of that form.
pub fn short_events(events: &str) -> String {
    #[derive(serde::Deserialize)]
    struct Short {
        name: String,
        #[serde(default)]
        area: Option<Area>,
        pages: Box<ron::value::RawValue>,
    }
    let events: Vec<Short> = maki_scene::ron_options()
        .from_str(&format!("[{events}]"))
        .unwrap();
    let object = |event: &Short| {
        let (position, area) = match event.area {
            Some(Area { center, size }) => (
                format!("({}, 0.0, {})", center[0], center[1]),
                format!(r#""area": (width: {}, depth: {}), "#, size[0], size[1]),
            ),
            None => ("(0.0, 0.0, 0.0)".to_owned(), String::new()),
        };
        let (name, pages) = (&event.name, event.pages.get_ron());
        format!(
            r#"(name: "{name}", pose: (position: {position}), components: {{{area}"event": (pages: {pages})}})"#
        )
    };
    events.iter().map(object).collect::<Vec<_>>().join(",\n")
}

/// The part of a room file that this crate reads: of each object, its name, its pose,
/// and the components `area` and `event`. The rest is skipped. Read with
/// `CommandRegistry::parse_events`.
#[derive(Deserialize)]
pub(crate) struct RoomFile {
    #[serde(default)]
    pub objects: Vec<RoomObject>,
}

#[derive(Deserialize)]
pub(crate) struct RoomObject {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub pose: PoseDef,
    #[serde(default)]
    pub components: EventComponents,
}

/// The components of an object that an event needs.
#[derive(Default)]
pub(crate) struct EventComponents {
    pub area: Option<TriggerArea>,
    pub event: Option<EventPages>,
}

impl RoomFile {
    /// The events of the objects, in the order of the objects.
    pub(crate) fn events(self) -> Vec<EventDef> {
        let objects = self.objects.into_iter().enumerate();
        let event = |(index, object): (usize, RoomObject)| {
            let EventPages { pages, wires } = object.components.event?;
            let area = object.components.area;
            Some(EventDef {
                name: object.name.unwrap_or_default(),
                object: index,
                area: area.map(|area| EventDef::area_at(&area, object.pose.position)),
                pages,
                wires,
            })
        };
        objects.filter_map(event).collect()
    }
}

// A map of component names to values of many types: only two are read, the others
// are skipped as they are. The reader stays in the text, so an error has its line.
impl<'de> Deserialize<'de> for EventComponents {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Components;

        impl<'de> Visitor<'de> for Components {
            type Value = EventComponents;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                write!(formatter, "the components of an object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<EventComponents, A::Error> {
                let mut components = EventComponents::default();
                while let Some(name) = map.next_key::<String>()? {
                    match name.as_str() {
                        "area" => components.area = Some(map.next_value()?),
                        "event" => {
                            crate::command::event_starts()?;
                            components.event = Some(map.next_value()?);
                        }
                        _ => drop(map.next_value::<IgnoredAny>()?),
                    }
                }
                Ok(components)
            }
        }

        deserializer.deserialize_map(Components)
    }
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

    /// The pages of an event.
    fn event(pages: Vec<Page>) -> Vec<Page> {
        pages
    }

    #[test]
    fn the_active_page_is_the_last_one_whose_conditions_hold() {
        let flag = |name: &str| Condition::Flag(name.to_owned());
        let door = event(vec![
            page(&[]),
            page(&[flag("key")]),
            page(&[
                flag("key"),
                Condition::Variable("turns".to_owned(), Compare::AtLeast, 2),
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
        let variable = |compare, value| {
            let condition = Condition::Variable("new".to_owned(), compare, value);
            active_page(&event(vec![page(&[condition])]), &state).is_some()
        };
        assert!(variable(Compare::Equals, 0) && !variable(Compare::NotEquals, 0));
        assert!(variable(Compare::Less, 1) && !variable(Compare::Less, 0));
        assert!(variable(Compare::AtLeast, 0) && !variable(Compare::AtLeast, 1));
    }

    #[test]
    fn events_are_read_from_the_objects_of_a_room_file() {
        let text = r#"(
            width: 4, depth: 4, height: 3,
            objects: [
                (components: {"prop": (mesh: Cube(1.0))}),
                (
                    name: "door",
                    pose: (position: (3.0, 0.0, -3.3)),
                    components: {
                        "area": (width: 1.0, depth: 0.8),
                        "event": (pages: [
                            (trigger: Interact, commands: [Message("Locked.")]),
                            (
                                conditions: [Flag("power")],
                                trigger: Interact,
                                commands: [
                                    If([Flag("seen"), Variable("turns", Less, 3)], [Wait(0.5)], [SetFlag("seen", true)]),
                                    OpenDoor("door"),
                                    Transfer("room_b.ron", "west"),
                                ],
                            ),
                        ]),
                    },
                ),
                (name: "timer", components: {"event": (pages: [(trigger: Autorun)])}),
            ],
        )"#;
        let registry = CommandRegistry::default();
        registry.add(crate::builtin::run);
        let events = registry.parse_events(text).unwrap();
        assert_eq!(events.len(), 2);
        // The name of the object, and its area at the pose of the object.
        assert_eq!((events[0].name.as_str(), events[0].object), ("door", 1));
        assert_eq!(
            events[0].area,
            Some(Area {
                center: [3.0, -3.3],
                size: [1.0, 0.8]
            })
        );
        assert_eq!(events[0].pages[1].commands.len(), 3);
        assert_eq!(
            events[0].pages[0].commands[0].get(),
            Some(&Builtin::Message("Locked.".to_owned()))
        );
        // An object with no area: an event that no player walks to.
        assert_eq!((events[1].object, events[1].area), (2, None));
        // The scene crate reads the same text.
        assert!(maki_scene::parse_room(text).is_ok());
        // A room with no events.
        let none = registry.parse_events("(width: 4, depth: 4, height: 3)");
        assert_eq!(none.unwrap(), []);
        // A command that does not exist: the error has the line.
        let bad = "(\n objects: [(name: \"x\", components: {\"event\": (pages: [(trigger: Touch,\n commands: [Explode])])})])";
        let error = registry.parse_events(bad).unwrap_err().to_string();
        assert!(
            error.starts_with("3:") && error.contains("Explode"),
            "{error}"
        );
        // A bad argument of a command that exists.
        let bad = "(\n objects: [(components: {\"event\": (pages: [(trigger: Touch,\n commands: [Wait(\"long\")])])})])";
        let error = registry.parse_events(bad).unwrap_err().to_string();
        assert!(error.starts_with("3:"), "{error}");

        // A command that code makes is the command that the file gives.
        let made = registry.command(Builtin::Message("Locked.".to_owned()));
        assert_eq!(made.as_ref(), Some(&events[0].pages[0].commands[0]));
        assert!(registry.command(crate::Trigger::Touch).is_none());

        // The text of the component is read back as the same pages: the editor
        // writes it.
        let pages = EventPages {
            pages: events[0].pages.clone(),
            wires: Vec::new(),
        };
        let component = maki_scene::ComponentText::of(&pages);
        assert_eq!(registry.parse_pages(&component).unwrap(), pages);
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
