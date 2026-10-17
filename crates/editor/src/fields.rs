//! The rows of the inspector for a thing that says what its fields are
//! (`maki_core::fields`): one function for each type, with no code for a type.
//!
//! [`show`] draws the rows of a tree from its table and changes the tree. The one
//! who calls reads the tree back into the thing (`maki_core::fields::read`).

use std::path::PathBuf;

use maki_core::Fields;
use maki_core::fields::{Kind, Value, Variant, number, read, variant_of};
use maki_scene::ObjectId;
use maki_ui::egui::Ui;
use maki_ui::widgets::help;

use crate::widgets::{
    MODELS, PICTURES, PREFABS, SOUNDS, choice, color, file, number_or, object_pick, optional, pick,
    row, text, toggle, vector,
};

/// What the picks of the rows offer.
#[derive(Clone, Copy)]
pub struct Lists<'a> {
    /// The names of a list of the document, for a field with `pick`.
    pub names: &'a dyn Fn(&str) -> Vec<String>,
    /// `Some`: the list takes a new name ("New..."), and these names of it have the
    /// mark of a name that no event writes (the flags and the variables).
    pub open: &'a dyn Fn(&str) -> Option<Vec<String>>,
    /// The objects of the room, for a field of the kind `Object`
    /// (`crate::widgets::object_names`).
    pub objects: &'a [(ObjectId, String)],
}

impl<'a> Lists<'a> {
    /// No names, and these objects.
    pub fn objects(objects: &'a [(ObjectId, String)]) -> Self {
        Self {
            names: &|_| Vec::new(),
            open: &|_| None,
            objects,
        }
    }
}

/// Draws the rows of a tree of this kind. True: the user changed a value.
pub fn show(ui: &mut Ui, kind: &Kind, tree: &mut Value, lists: Lists) -> bool {
    value(ui, "", "", kind, tree, None, lists)
}

/// Draws the rows of the fields of the variant that a tree of an enum has: the
/// arguments of a command. The variant does not change. True: the user changed a value.
pub fn show_arguments(ui: &mut Ui, kind: &Kind, tree: &mut Value, lists: Lists) -> bool {
    let Kind::Choice(variants) = kind else {
        return show(ui, kind, tree, lists);
    };
    let variant = variant_of(variants, tree).and_then(|index| variants.get(index));
    variant.is_some_and(|variant| variant_fields(ui, variant, tree, lists))
}

/// The extensions of a group of files (`#[field(file = "model")]`).
fn extensions(group: &str) -> &'static [&'static str] {
    match group {
        "model" => &MODELS,
        "picture" => &PICTURES,
        "sound" => &SOUNDS,
        "prefab" => &PREFABS,
        "routine" => &crate::widgets::ROUTINES,
        _ => &[],
    }
}

fn numbers(value: &Value) -> [f32; 3] {
    let part = |index: usize| value.get(index).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    [part(0), part(1), part(2)]
}

