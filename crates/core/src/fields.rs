//! Fields that describe themselves: one mark on a type says what its fields are,
//! and a tool (the inspector of the editor) shows and changes them with no code
//! for that type.
//!
//! ```ignore
//! /// A lamp that swings.
//! #[derive(Fields, Serialize, Deserialize)]
//! #[fields(default)]
//! struct Swing {
//!     /// The largest angle, in degrees.
//!     #[field(range = 0.0..=90.0, step = 0.5)]
//!     angle: f32,
//!     #[field(label = "seconds for one swing")]
//!     seconds: f32,
//!     #[field(color, some = [1.0; 3])]
//!     tint: Option<[f32; 3]>,
//! }
//! ```
//!
//! The derive [`Fields`](macro@crate::Fields) writes two functions:
//!
//! - [`Fields::kind`]: the table. A struct is a [`Kind::Group`] of its fields; an
//!   enum is a [`Kind::Choice`] of its variants. A field has its name, a label, its
//!   doc comment as the help text, its kind, and its default.
//! - [`Fields::tree`]: the value as a tree ([`Value`]), with each field in it. A
//!   tool changes the tree, and `serde` reads the tree back into the type
//!   ([`read`]). So the names and the forms of a tree are those of `serde`: a
//!   struct is a map, a variant is `"Name"` or `{"Name": content}`.
//!
//! The hints of `#[field(...)]`:
//!
//! | Hint | On | What it says |
//! |---|---|---|
//! | `label = "..."` | a field, a variant | The label. With none: the name |
//! | `default = value` | a field | The value of "back to default" |
//! | `default = value` | a variant | The value when the variant is chosen |
//! | `skip` | a field | No tool sees it. `serde` must read the type with no such field |
//! | `range = 0.0..=1.0` | a number | The least and the largest value |
//! | `step = 0.05` | a number, a vector | The change of one step of a drag |
//! | `color` | `[f32; 3]` | Linear RGB, with a color button |
//! | `file = "model"` | a path | The group of files to pick from. The tool has the groups |
//! | `pick = "list"` | a number, a text | An index or a name of a list that the tool has |
//! | `some = value` | an `Option` | The value when it is turned on |
//!
//! `#[fields(default)]` on a struct: the default of each field is that of `Default`
//! of the struct.
//!
//! A module gives a type a name with [`add_field_type`]; a tool finds the table by
//! that name in [`FieldTypes`].

use std::collections::BTreeMap;
use std::path::PathBuf;

use bevy_ecs::resource::Resource;
use serde::de::DeserializeOwned;
pub use serde_json::{Map, Value};

use crate::App;

/// A type that says what its fields are. Write it with the derive of the same name.
#[diagnostic::on_unimplemented(
    message = "`{Self}` does not say what its fields are",
    note = "derive `Fields` for the type, or mark the field `#[field(skip)]`"
)]
pub trait Fields {
    /// What a tool shows for a value of this type.
    fn kind() -> Kind
    where
        Self: Sized;

    /// The value as a tree, with each field in it.
    fn tree(&self) -> Value;
}

/// What a tool shows for a value.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Number {
        /// The least and the largest value.
        range: Option<[f64; 2]>,
        /// The change of one step of a drag.
        step: f64,
        /// True: no fraction.
        whole: bool,
    },
    Switch,
    Text,
    /// X, Y, and Z.
    Vector {
        step: f64,
    },
    /// Linear RGB.
    Color,
    /// A file of this group: `"model"`, `"picture"`, `"sound"`. Empty: any file.
    File(&'static str),
    /// One of a list that the tool has under this name: its index (`text` is
    /// false) or its name.
    Pick {
        list: &'static str,
        text: bool,
    },
    /// An object of the room: its id (a number in the tree). A tool shows the name
    /// of the object and picks it by its name.
    Object,
    /// A value that can be absent: `null` in the tree.
    Optional {
        inner: Box<Kind>,
        /// The value when a tool turns it on. `None`: [`Kind::zero`] of `inner`.
        some: Option<Value>,
    },
    /// A struct.
    Group(Vec<Field>),
    /// An enum.
    Choice(Vec<Variant>),
}

