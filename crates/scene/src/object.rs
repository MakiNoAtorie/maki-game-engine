//! The objects of a room file, and the registry of the types of their components.
//!
//! An object is a name, a pose, an optional parent, and components. In the file a
//! component is the name of its type and its value in the RON form of that type:
//!
//! ```ron
//! (name: "cabinet", pose: (position: (-3.2, 0.0, -3.7)), components: {"prop": (mesh: Cube(1.0))})
//! ```
//!
//! RON needs the type to read a value (`Cube(1.0)` has no meaning without its enum),
//! and a component of a script DLL that did not build has no type. So a component is
//! kept as its text ([`ComponentText`]), and is read into its type only where it is
//! used. A text that no module can read is written back as it was.

use std::collections::BTreeMap;
use std::iter::Peekable;
use std::str::Chars;

use bevy_ecs::component::Component;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::EntityCommands;
use maki_core::fields::{Value, read};
use maki_core::{App, Fields};
use ron::value::RawValue;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::room::{PoseDef, ron_options};

/// One thing of a room.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Fields)]
#[serde(deny_unknown_fields)]
#[fields(default)]
pub struct ObjectDef {
    /// Events and scripts find the object by its name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "is_default_pose")]
    pub pose: PoseDef,
    /// The name of the object that this one belongs to. The pose is in the space of
    /// the room all the same (stage 49 changes that).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// By the name of the type of each component.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[field(skip)]
    pub components: BTreeMap<String, ComponentText>,
}

impl ObjectDef {
    /// The component `name` read as its type. `None`: the object has no such
    /// component. `Err`: the text is not of that type.
    pub fn component<T: DeserializeOwned>(
        &self,
        name: &str,
    ) -> Option<Result<T, ron::error::SpannedError>> {
        Some(self.components.get(name)?.read())
    }

    /// Writes `value` as the component `name`.
    pub fn set_component<T: Serialize>(&mut self, name: &str, value: &T) {
        self.components
            .insert(name.to_owned(), ComponentText::of(value));
    }
}

fn is_default_pose(pose: &PoseDef) -> bool {
    *pose == PoseDef::default()
}

/// A component as compact RON text: no comments, no white space but in strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentText(String);

impl ComponentText {
    /// The compact RON text of a value.
    pub fn of<T: Serialize>(value: &T) -> Self {
        Self(
            ron_options()
                .to_string(value)
                .expect("a component has no value that RON cannot write"),
        )
    }

