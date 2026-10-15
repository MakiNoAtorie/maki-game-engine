//! Prefab files and their instances.
//!
//! A prefab file is a list of objects around one origin, in the form of a room file
//! ([`PrefabDef`]). An object of a room with the component `prefab` ([`PrefabFile`])
//! shows the objects of the file at its own pose. It stores only the values that it
//! changed: by `object/component`, the changed fields as a tree (2.14). A change of
//! the file shows in each instance that did not change that value.
//!
//! A change is a tree and not RON text: it is a part of a value, and RON cannot read
//! a part of a struct with no type. Two trees merge with no type.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use glam::Vec3;
use maki_core::Fields;
use maki_core::Transform;
use maki_core::fields::Value;
use serde::{Deserialize, Serialize};

use crate::object::{ComponentText, ComponentTypes, ObjectDef};
use crate::room::{PoseDef, pose_in, ron_options};

/// The content of a prefab file: objects around one origin (a table with its
/// chairs). A file name in it (a model, a texture) starts at the directory of the
/// room file that names the prefab.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrefabDef {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objects: Vec<ObjectDef>,
}

/// Reads a prefab file. The error has the file, the line, and the column.
pub fn read_prefab(path: &Path) -> Result<PrefabDef> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read prefab file {}", path.display()))?;
    ron_options()
        .from_str(&text)
        .map_err(|error| anyhow!("{}:{error}", path.display()))
}

/// The text of a prefab file.
pub fn prefab_text(prefab: &PrefabDef) -> String {
    let config = ron::ser::PrettyConfig::default();
    let text = ron_options().to_string_pretty(prefab, config);
    text.expect("a prefab has no value that RON cannot write")
}

/// The component of an object that shows a prefab file at the pose of the object.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Fields)]
#[serde(deny_unknown_fields)]
#[fields(default)]
pub struct PrefabFile {
    /// From the directory of the room file.
    #[field(label = "prefab file", file = "prefab")]
    pub file: PathBuf,
    /// The values that this instance changed: by `object/component` of the file
    /// ([`change_key`]), the changed fields as a tree.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[field(skip)]
    pub changes: BTreeMap<String, Value>,
}

/// An object with the component `prefab`, read: its name, the file, its pose, and
/// its changes.
#[derive(Debug, Clone, PartialEq)]
pub struct PrefabUse {
    pub name: Option<String>,
    /// From the directory of the room file.
    pub file: PathBuf,
    /// Where the origin of the prefab is in the room, and its turn and scale.
    pub pose: PoseDef,
    /// As [`PrefabFile::changes`].
    pub changes: BTreeMap<String, Value>,
}

impl PrefabUse {
    /// The prefab use as an object with the component `prefab`.
    pub fn into_object(self) -> ObjectDef {
        let mut object = ObjectDef {
            name: self.name,
            pose: self.pose,
            ..ObjectDef::default()
        };
        let prefab = PrefabFile {
            file: self.file,
            changes: self.changes,
        };
        object.set_component("prefab", &prefab);
        object
    }
}

impl ObjectDef {
    /// The object as a prefab use, if it has the component `prefab`.
    pub fn prefab(&self) -> Option<Result<PrefabUse, ron::error::SpannedError>> {
        Some(
            self.component::<PrefabFile>("prefab")?
                .map(|prefab| PrefabUse {
                    name: self.name.clone(),
                    file: prefab.file,
                    pose: self.pose,
                    changes: prefab.changes,
                }),
        )
    }
}

/// The key of a component of an object of a prefab file in the changes of an
/// instance: the name of the object (its index if it has none), and the component.
pub fn change_key(index: usize, object: &ObjectDef, component: &str) -> String {
    match &object.name {
        Some(name) => format!("{name}/{component}"),
        None => format!("{index}/{component}"),
    }
}