/// One field of a struct, or of a variant of an enum.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// The key in the tree: the name of the field. `"0"`, `"1"` for a field with
    /// no name.
    pub name: &'static str,
    pub label: &'static str,
    /// The doc comment of the field.
    pub help: &'static str,
    pub kind: Kind,
    /// The value of "back to default".
    pub default: Option<Value>,
}

/// One variant of an enum.
#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub name: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    pub form: Form,
    pub fields: Vec<Field>,
    /// The tree of the variant when a tool chooses it. `None`: [`Variant::zero`]
    /// makes one.
    pub default: Option<Value>,
}

/// How a variant holds its fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// `Name`: no field. The tree is `"Name"`.
    Unit,
    /// `Name(a, b)`. The tree is `{"Name": a}` for one field, `{"Name": [a, b]}`
    /// for more.
    Tuple,
    /// `Name { a, b }`. The tree is `{"Name": {"a": a, "b": b}}`.
    Struct,
}

impl Kind {
    /// Changes the kind of the value: of the value in it, for an optional one.
    fn of_value(self, change: impl FnOnce(Kind) -> Kind) -> Kind {
        match self {
            Kind::Optional { inner, some } => Kind::Optional {
                inner: Box::new(change(*inner)),
                some,
            },
            kind => change(kind),
        }
    }

    // The hints of `#[field(...)]`: the derive calls them. A hint on a kind that
    // does not have it is an error of the code, and it shows at the start.

    pub fn range(self, min: f64, max: f64) -> Kind {
        self.of_value(|kind| match kind {
            Kind::Number { step, whole, .. } => Kind::Number {
                range: Some([min, max]),
                step,
                whole,
            },
            other => panic!("`range` is a hint of a number, not of {other:?}"),
        })
    }

    pub fn step(self, step: f64) -> Kind {
        self.of_value(|kind| match kind {
            Kind::Number { range, whole, .. } => Kind::Number { range, step, whole },
            Kind::Vector { .. } => Kind::Vector { step },
            other => panic!("`step` is a hint of a number or a vector, not of {other:?}"),
        })
    }

    pub fn color(self) -> Kind {
        self.of_value(|kind| match kind {
            Kind::Vector { .. } => Kind::Color,
            other => panic!("`color` is a hint of `[f32; 3]`, not of {other:?}"),
        })
    }

    pub fn file(self, group: &'static str) -> Kind {
        self.of_value(|kind| match kind {
            Kind::File(_) => Kind::File(group),
            other => panic!("`file` is a hint of a path, not of {other:?}"),
        })
    }

    pub fn pick(self, list: &'static str) -> Kind {
        self.of_value(|kind| match kind {
            Kind::Number { .. } => Kind::Pick { list, text: false },
            Kind::Text => Kind::Pick { list, text: true },
            other => panic!("`pick` is a hint of a number or a text, not of {other:?}"),
        })
    }

    pub fn some(self, value: Value) -> Kind {
        match self {
            Kind::Optional { inner, .. } => Kind::Optional {
                inner,
                some: Some(value),
            },
            other => panic!("`some` is a hint of an `Option`, not of {other:?}"),
        }
    }

    /// A tree of this kind for a value that nothing gives: zero, empty, off, the
    /// first variant. A group has the default of each field that has one.
    pub fn zero(&self) -> Value {
        match self {
            Kind::Number { range, whole, .. } => {
                let zero = range.map_or(0.0, |[min, max]| 0.0f64.clamp(min, max));
                number(zero, *whole)
            }
            Kind::Switch => Value::Bool(false),
            Kind::Text | Kind::File(_) | Kind::Pick { text: true, .. } => {
                Value::String(String::new())
            }
            // No object has the id 0: the editor gives ids from 1.
            Kind::Pick { text: false, .. } | Kind::Object => Value::from(0),
            Kind::Vector { .. } => Value::from(vec![0.0; 3]),
            Kind::Color => Value::from(vec![1.0; 3]),
            Kind::Optional { .. } => Value::Null,
            Kind::Group(fields) => Value::Object(zero_map(fields)),
            Kind::Choice(variants) => variants.first().map_or(Value::Null, Variant::zero),
        }
    }
}

