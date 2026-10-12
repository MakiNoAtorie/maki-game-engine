//! Widgets of the property panel: one function for each kind of value of a room
//! file. Each returns true in a frame where the user changed the value.

use std::path::{Path, PathBuf};

use maki_assets::Material;
use maki_scene::{Filter, LookDef, MeshDef, PoseDef, TextureDef};
use maki_ui::egui::{self, DragValue, Ui};

/// The widget of a number. A value of the file that is out of the range of the
/// widget stays as it is until the user changes it: a look at an item does not
/// change the room.
pub(crate) fn drag<Num: egui::emath::Numeric>(value: &mut Num) -> DragValue<'_> {
    DragValue::new(value).clamp_existing_to_range(false)
}

/// The width of the label of a value: the values of the rows are in one column.
const LABEL_WIDTH: f32 = 118.0;

/// The label of a row, in the label column. An empty label takes no room.
pub(crate) fn label(ui: &mut Ui, text: &str) {
    if text.is_empty() {
        return;
    }
    let size = egui::vec2(LABEL_WIDTH, ui.spacing().interact_size.y);
    let layout = egui::Layout::left_to_right(egui::Align::Center);
    ui.allocate_ui_with_layout(size, layout, |ui| {
        ui.set_min_width(LABEL_WIDTH);
        ui.label(text);
    });
}

/// The unit of a value, from its label: meters, degrees, seconds. Empty: a count
/// or a factor.
// ponytail: by the words of the labels of this crate. A parameter of each widget
// call if a label gets the wrong unit.
fn unit(label: &str) -> &'static str {
    let has = |words: &[&str]| words.iter().any(|word| label.contains(word));
    if has(&["seconds"]) {
        " s"
    } else if has(&["degrees", "rotation"]) {
        " deg"
    } else if has(&[
        "width", "depth", "height", "offset", "size", "radius", "position", "from", "to",
    ]) {
        " m"
    } else {
        ""
    }
}

/// A number with a label and its unit. Drag it, or click it and type.
pub(crate) fn number(
    ui: &mut Ui,
    text: &str,
    value: &mut f32,
    speed: f32,
    range: std::ops::RangeInclusive<f32>,
) -> bool {
    ui.horizontal(|ui| {
        label(ui, text);
        let drag = drag(value).speed(speed).range(range).suffix(unit(text));
        ui.add(drag).changed()
    })
    .inner
}

/// The colors of X, Y, and Z: those of the handles of the gizmo.
pub(crate) const AXES: [egui::Color32; 3] = [
    egui::Color32::from_rgb(235, 80, 80),
    egui::Color32::from_rgb(90, 210, 90),
    egui::Color32::from_rgb(90, 140, 255),
];

/// Three numbers of X, Y, and Z, each with its letter in the color of its axis.
pub(crate) fn vector(ui: &mut Ui, text: &str, value: &mut [f32; 3], speed: f32) -> bool {
    ui.horizontal(|ui| {
        label(ui, text);
        let mut changed = false;
        for (axis, part) in value.iter_mut().enumerate() {
            ui.spacing_mut().item_spacing.x = 3.0;
            ui.colored_label(AXES[axis], ["X", "Y", "Z"][axis]);
            changed |= ui.add(drag(part).speed(speed).suffix(unit(text))).changed();
        }
        changed
    })
    .inner
}

/// Linear RGB. The numbers can go above 1 (a surface with a light of its own); the
/// color button shows and sets the part up to 1.
pub(crate) fn color(ui: &mut Ui, text: &str, value: &mut [f32; 3]) -> bool {
    ui.horizontal(|ui| {
        label(ui, text);
        // On a copy: the button writes the color back also with no change, with the
        // rounding of its color space.
        let mut picked = *value;
        let mut changed = ui.color_edit_button_rgb(&mut picked).changed();
        if changed {
            *value = picked;
        }
        for part in value {
            let drag = drag(part).speed(0.01).range(0.0..=1000.0);
            changed |= ui.add(drag).changed();
        }
        changed
    })
    .inner
}

/// A text field. The value changes when the field loses the focus, not with each
/// key: half of a file name is not a file.
pub(crate) fn text(ui: &mut Ui, text: &str, value: &mut String) -> bool {
    ui.horizontal(|ui| {
        label(ui, text);
        let id = ui.make_persistent_id(text);
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

/// The files with one of these extensions in a directory and one level below it,
/// as a room file names them: from that directory, with `/`.
pub(crate) fn files(directory: &Path, extensions: &[&str]) -> Vec<PathBuf> {
    let files_of = |directory: &Path| -> Vec<PathBuf> {
        let entries = std::fs::read_dir(directory).into_iter().flatten().flatten();
        entries.map(|entry| entry.path()).collect()
    };
    let top = files_of(directory);
    let below = top
        .iter()
        .filter(|path| path.is_dir())
        .flat_map(|path| files_of(path));
    let wanted = |path: &PathBuf| {
        let extension = path.extension().and_then(|extension| extension.to_str());
        extension.is_some_and(|extension| extensions.contains(&extension.to_lowercase().as_str()))
    };
    let mut found: Vec<PathBuf> = top.iter().cloned().chain(below).filter(wanted).collect();
    found.sort();
    let relative = |path: PathBuf| {
        path.strip_prefix(directory)
            .map(Path::to_owned)
            .unwrap_or(path)
    };
    let found = found.into_iter().map(relative);
    found
        .map(|path| PathBuf::from(path.to_string_lossy().replace('\\', "/")))
        .collect()
}

/// The directory of the open room file, for the file pickers of this frame. The
/// property panel sets it before it draws.
pub(crate) fn set_room_directory(ui: &Ui, directory: &Path) {
    let id = egui::Id::new("room directory");
    ui.data_mut(|data| data.insert_temp(id, directory.to_owned()));
}

/// A file of the directory of the room file, picked from the list of the files
/// with these extensions. A path that the list does not have shows in red.
pub(crate) fn file(ui: &mut Ui, text: &str, value: &mut PathBuf, extensions: &[&str]) -> bool {
    let id = egui::Id::new("room directory");
    let directory: PathBuf = ui.data(|data| data.get_temp(id)).unwrap_or_default();
    let known: Vec<String> = files(&directory, extensions)
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    // Forward slashes: the same room file on each system.
    let mut name = value.to_string_lossy().replace('\\', "/");
    ui.horizontal(|ui| {
        label(ui, text);
        let changed = crate::event_panel::picker(ui, text, &mut name, &known, &[], false);
        if changed {
            *value = PathBuf::from(name);
        }
        changed
    })
    .inner
}

/// The extensions of the picture files that a texture can be.
const PICTURES: [&str; 3] = ["png", "jpg", "jpeg"];
/// The extensions of the model files.
pub(crate) const MODELS: [&str; 2] = ["gltf", "glb"];

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
        self::label(ui, label);
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
        file(ui, "file", &mut texture.path, &PICTURES)
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
                let count = |ui: &mut Ui, text, value: &mut u32, least| {
                    ui.horizontal(|ui| {
                        label(ui, text);
                        ui.add(drag(value).range(least..=256)).changed()
                    })
                    .inner
                };
                number(ui, "radius", radius, 0.01, 0.01..=100.0)
                    | count(ui, "segments", segments, 3)
                    | count(ui, "rings", rings, 2)
            }
            MeshDef::File(path) => file(ui, "glTF file", path, &MODELS),
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