    /// A RON text, made compact. `Err`: the text is not RON.
    pub fn parse(text: &str) -> Result<Self, ron::error::SpannedError> {
        RawValue::from_ron(text)?;
        Ok(Self(compact(text)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The text read as its type.
    pub fn read<T: DeserializeOwned>(&self) -> Result<T, ron::error::SpannedError> {
        ron_options().from_str(&self.0)
    }
}

impl Serialize for ComponentText {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // A raw value: the serializer of RON writes the text as it is.
        RawValue::from_ron(&self.0)
            .expect("a component text is RON")
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ComponentText {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Box::<RawValue>::deserialize(deserializer)?;
        Ok(Self(compact(raw.get_ron())))
    }
}

/// The text with no comments and no white space but in strings, and with no comma
/// before a closing bracket.
// ponytail: raw strings (`r#"..."#`) are not known; a `"` in one ends the string here.
fn compact(text: &str) -> String {
    fn copy_quoted(chars: &mut Peekable<Chars>, quote: char, out: &mut String) {
        while let Some(c) = chars.next() {
            out.push(c);
            match c {
                '\\' => out.extend(chars.next()),
                c if c == quote => return,
                _ => {}
            }
        }
    }
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek().copied()) {
            ('"' | '\'', _) => {
                out.push(c);
                copy_quoted(&mut chars, c, &mut out);
            }
            ('/', Some('/')) => while chars.next().is_some_and(|c| c != '\n') {},
            ('/', Some('*')) => {
                chars.next();
                while let Some(c) = chars.next() {
                    if c == '*' && chars.peek() == Some(&'/') {
                        chars.next();
                        break;
                    }
                }
            }
            (c, _) if c.is_whitespace() => {}
            (')' | ']' | '}', _) => {
                if out.ends_with(',') {
                    out.pop();
                }
                out.push(c);
            }
            (c, _) => out.push(c),
        }
    }
    out
}

/// Reads the text of a component and inserts it into an entity. `Err`: why the type
/// does not read it.
pub type Insert = fn(&ComponentText, &mut EntityCommands) -> Result<(), String>;

/// A type of component that a tool can read and write as a tree.
pub struct ComponentType {
    /// The tree of a text. `Err`: why the type does not read it.
    pub tree: fn(&ComponentText) -> Result<Value, String>,
    /// The text of a tree over the old text of the component: a field that the tree
    /// does not have (`#[field(skip)]`) keeps its old value. `Err`: why the type does
    /// not read it.
    pub text: fn(&ComponentText, &Value) -> Result<ComponentText, String>,
    /// For a component of `bevy_ecs` ([`add_object_component`]): reads the text and
    /// inserts the value into the entity of the object. `None`: the room loader has
    /// its own code for the type (a prop, a light), or the type is data only.
    pub insert: Option<Insert>,
}

/// The types of components by name. [`add_component_type`] adds one.
#[derive(Resource, Default)]
pub struct ComponentTypes(BTreeMap<&'static str, ComponentType>);

impl ComponentTypes {
    pub fn get(&self, name: &str) -> Option<&ComponentType> {
        self.0.get(name)
    }
}

/// Registers a type of component under the name that the room files give it: its
/// table of fields (`maki_core::FieldTypes`, under the same name), and the way from
/// its text to its tree and back.
pub fn add_component_type<T: Fields + Serialize + DeserializeOwned>(
    app: &mut App,
    name: &'static str,
) {
    register::<T>(app, name, None);
}

/// Registers a type of component that is also a component of `bevy_ecs`: as
/// [`add_component_type`], and the room loader inserts the value into the entity of
/// the object (the object entity: see `Spawner::object`).
pub fn add_object_component<T: Component + Fields + Serialize + DeserializeOwned>(
    app: &mut App,
    name: &'static str,
) {
    register::<T>(
        app,
        name,
        Some(|text, entity| {
            entity.insert(text.read::<T>().map_err(|error| error.to_string())?);
            Ok(())
        }),
    );
}

/// Registers a type of component whose text needs more than its type to be read (the
/// pages of an event: commands of all modules). The room loader calls `insert`. It
/// has no table of fields: a tool shows it with a panel of its own.
pub fn add_component_reader(app: &mut App, name: &'static str, insert: Insert) {
    let component_type = ComponentType {
        tree: |_| Err("the type is edited in a panel of its own".to_owned()),
        text: |_, _| Err("the type is edited in a panel of its own".to_owned()),
        insert: Some(insert),
    };
    app.init_resource::<ComponentTypes>();
    app.resource_mut::<ComponentTypes>()
        .0
        .insert(name, component_type);
}

fn register<T: Fields + Serialize + DeserializeOwned>(
    app: &mut App,
    name: &'static str,
    insert: Option<Insert>,
) {
    maki_core::add_field_type::<T>(app, name);
    let component_type = ComponentType {
        tree: |text| match text.read::<T>() {
            Ok(value) => Ok(value.tree()),
            Err(error) => Err(error.to_string()),
        },
        text: |old, tree| {
            // The old value with each field of the tree in place of its own.
            let old = old
                .read::<T>()
                .ok()
                .and_then(|old| serde_json::to_value(old).ok());
            let full = match (old, tree) {
                (Some(Value::Object(mut fields)), Value::Object(new)) => {
                    fields.extend(new.clone());
                    Value::Object(fields)
                }
                _ => tree.clone(),
            };
            match read::<T>(full) {
                Ok(value) => Ok(ComponentText::of(&value)),
                Err(error) => Err(error.to_string()),
            }
        },
        insert,
    };
    app.init_resource::<ComponentTypes>();
    app.resource_mut::<ComponentTypes>()
        .0
        .insert(name, component_type);
}

#[cfg(test)]
mod tests {
    use maki_core::ConfigFile;

    use super::*;
    use crate::room::{LightDef, MeshDef, Prop, parse_room};

    const ROOM: &str = r#"(width: 4.0, depth: 4.0, height: 3.0, objects: [
        (name: "ball", pose: (position: (1.0, 0.5, 0.0)), components: {
            // A comment in a component.
            "prop": (mesh: Sphere(radius: 0.5, segments: 8, rings: 4), collider: false, ),
            "swing": (angle: 12.5, axis: Y, label: "a \"b\" // c"),
        }),
        (parent: "ball"),
    ])"#;