/// A number of a tree. `whole`: with no fraction.
pub fn number(value: f64, whole: bool) -> Value {
    match whole {
        true => Value::from(value.round() as i64),
        false => Value::from(value),
    }
}

impl Field {
    /// The default of the field, or the zero of its kind.
    pub fn zero(&self) -> Value {
        self.default.clone().unwrap_or_else(|| self.kind.zero())
    }
}

fn zero_map(fields: &[Field]) -> Map<String, Value> {
    let entries = fields
        .iter()
        .map(|field| (field.name.to_owned(), field.zero()));
    entries.collect()
}

/// The tree of a variant with content: `{"Name": content}`.
pub fn variant_tree(name: &str, content: Value) -> Value {
    Value::Object([(name.to_owned(), content)].into_iter().collect())
}

impl Variant {
    /// The tree of the variant when a tool chooses it.
    pub fn zero(&self) -> Value {
        if let Some(default) = &self.default {
            return default.clone();
        }
        match (self.form, self.fields.as_slice()) {
            (Form::Unit, _) => Value::String(self.name.to_owned()),
            (Form::Struct, fields) => variant_tree(self.name, Value::Object(zero_map(fields))),
            (Form::Tuple, [field]) => variant_tree(self.name, field.zero()),
            (Form::Tuple, fields) => {
                let values = fields.iter().map(Field::zero).collect();
                variant_tree(self.name, Value::Array(values))
            }
        }
    }

    /// The value of the field `index` of this variant in the tree of the enum.
    /// `None`: the tree is not of this variant.
    pub fn part<'a>(&self, tree: &'a mut Value, index: usize) -> Option<&'a mut Value> {
        let content = tree.get_mut(self.name)?;
        match self.form {
            Form::Unit => None,
            Form::Tuple if self.fields.len() == 1 => Some(content),
            Form::Tuple => content.get_mut(index),
            Form::Struct => content.get_mut(self.fields.get(index)?.name),
        }
    }
}

/// The index of the variant that a tree of an enum has.
pub fn variant_of(variants: &[Variant], tree: &Value) -> Option<usize> {
    let name = match tree {
        Value::String(name) => name,
        Value::Object(map) => map.keys().next()?,
        _ => return None,
    };
    variants.iter().position(|variant| variant.name == name)
}

/// Reads a tree back into its type.
pub fn read<T: DeserializeOwned>(tree: Value) -> anyhow::Result<T> {
    Ok(serde_json::from_value(tree)?)
}

// ---- The types with no fields of their own ----

macro_rules! numbers {
    ($($type:ty: $whole:expr, $range:expr;)*) => {$(
        impl Fields for $type {
            fn kind() -> Kind {
                let step = if $whole { 1.0 } else { 0.01 };
                Kind::Number { range: $range, step, whole: $whole }
            }

            fn tree(&self) -> Value {
                Value::from(*self)
            }
        }
    )*};
}

/// A number with no sign: not below zero.
const NOT_BELOW_ZERO: Option<[f64; 2]> = Some([0.0, u32::MAX as f64]);

numbers! {
    f32: false, None;
    f64: false, None;
    i32: true, None;
    i64: true, None;
    u8: true, Some([0.0, 255.0]);
    u32: true, NOT_BELOW_ZERO;
    u64: true, NOT_BELOW_ZERO;
    usize: true, NOT_BELOW_ZERO;
}

impl Fields for bool {
    fn kind() -> Kind {
        Kind::Switch
    }

    fn tree(&self) -> Value {
        Value::Bool(*self)
    }
}

impl Fields for String {
    fn kind() -> Kind {
        Kind::Text
    }

    fn tree(&self) -> Value {
        Value::String(self.clone())
    }
}

impl Fields for PathBuf {
    fn kind() -> Kind {
        Kind::File("")
    }