/// The objects that an instance shows: each object of the file at its place in the
/// room, with the changes of the instance. A change that cannot be made (a type that
/// no module registered, a value that does not read) is left out with a warning: the
/// value of the file shows. An instance in the file is left out (no nesting yet).
pub fn instance_objects(
    used: &PrefabUse,
    prefab: &PrefabDef,
    types: &ComponentTypes,
) -> Vec<ObjectDef> {
    let outer = used.pose.transform();
    let mut objects = Vec::new();
    for (index, inner) in prefab.objects.iter().enumerate() {
        if inner.components.contains_key("prefab") {
            tracing::warn!(file = %used.file.display(), index, "a prefab in a prefab is not shown");
            continue;
        }
        let mut object = inner.clone();
        for (name, text) in &mut object.components {
            let Some(change) = used.changes.get(&change_key(index, inner, name)) else {
                continue;
            };
            match changed_text(types, name, text, change) {
                Ok(new) => *text = new,
                Err(error) => {
                    tracing::warn!(file = %used.file.display(), index, name, %error, "a change of a prefab instance is left out");
                }
            }
        }
        place(&mut object, &outer);
        objects.push(object);
    }
    objects
}

/// The text of a component with a change merged into its tree.
pub fn changed_text(
    types: &ComponentTypes,
    name: &str,
    text: &ComponentText,
    change: &Value,
) -> Result<ComponentText, String> {
    let kind = types
        .get(name)
        .ok_or_else(|| format!("no module registered the type \"{name}\""))?;
    let mut tree = (kind.tree)(text)?;
    merge(&mut tree, change);
    (kind.text)(text, &tree)
}

/// Moves an object of a prefab into the space of the room: its pose, the direction
/// of its light, the second pose of its mover.
fn place(object: &mut ObjectDef, outer: &Transform) {
    object.pose = PoseDef::of(&pose_in(outer, &object.pose.transform()));
    if let Some(Ok(mut light)) = object.light() {
        let turn = |direction: [f32; 3]| (outer.rotation * Vec3::from(direction)).into();
        light.direction = light.direction.map(turn);
        object.set_light(light);
    }
    if let Some(Ok(mut mover)) = object.mover() {
        mover.b = PoseDef::of(&pose_in(outer, &mover.b.transform()));
        object.set_mover(mover);
    }
}

/// Merges `change` into `tree`. A map whose keys are all in the map of the tree goes
/// field by field; another value (a number, a variant of another name) takes the
/// place of the old one.
pub fn merge(tree: &mut Value, change: &Value) {
    match (tree, change) {
        (Value::Object(fields), Value::Object(changed))
            if changed.keys().all(|key| fields.contains_key(key)) =>
        {
            for (key, value) in changed {
                merge(fields.get_mut(key).expect("tested above"), value);
            }
        }
        (tree, change) => *tree = change.clone(),
    }
}

/// What `new` changed of `base`, as a tree that [`merge`] takes. `None`: no change.
/// A map with the same keys gives the changed fields only; a map with other keys (a
/// variant of another name) is a change as a whole.
pub fn difference(base: &Value, new: &Value) -> Option<Value> {
    match (base, new) {
        (Value::Object(old), Value::Object(fields))
            if old.len() == fields.len() && fields.keys().all(|key| old.contains_key(key)) =>
        {
            let changed: serde_json::Map<String, Value> = fields
                .iter()
                .filter_map(|(key, value)| Some((key.clone(), difference(&old[key], value)?)))
                .collect();
            (!changed.is_empty()).then_some(Value::Object(changed))
        }
        (base, new) => (base != new).then(|| new.clone()),
    }
}

#[cfg(test)]
mod tests {
    use maki_core::{App, ConfigFile};
    use serde_json::json;

    use super::*;
    use crate::object::add_component_type;
    use crate::room::{LightSource, Prop};

    fn types() -> App {
        let mut app = App::new(ConfigFile::default()).unwrap();
        add_component_type::<Prop>(&mut app, "prop");
        add_component_type::<LightSource>(&mut app, "light");
        app
    }