    #[test]
    fn an_object_keeps_each_component_as_its_text() {
        let room = parse_room(ROOM).unwrap();
        let ball = &room.objects[0];
        assert_eq!(ball.name.as_deref(), Some("ball"));
        assert_eq!(ball.pose.position, [1.0, 0.5, 0.0]);
        assert_eq!(room.objects[1].parent.as_deref(), Some("ball"));
        // No type reads "swing": its text stays, compact.
        assert_eq!(
            ball.components["swing"].as_str(),
            r#"(angle:12.5,axis:Y,label:"a \"b\" // c")"#
        );
        let prop: Prop = ball.components["prop"].read().unwrap();
        assert_eq!(
            prop.mesh,
            MeshDef::Sphere {
                radius: 0.5,
                segments: 8,
                rings: 4
            }
        );
        assert!(!prop.collider);

        // Written and read again: the same room, and the same text.
        let text = ron_options().to_string(&room).unwrap();
        assert!(
            text.contains(r#""swing":(angle:12.5,axis:Y,label:"a \"b\" // c")"#),
            "{text}"
        );
        let again = parse_room(&text).unwrap();
        assert_eq!(again, room);
        assert_eq!(ron_options().to_string(&again).unwrap(), text);
    }

    #[test]
    fn a_light_is_an_object_and_back() {
        let light = LightDef {
            name: Some("lamp".to_owned()),
            position: [1.0, 2.0, 3.0],
            radius: 0.1,
            color: [1.0, 0.5, 0.0],
            intensity: 4.0,
            on: false,
            direction: Some([0.0, -1.0, 0.0]),
            cone: Some(30.0),
            sound: None,
        };
        let mut object = light.clone().into_object();
        assert_eq!(object.pose.position, light.position);
        assert_eq!(object.light().unwrap().unwrap(), light);
        let moved = LightDef {
            position: [0.0; 3],
            intensity: 1.0,
            ..light
        };
        object.set_light(moved.clone());
        assert_eq!(object.light().unwrap().unwrap(), moved);
        assert_eq!(object.components.len(), 1);
    }

    #[test]
    fn a_registered_component_goes_to_its_tree_and_back() {
        let mut app = App::new(ConfigFile::default()).unwrap();
        add_component_type::<Prop>(&mut app, "prop");
        let types = app.world().resource::<ComponentTypes>();
        let prop = types.get("prop").unwrap();
        let room = parse_room(ROOM).unwrap();
        let text = &room.objects[0].components["prop"];
        let tree = (prop.tree)(text).unwrap();
        assert_eq!(tree["collider"], Value::Bool(false));
        // The text that the type writes reads as the same prop.
        let back = (prop.text)(text, &tree).unwrap();
        assert_eq!(back.read::<Prop>().unwrap(), text.read::<Prop>().unwrap());
        assert!((prop.tree)(&ComponentText::parse("(angle: 1.0)").unwrap()).is_err());
        assert!(types.get("swing").is_none());
    }
}
