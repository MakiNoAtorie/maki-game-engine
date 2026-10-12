//! Widgets of the property panel: one function for each kind of value of a room
//! file. Each returns true in a frame where the user changed the value.

use std::path::PathBuf;

use maki_assets::Material;
use maki_scene::{Filter, LookDef, MeshDef, PoseDef, TextureDef};
use maki_ui::egui::{self, DragValue, Ui};

/// A number with a label. Drag it, or click it and type.
pub(crate) fn number(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    speed: f32,
    range: std::ops::RangeInclusive<f32>,
) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(DragValue::new(value).speed(speed).range(range))
            .changed()
    })
    .inner
}

pub(crate) fn vector(ui: &mut Ui, label: &str, value: &mut [f32; 3], speed: f32) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        let mut changed = false;
        for part in value {
            changed |= ui.add(DragValue::new(part).speed(speed)).changed();
        }
        changed
    })
    .inner
}

/// Linear RGB. The numbers can go above 1 (a surface with a light of its own); the
/// color button shows and sets the part up to 1.
pub(crate) fn color(ui: &mut Ui, label: &str, value: &mut [f32; 3]) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        let mut changed = ui.color_edit_button_rgb(value).changed();
        for part in value {
            let drag = DragValue::new(part).speed(0.01).range(0.0..=1000.0);
            changed |= ui.add(drag).changed();
        }
        changed
    })
    .inner
}

/// A text field. The value changes when the field loses the focus, not with each
/// key: half of a file name is not a file.
pub(crate) fn text(ui: &mut Ui, label: &str, value: &mut String) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        let id = ui.make_persistent_id(label);
        // The text while it is typed.
        let typed = ui.ctx().memory(|memory| memory.has_focus(id));
        let typed = typed.then(|| ui.ctx().data(|data| data.get_temp::<String>(id)));
        let mut buffer = typed.flatten().unwrap_or_else(|| value.clone());
        let response = ui.add(egui::TextEdit::singleline(&mut buffer).id(id));
        if response.has_focus() {
            ui.ctx().data_mut(|data| data.insert_temp(id, buffer));
            return false;
        }
        ui.ctx().data_mut(|data| data.remove::<String>(id));
        let changed = response.lost_focus() && buffer != *value;
        if changed {
            *value = buffer;
        }
        changed
    })
    .inner
}

/// A name that an item can be without: an empty field is no name.
pub(crate) fn name(ui: &mut Ui, value: &mut Option<String>) -> bool {
    let mut name = value.clone().unwrap_or_default();
    let changed = text(ui, "name", &mut name);
    if changed {
        *value = (!name.is_empty()).then_some(name);
    }
    changed
}

pub(crate) fn path(ui: &mut Ui, label: &str, value: &mut PathBuf) -> bool {
    // Forward slashes: the same room file on each system.
    let mut name = value.to_string_lossy().replace('\\', "/");
    let changed = text(ui, label, &mut name);
    if changed {
        *value = PathBuf::from(name);
    }
    changed
}

/// A list to choose one option from. `same` tells if the value is that option (for
/// an enum with fields: the same variant).
pub(crate) fn choice<T: Clone>(
    ui: &mut Ui,
    label: &str,
    value: &mut T,
    options: &[(&str, T)],
    same: impl Fn(&T, &T) -> bool,
) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        let current = options.iter().find(|(_, option)| same(option, value));
        let mut changed = false;
        egui::ComboBox::from_id_salt(label)
            .selected_text(current.map_or("?", |(name, _)| name))
            .show_ui(ui, |ui| {
                for (name, option) in options {
                    let selected = same(option, value);
                    if ui.selectable_label(selected, *name).clicked() && !selected {
                        *value = option.clone();
                        changed = true;
                    }
                }
            });
        changed
    })
    .inner
}

/// The same variant of an enum.
pub(crate) fn same_kind<T>(a: &T, b: &T) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

/// A value that an item can be without: a checkbox, and the value while it is set.
pub(crate) fn optional<T>(
    ui: &mut Ui,
    label: &str,
    value: &mut Option<T>,
    default: impl FnOnce() -> T,
    edit: impl FnOnce(&mut Ui, &mut T) -> bool,
) -> bool {
    let mut set = value.is_some();
    let mut changed = ui.checkbox(&mut set, label).changed();
    if changed {
        *value = set.then(default);
    }
    if let Some(value) = value {
        changed |= ui.indent(label, |ui| edit(ui, value)).inner;
    }
    changed
}