    const LAMP: &str = r#"(objects: [
        (name: "lamp", pose: (position: (0.0, 2.0, 0.0)), components: {
            "light": (radius: 0.1, color: (1.0, 0.9, 0.8), intensity: 8.0, cone: 30.0, direction: (1.0, 0.0, 0.0)),
        }),
        (pose: (position: (1.0, 0.5, 0.0)), components: {"prop": (mesh: Cube(0.4))}),
    ])"#;

    #[test]
    fn a_prefab_file_is_a_list_of_objects_that_reads_back() {
        let prefab: PrefabDef = ron_options().from_str(LAMP).unwrap();
        assert_eq!(prefab.objects.len(), 2);
        let again: PrefabDef = ron_options().from_str(&prefab_text(&prefab)).unwrap();
        assert_eq!(again, prefab);
    }

    #[test]
    fn an_instance_shows_the_file_at_its_pose_with_its_changes() {
        let app = types();
        let types = app.world().resource::<ComponentTypes>();
        let prefab: PrefabDef = ron_options().from_str(LAMP).unwrap();
        let mut used = PrefabUse {
            name: Some("lamp 2".to_owned()),
            file: PathBuf::from("lamp.prefab"),
            pose: PoseDef {
                position: [3.0, 0.0, 1.0],
                rotation: [0.0, 90.0, 0.0],
                ..PoseDef::default()
            },
            changes: BTreeMap::new(),
        };
        // No change: the objects of the file, at the pose of the instance. A quarter
        // turn around Y moves +X to -Z: the cube and the cone of the light.
        let objects = instance_objects(&used, &prefab, types);
        let light = objects[0].light().unwrap().unwrap();
        assert!(Vec3::from(light.position).distance(Vec3::new(3.0, 2.0, 1.0)) < 1e-6);
        let direction = light.direction.unwrap();
        assert!(
            Vec3::from(direction).distance(Vec3::NEG_Z) < 1e-6,
            "{direction:?}"
        );
        let cube = objects[1].pose.position;
        assert!(
            Vec3::from(cube).distance(Vec3::new(3.0, 0.5, 0.0)) < 1e-6,
            "{cube:?}"
        );

        // A change of the intensity, and of the mesh of the object with no name.
        used.changes = BTreeMap::from([
            ("lamp/light".to_owned(), json!({"intensity": 3.0})),
            (
                "1/prop".to_owned(),
                json!({"mesh": {"Sphere": {"radius": 0.2, "segments": 8, "rings": 4}}}),
            ),
            ("lamp/swing".to_owned(), json!({"angle": 1.0})),
        ]);
        let objects = instance_objects(&used, &prefab, types);
        let light = objects[0].light().unwrap().unwrap();
        assert_eq!(
            (light.intensity, light.radius, light.cone),
            (3.0, 0.1, Some(30.0))
        );
        let prop = objects[1].prop().unwrap().unwrap();
        assert_eq!(
            prop.mesh,
            crate::MeshDef::Sphere {
                radius: 0.2,
                segments: 8,
                rings: 4
            }
        );

        // The file changes: what the instance did not change follows it.
        let changed = LAMP
            .replace("radius: 0.1", "radius: 0.3")
            .replace("intensity: 8.0", "intensity: 9.0");
        let prefab: PrefabDef = ron_options().from_str(&changed).unwrap();
        let light = instance_objects(&used, &prefab, types)[0]
            .light()
            .unwrap()
            .unwrap();
        assert_eq!((light.intensity, light.radius), (3.0, 0.3));
    }

    #[test]
    fn a_difference_merged_into_its_base_gives_the_new_tree() {
        let base = json!({"intensity": 8.0, "color": [1.0, 0.9, 0.8], "mesh": {"Cube": 1.0}, "cone": null});
        let new = json!({"intensity": 3.0, "color": [1.0, 0.9, 0.8], "mesh": {"Sphere": {"radius": 0.5}}, "cone": 30.0});
        let change = difference(&base, &new).unwrap();
        assert_eq!(
            change,
            json!({"intensity": 3.0, "mesh": {"Sphere": {"radius": 0.5}}, "cone": 30.0})
        );
        let mut merged = base.clone();
        merge(&mut merged, &change);
        assert_eq!(merged, new);
        assert_eq!(difference(&base, &base), None);
        // A change written to RON and read back is the same tree.
        let text = ron_options().to_string(&change).unwrap();
        let back: Value = ron_options().from_str(&text).unwrap();
        assert_eq!(back, change);
    }
}