/// The row, or the rows, of one value. `default`: for "back to default".
fn value(
    ui: &mut Ui,
    label: &str,
    help_text: &str,
    kind: &Kind,
    tree: &mut Value,
    default: Option<&Value>,
    lists: Lists,
) -> bool {
    help(ui, help_text);
    match kind {
        Kind::Number { range, step, whole } => {
            // The widgets have `f32`, as the fields of the engine have.
            let mut shown = tree.as_f64().unwrap_or(0.0) as f32;
            let range = range.map_or(f32::MIN..=f32::MAX, |[min, max]| min as f32..=max as f32);
            let default = default
                .and_then(Value::as_f64)
                .map(|default| default as f32);
            let changed = number_or(ui, label, &mut shown, default, *step as f32, range);
            if changed {
                *tree = number(f64::from(shown), *whole);
            }
            changed
        }
        Kind::Switch => {
            let mut on = tree.as_bool().unwrap_or(false);
            let changed = toggle(ui, label, &mut on, default.and_then(Value::as_bool));
            if changed {
                *tree = Value::Bool(on);
            }
            changed
        }
        Kind::Text => {
            let mut shown = tree.as_str().unwrap_or_default().to_owned();
            let changed = text(ui, label, &mut shown);
            if changed {
                *tree = Value::String(shown);
            }
            changed
        }
        Kind::Vector { step } => {
            let mut shown = numbers(tree);
            let changed = vector(ui, label, &mut shown, default.map(numbers), *step as f32);
            if changed {
                *tree = Value::from(shown.to_vec());
            }
            changed
        }
        Kind::Color => {
            let mut shown = numbers(tree);
            let changed = color(ui, label, &mut shown);
            if changed {
                *tree = Value::from(shown.to_vec());
            }
            changed
        }
        Kind::File(group) => {
            // A reference with an id is `{path, id}`; a new file drops the id of the old one.
            let shown = tree.as_str().or_else(|| tree.get("path")?.as_str());
            let mut path = PathBuf::from(shown.unwrap_or_default());
            // A group that is not fixed is a data type: its files end in `<type>.ron`.
            let fixed = extensions(group);
            let data = maki_assets::DataTypes::extension(group);
            let data = [data.as_str()];
            let changed = file(
                ui,
                label,
                &mut path,
                if fixed.is_empty() && !group.is_empty() {
                    &data
                } else {
                    fixed
                },
            );
            if changed {
                *tree = Value::String(path.to_string_lossy().replace('\\', "/"));
            }
            changed
        }
        Kind::Pick { list, text: true } => {
            let mut name = tree.as_str().unwrap_or_default().to_owned();
            let known = (lists.names)(list);
            let changed = match (lists.open)(list) {
                Some(unset) => row(ui, label, |ui| {
                    crate::event_panel::pick_or_new(ui, list, &mut name, &known, &unset)
                }),
                None => pick(ui, label, &mut name, &known),
            };
            if changed {
                *tree = Value::String(name);
            }
            changed
        }
        Kind::Pick { list, text: false } => {
            let names = (lists.names)(list);
            let options: Vec<(&str, u64)> = names.iter().map(String::as_str).zip(0..).collect();
            let mut index = tree.as_u64().unwrap_or(0);
            let changed = choice(ui, label, &mut index, &options, PartialEq::eq);
            if changed {
                *tree = Value::from(index);
            }
            changed
        }
        Kind::Object => {
            let Ok(mut object) = read::<ObjectId>(tree.clone()) else {
                return false;
            };
            let names: Vec<String> = lists.objects.iter().map(|(_, name)| name.clone()).collect();
            let changed = object_pick(&mut object, lists.objects, |name| {
                pick(ui, label, name, &names)
            });
            if changed {
                *tree = object.tree();
            }
            changed
        }
        // A text that can be absent: an empty field is no text.
        Kind::Optional { inner, .. } if **inner == Kind::Text => {
            let mut shown = tree.as_str().unwrap_or_default().to_owned();
            let changed = text(ui, label, &mut shown);
            if changed {
                *tree = match shown.is_empty() {
                    true => Value::Null,
                    false => Value::String(shown),
                };
            }
            changed
        }
        Kind::Optional { inner, some } => {
            let mut set = (!tree.is_null()).then(|| tree.take());
            let on = || some.clone().unwrap_or_else(|| inner.zero());
            let changed = optional(ui, label, &mut set, on, |ui, tree| {
                value(ui, "", "", inner, tree, None, lists)
            });
            *tree = set.unwrap_or(Value::Null);
            changed
        }
        Kind::Group(table) => {
            let mut rows = |ui: &mut Ui| {
                let mut changed = false;
                for field in table {
                    if let Some(part) = tree.get_mut(field.name) {
                        let default = field.default.as_ref();
                        changed |= value(
                            ui,
                            field.label,
                            field.help,
                            &field.kind,
                            part,
                            default,
                            lists,
                        );
                    }
                }
                changed
            };
            // With no label: the rows of the thing itself, or of an optional group.
            if label.is_empty() {
                return rows(ui);
            }
            row(ui, label, |_| {});
            ui.indent(label, rows).inner
        }
        Kind::Choice(variants) => {
            let options: Vec<(&str, Option<usize>)> = variants
                .iter()
                .map(|variant| variant.label)
                .zip((0..).map(Some))
                .collect();
            let before = variant_of(variants, tree);
            let mut index = before;
            let mut changed = choice(ui, label, &mut index, &options, PartialEq::eq);
            let Some(variant) = index.and_then(|index| variants.get(index)) else {
                return changed;
            };
            if changed {
                let before = before.and_then(|index| variants.get(index));
                *tree = change_variant(before, tree.take(), variant);
            }
            if !variant.fields.is_empty() {
                let rows = |ui: &mut Ui| variant_fields(ui, variant, tree, lists);
                changed |= ui.indent((label, "fields"), rows).inner;
            }
            changed
        }
        Kind::List(item) => {
            let item = item.get();
            if !tree.is_array() {
                *tree = Value::Array(Vec::new());
            }
            let items = tree.as_array_mut().expect("set above");
            if !label.is_empty() {
                row(ui, label, |_| {});
            }
            let rows = |ui: &mut Ui| {
                let (mut changed, mut removed) = (false, None);
                for (index, part) in items.iter_mut().enumerate() {
                    ui.push_id(index, |ui| {
                        if ui.small_button("x").on_hover_text("Remove").clicked() {
                            removed = Some(index);
                        }
                        changed |= value(ui, "", "", &item, part, None, lists);
                    });
                }
                if let Some(index) = removed {
                    items.remove(index);
                    changed = true;
                }
                if ui.button("Add").clicked() {
                    items.push(item.zero());
                    changed = true;
                }
                changed
            };
            ui.indent((label, "items"), rows).inner
        }
    }
}

/// The tree of the variant `to` when a tool chooses it in place of `before` (the
/// tree `old`). A field of the same kind at the same place keeps its value, so a
/// change of "flag is set" to "flag is not set" keeps the flag.
pub(crate) fn change_variant(before: Option<&Variant>, mut old: Value, to: &Variant) -> Value {
    let mut tree = to.zero();
    let Some(before) = before else {
        return tree;
    };
    for (index, field) in to.fields.iter().enumerate() {
        let same = before
            .fields
            .get(index)
            .is_some_and(|old| old.kind == field.kind);
        if let (true, Some(value)) = (same, before.part(&mut old, index))
            && let Some(part) = to.part(&mut tree, index)
        {
            *part = value.take();
        }
    }
    tree
}

/// The rows of the fields of the variant that a tree of an enum has.
fn variant_fields(ui: &mut Ui, variant: &Variant, tree: &mut Value, lists: Lists) -> bool {
    let mut changed = false;
    for (index, field) in variant.fields.iter().enumerate() {
        if let Some(part) = variant.part(tree, index) {
            changed |= value(ui, field.label, field.help, &field.kind, part, None, lists);
        }
    }
    changed
}
