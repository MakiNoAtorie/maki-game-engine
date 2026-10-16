//! The rows of the inspector for a thing that says what its fields are
//! (`maki_core::fields`): one function for each type, with no code for a type.
//!
//! [`show`] draws the rows of a tree from its table and changes the tree. The one
//! who calls reads the tree back into the thing (`maki_core::fields::read`).

use std::path::PathBuf;

use maki_core::fields::{Kind, Value, Variant, number, variant_of};
use maki_ui::egui::Ui;
use maki_ui::widgets::help;

use crate::widgets::{
    MODELS, PICTURES, PREFABS, SOUNDS, choice, color, file, number_or, optional, pick, row, text,
    toggle, vector,
};

/// The names of a list of the document, for a field with `pick`.
pub type Lists<'a> = &'a dyn Fn(&str) -> Vec<String>;

/// Draws the rows of a tree of this kind. True: the user changed a value.
pub fn show(ui: &mut Ui, kind: &Kind, tree: &mut Value, lists: Lists) -> bool {
    value(ui, "", "", kind, tree, None, lists)
}

/// The extensions of a group of files (`#[field(file = "model")]`).
fn extensions(group: &str) -> &'static [&'static str] {
    match group {
        "model" => &MODELS,
        "picture" => &PICTURES,
        "sound" => &SOUNDS,
        "prefab" => &PREFABS,
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
            let changed = file(ui, label, &mut path, extensions(group));
            if changed {
                *tree = Value::String(path.to_string_lossy().replace('\\', "/"));
            }
            changed
        }
        Kind::Pick { list, text: true } => {
            let mut name = tree.as_str().unwrap_or_default().to_owned();
            let changed = pick(ui, label, &mut name, &lists(list));
            if changed {
                *tree = Value::String(name);
            }
            changed
        }
        Kind::Pick { list, text: false } => {
            let names = lists(list);
            let options: Vec<(&str, u64)> = names.iter().map(String::as_str).zip(0..).collect();
            let mut index = tree.as_u64().unwrap_or(0);
            let changed = choice(ui, label, &mut index, &options, PartialEq::eq);
            if changed {
                *tree = Value::from(index);
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
            let mut index = variant_of(variants, tree);
            let mut changed = choice(ui, label, &mut index, &options, PartialEq::eq);
            let Some(variant) = index.and_then(|index| variants.get(index)) else {
                return changed;
            };
            if changed {
                *tree = variant.zero();
            }
            if !variant.fields.is_empty() {
                let rows = |ui: &mut Ui| variant_fields(ui, variant, tree, lists);
                changed |= ui.indent((label, "fields"), rows).inner;
            }
            changed
        }
    }
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