pub(crate) fn texture(ui: &mut Ui, label: &str, value: &mut Option<TextureDef>) -> bool {
    let default = || TextureDef {
        path: PathBuf::new(),
        scale: 1.0,
        filter: Filter::Linear,
    };
    optional(ui, label, value, default, |ui, texture| {
        let filters = [("Linear", Filter::Linear), ("Nearest", Filter::Nearest)];
        path(ui, "file", &mut texture.path)
            | number(ui, "scale", &mut texture.scale, 0.01, 0.01..=100.0)
            | choice(ui, "filter", &mut texture.filter, &filters, PartialEq::eq)
    })
}

pub(crate) fn mesh(ui: &mut Ui, value: &mut MeshDef) -> bool {
    let kinds = [
        ("Cube", MeshDef::Cube(1.0)),
        (
            "Cuboid",
            MeshDef::Cuboid([-0.5, 0.0, -0.5], [0.5, 1.0, 0.5]),
        ),
        (
            "Sphere",
            MeshDef::Sphere {
                radius: 0.5,
                segments: 32,
                rings: 16,
            },
        ),
        ("File", MeshDef::File(PathBuf::new())),
    ];
    let mut changed = choice(ui, "mesh", value, &kinds, same_kind);
    ui.indent("mesh", |ui| {
        changed |= match value {
            MeshDef::Cube(size) => number(ui, "size", size, 0.01, 0.01..=100.0),
            MeshDef::Cuboid(min, max) => {
                vector(ui, "from", min, 0.01) | vector(ui, "to", max, 0.01)
            }
            MeshDef::Sphere {
                radius,
                segments,
                rings,
            } => {
                let count = |ui: &mut Ui, label, value: &mut u32, least| {
                    ui.horizontal(|ui| {
                        ui.label(label);
                        ui.add(DragValue::new(value).range(least..=256)).changed()
                    })
                    .inner
                };
                number(ui, "radius", radius, 0.01, 0.01..=100.0)
                    | count(ui, "segments", segments, 3)
                    | count(ui, "rings", rings, 2)
            }
            MeshDef::File(file) => path(ui, "glTF file", file),
        };
    });
    changed
}

pub(crate) fn pose(ui: &mut Ui, label: &str, value: &mut PoseDef) -> bool {
    ui.label(label);
    ui.indent(label, |ui| {
        vector(ui, "position", &mut value.position, 0.01)
            | vector(ui, "rotation", &mut value.rotation, 0.5)
            | vector(ui, "scale", &mut value.scale, 0.01)
    })
    .inner
}

pub(crate) fn look(ui: &mut Ui, value: &mut LookDef) -> bool {
    ui.label("look");
    ui.indent("look", |ui| {
        let materials = [
            ("Diffuse", Material::Diffuse),
            ("Mirror", Material::Mirror),
            ("Glass", Material::Glass { ior: 1.5 }),
            ("Metal", Material::Metal { roughness: 0.3 }),
            ("Plastic", Material::Plastic { roughness: 0.3 }),
        ];
        let material = |ui: &mut Ui, material: &mut Material| {
            choice(ui, "kind", material, &materials, same_kind)
                | match material {
                    Material::Diffuse | Material::Mirror => false,
                    Material::Glass { ior } => number(ui, "ior", ior, 0.01, 1.0..=3.0),
                    Material::Metal { roughness } | Material::Plastic { roughness } => {
                        number(ui, "roughness", roughness, 0.01, 0.05..=1.0)
                    }
                }
        };
        let white = || [1.0; 3];
        texture(ui, "texture", &mut value.texture)
            | optional(ui, "color", &mut value.color, white, |ui, value| {
                color(ui, "", value)
            })
            | optional(
                ui,
                "material",
                &mut value.material,
                || Material::Diffuse,
                material,
            )
            | optional(ui, "emissive", &mut value.emissive, white, |ui, value| {
                color(ui, "", value)
            })
    })
    .inner
}