    fn tree(&self) -> Value {
        Value::String(self.to_string_lossy().into_owned())
    }
}

impl Fields for [f32; 3] {
    fn kind() -> Kind {
        Kind::Vector { step: 0.01 }
    }

    fn tree(&self) -> Value {
        Value::from(self.to_vec())
    }
}

impl<T: Fields> Fields for Option<T> {
    fn kind() -> Kind {
        Kind::Optional {
            inner: Box::new(T::kind()),
            some: None,
        }
    }

    fn tree(&self) -> Value {
        self.as_ref().map_or(Value::Null, Fields::tree)
    }
}

// ---- The registry ----

/// A type that a tool finds by a name.
pub struct FieldType {
    pub kind: Kind,
    /// Reads a tree as the type and gives the tree of what was read. `Err`: why
    /// the type does not take the tree.
    pub check: fn(&Value) -> Result<Value, String>,
}

/// The types with fields that the modules gave a name. [`add_field_type`] adds one.
#[derive(Resource, Default)]
pub struct FieldTypes(BTreeMap<&'static str, FieldType>);

impl FieldTypes {
    pub fn get(&self, name: &str) -> Option<&FieldType> {
        self.0.get(name)
    }

    /// In the order of the names.
    pub fn iter(&self) -> impl Iterator<Item = (&'static str, &FieldType)> {
        self.0.iter().map(|(name, field_type)| (*name, field_type))
    }
}

/// Gives a type with fields a name for the tools. A name that is in use gets the
/// new type: a project can put its own type in the place of one of the engine.
pub fn add_field_type<T: Fields + DeserializeOwned>(app: &mut App, name: &'static str) {
    let field_type = FieldType {
        kind: T::kind(),
        check: |tree| match read::<T>(tree.clone()) {
            Ok(value) => Ok(value.tree()),
            Err(error) => Err(error.to_string()),
        },
    };
    app.init_resource::<FieldTypes>();
    app.resource_mut::<FieldTypes>().0.insert(name, field_type);
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    use super::*;
    use crate::ConfigFile;

    #[derive(Debug, Clone, PartialEq, Default, crate::Fields, Serialize, Deserialize)]
    enum Shape {
        #[default]
        Point,
        /// A ball.
        #[field(label = "Ball (round)", default = Shape::Ball(0.5))]
        Ball(#[field(label = "radius", range = 0.1..=10.0)] f32),
        Box([f32; 3], [f32; 3]),
        Ring {
            radius: f32,
            #[field(range = 3.0..=64.0)]
            parts: u32,
        },
    }

    #[derive(Debug, Clone, PartialEq, crate::Fields, Serialize, Deserialize)]
    #[fields(default)]
    struct Lamp {
        /// The name in the room.
        name: Option<String>,
        #[field(label = "strength", range = 0.0..=100.0, step = 0.5)]
        intensity: f32,
        on: bool,
        #[field(color)]
        tint: [f32; 3],
        #[field(step = 0.5)]
        place: [f32; 3],
        #[field(file = "sound")]
        hum: PathBuf,
        #[field(pick = "rooms")]
        room: usize,
        #[field(pick = "spawns")]
        spawn: String,
        #[field(color, some = [1.0; 3])]
        glow: Option<[f32; 3]>,
        #[field(range = 1.0..=180.0, some = 30.0)]
        cone: Option<f32>,
        shape: Shape,
        // The state of a game, not a value of a file.
        #[field(skip)]
        #[serde(skip)]
        seconds: f32,
    }

    impl Default for Lamp {
        fn default() -> Self {
            Self {
                name: None,
                intensity: 12.0,
                on: true,
                tint: [1.0, 0.9, 0.8],
                place: [0.0, 2.4, 0.0],
                hum: PathBuf::new(),
                room: 0,
                spawn: String::new(),
                glow: None,
                cone: None,
                shape: Shape::Point,
                seconds: 0.0,
            }
        }
    }

    /// A field with a default of its own, in a type with no `Default`.
    #[derive(Debug, PartialEq, crate::Fields, Serialize, Deserialize)]
    struct Door {
        #[field(default = 1.2)]
        seconds: f32,
        lamp: Lamp,
    }

    fn fields(kind: &Kind) -> &[Field] {
        match kind {
            Kind::Group(fields) => fields,
            other => panic!("not a group: {other:?}"),
        }
    }

    #[test]
    fn the_derive_writes_the_table_of_a_struct() {
        let kind = Lamp::kind();
        let table = fields(&kind);
        // Each field but the skipped one, in the order of the struct.
        let names: Vec<&str> = table.iter().map(|field| field.name).collect();
        let all = "name intensity on tint place hum room spawn glow cone shape";
        assert_eq!(names.join(" "), all);
        let of = |name: &str| table.iter().find(|field| field.name == name).unwrap();

        // The label, the help, and the default of `Default` of the struct.
        assert_eq!(
            (of("name").label, of("name").help),
            ("name", "The name in the room.")
        );
        assert_eq!(of("intensity").label, "strength");
        assert_eq!(of("intensity").default, Some(json!(12.0)));
        assert_eq!(of("on").default, Some(json!(true)));
        assert_eq!(of("name").default, Some(Value::Null));

        // Each kind with its hints.
        let number = Kind::Number {
            range: Some([0.0, 100.0]),
            step: 0.5,
            whole: false,
        };
        assert_eq!(of("intensity").kind, number);
        assert_eq!(of("on").kind, Kind::Switch);
        assert_eq!(of("tint").kind, Kind::Color);
        assert_eq!(of("place").kind, Kind::Vector { step: 0.5 });
        assert_eq!(of("hum").kind, Kind::File("sound"));
        let pick = |list, text| Kind::Pick { list, text };
        assert_eq!(of("room").kind, pick("rooms", false));
        assert_eq!(of("spawn").kind, pick("spawns", true));
        // A hint of the value goes through `Option`; `some` is of the option.
        let optional = |inner, some| Kind::Optional {
            inner: Box::new(inner),
            some: Some(some),
        };
        assert_eq!(
            of("glow").kind,
            optional(Kind::Color, json!([1.0, 1.0, 1.0]))
        );
        let cone = Kind::Number {
            range: Some([1.0, 180.0]),
            step: 0.01,
            whole: false,
        };
        assert_eq!(of("cone").kind, optional(cone, json!(30.0)));
        assert!(matches!(of("shape").kind, Kind::Choice(_)));

        // A type with no `Default`: the default that the field gives.
        let kind = Door::kind();
        assert_eq!(fields(&kind)[0].default, Some(json!(1.2f32)));
        assert_eq!(fields(&kind)[1].default, None);
    }

    #[test]
    fn the_derive_writes_the_table_of_an_enum() {
        let Kind::Choice(variants) = Shape::kind() else {
            panic!("an enum is a choice");
        };
        let names: Vec<&str> = variants.iter().map(|variant| variant.name).collect();
        assert_eq!(names, ["Point", "Ball", "Box", "Ring"]);
        let forms: Vec<Form> = variants.iter().map(|variant| variant.form).collect();
        assert_eq!(forms, [Form::Unit, Form::Tuple, Form::Tuple, Form::Struct]);

        let ball = &variants[1];
        assert_eq!((ball.label, ball.help), ("Ball (round)", "A ball."));
        assert_eq!(ball.default, Some(json!({"Ball": 0.5})));
        // A field with no name: its place as the name, and no label but a given one.
        assert_eq!((ball.fields[0].name, ball.fields[0].label), ("0", "radius"));
        assert_eq!(variants[2].fields[1].name, "1");
        assert_eq!(variants[2].fields[1].label, "");
        assert_eq!(variants[3].fields[1].label, "parts");

        // The tree of a variant that a tool chooses: the default, or zeros. A
        // number with a range starts in it.
        assert_eq!(variants[0].zero(), json!("Point"));
        assert_eq!(variants[1].zero(), json!({"Ball": 0.5}));
        assert_eq!(
            variants[2].zero(),
            json!({"Box": [[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]]})
        );
        assert_eq!(
            variants[3].zero(),
            json!({"Ring": {"radius": 0.0, "parts": 3}})
        );

        // Which variant a tree has, and where its fields are.
        for (index, variant) in variants.iter().enumerate() {
            assert_eq!(variant_of(&variants, &variant.zero()), Some(index));
        }
        assert_eq!(variant_of(&variants, &json!("Cone")), None);
        let mut tree = json!({"Box": [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]});
        assert_eq!(
            variants[2].part(&mut tree, 1),
            Some(&mut json!([4.0, 5.0, 6.0]))
        );
        assert_eq!(variants[1].part(&mut tree, 0), None);
        let mut tree = json!({"Ball": 2.0});
        assert_eq!(variants[1].part(&mut tree, 0), Some(&mut json!(2.0)));
        let mut tree = json!({"Ring": {"radius": 1.0, "parts": 8}});
        assert_eq!(variants[3].part(&mut tree, 1), Some(&mut json!(8)));
        assert_eq!(variants[0].part(&mut json!("Point"), 0), None);
    }

    #[test]
    fn a_value_goes_to_its_tree_and_back() {
        let lamp = Lamp {
            name: Some("desk".to_owned()),
            // A number that is not exact in binary: the tree has the wide form of
            // the `f32`, and the read gives the same `f32`.
            intensity: 0.15,
            glow: Some([0.1, 0.2, 0.3]),
            cone: Some(33.3),
            shape: Shape::Ring {
                radius: 0.7,
                parts: 12,
            },
            ..Lamp::default()
        };
        let tree = lamp.tree();
        // Each field is in the tree, also one with its default, and not the
        // skipped one.
        let Value::Object(map) = &tree else {
            panic!("a struct is a map");
        };
        assert_eq!(map.len(), 11);
        assert_eq!(tree["on"], json!(true));
        assert_eq!(
            tree["shape"],
            json!({"Ring": {"radius": 0.7f32, "parts": 12}})
        );
        assert_eq!(read::<Lamp>(tree).unwrap(), lamp);

        for shape in [
            Shape::Point,
            Shape::Ball(0.3),
            Shape::Box([0.1; 3], [0.2; 3]),
        ] {
            assert_eq!(read::<Shape>(shape.tree()).unwrap(), shape);
        }
        assert_eq!(Shape::Point.tree(), json!("Point"));
        assert_eq!(Shape::Ball(0.5).tree(), json!({"Ball": 0.5}));

        // The zero tree of a type is a tree of the type.
        let zero = Lamp::kind().zero();
        assert_eq!(read::<Lamp>(zero.clone()).unwrap().tree(), zero);
        assert_eq!(zero["intensity"], json!(12.0));
        let door = read::<Door>(Door::kind().zero()).unwrap();
        assert_eq!((door.seconds, &door.lamp), (1.2, &Lamp::default()));
    }

    #[test]
    fn a_module_gives_a_type_a_name() {
        let mut app = App::new(ConfigFile::default()).unwrap();
        add_field_type::<Lamp>(&mut app, "lamp");
        add_field_type::<Shape>(&mut app, "shape");
        let types = app.world().resource::<FieldTypes>();
        let names: Vec<&str> = types.iter().map(|(name, _)| name).collect();
        assert_eq!(names, ["lamp", "shape"]);

        let lamp = types.get("lamp").unwrap();
        assert_eq!(lamp.kind, Lamp::kind());
        let zero = lamp.kind.zero();
        assert_eq!((lamp.check)(&zero), Ok(zero));
        // A tree that the type does not take: the reason of `serde`.
        let error = (lamp.check)(&json!({"intensity": "much"})).unwrap_err();
        assert!(error.contains("invalid type"), "{error}");
        assert!(types.get("door").is_none());
    }

    #[test]
    #[should_panic(expected = "`range` is a hint of a number")]
    fn a_hint_on_the_wrong_kind_is_an_error() {
        let _ = Kind::Text.range(0.0, 1.0);
    }
}
