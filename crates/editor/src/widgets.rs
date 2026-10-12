//! The rows of the inspector for the values of a room file: a file, a texture, a
//! mesh, a pose, a look. They are rows of the one property grid of the tool
//! (`maki_ui::widgets`). Each returns true in a frame where the user changed the
//! value.
//!
//! The public ones are for the argument widgets of the commands of a module
//! (`add_argument_editor`).

use std::path::{Path, PathBuf};

use maki_assets::Material;
use maki_scene::{Filter, LookDef, MeshDef, PoseDef, TextureDef};
use maki_ui::egui::{self, Ui};
pub(crate) use maki_ui::widgets::{
    choice, color, drag, number as number_or, optional, row, same_kind, text, toggle, vector,
};

/// A number with a label and its unit. Drag it, or click it and type.
pub fn number(
    ui: &mut Ui,
    text: &str,
    value: &mut f32,
    speed: f32,
    range: std::ops::RangeInclusive<f32>,
) -> bool {
    number_or(ui, text, value, None, speed, range)
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
/// with these extensions. A path that the list does not have shows in red. An image
/// file and a model file show with a small picture, in the list and at the value.
pub fn file(ui: &mut Ui, text: &str, value: &mut PathBuf, extensions: &[&str]) -> bool {
    let id = egui::Id::new("room directory");
    let directory: PathBuf = ui.data(|data| data.get_temp(id)).unwrap_or_default();
    let known: Vec<String> = files(&directory, extensions)
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    // Forward slashes: the same room file on each system.
    let mut name = value.to_string_lossy().replace('\\', "/");
    row(ui, text, |ui| {
        let show = |ui: &mut Ui, name: &str| crate::pictures::show(ui, &directory.join(name));
        let pictures = crate::pictures::has_picture(extensions);
        if pictures && !name.is_empty() {
            show(ui, &name);
        }
        let picture: Option<crate::event_panel::Picture> =
            if pictures { Some(&show) } else { None };
        let changed =
            crate::event_panel::picker_with(ui, text, &mut name, &known, &[], false, picture);
        if changed {
            *value = PathBuf::from(name);
        }
        changed
    })
}

/// A name of a list, picked from it by a click, with a search field: a thing of the
/// room that a command names. A name that the list does not have shows in red.
pub fn pick(ui: &mut Ui, text: &str, value: &mut String, known: &[String]) -> bool {
    row(ui, text, |ui| {
        crate::event_panel::picker(ui, text, value, known, &[], false)
    })
}

/// The extensions of the sound files.
pub const SOUNDS: [&str; 2] = ["wav", "ogg"];
/// The extensions of the picture files that a texture can be.
pub(crate) const PICTURES: [&str; 3] = ["png", "jpg", "jpeg"];
/// The extensions of the model files.
pub(crate) const MODELS: [&str; 2] = ["gltf", "glb"];
/// The extension of the prefab files.
pub(crate) const PREFABS: [&str; 1] = ["prefab"];

pub(crate) fn texture(ui: &mut Ui, label: &str, value: &mut Option<TextureDef>) -> bool {
    let default = || TextureDef {
        path: PathBuf::new(),
        scale: 1.0,
        filter: Filter::Linear,
    };
    optional(ui, label, value, default, |ui, texture| {
        let filters = [("Linear", Filter::Linear), ("Nearest", Filter::Nearest)];
        file(ui, "file", &mut texture.path, &PICTURES)
            | number_or(
                ui,
                "scale",
                &mut texture.scale,
                Some(1.0),
                0.01,
                0.01..=100.0,
            )
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
                vector(ui, "from", min, None, 0.01) | vector(ui, "to", max, None, 0.01)
            }
            MeshDef::Sphere {
                radius,
                segments,
                rings,
            } => {
                let count = |ui: &mut Ui, text, value: &mut u32, least| {
                    row(ui, text, |ui| {
                        ui.add(drag(value).range(least..=256)).changed()
                    })
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
        // A pose that the room file leaves out: at the origin, not turned, size 1.
        let default = PoseDef::default();
        vector(
            ui,
            "position",
            &mut value.position,
            Some(default.position),
            0.01,
        ) | vector(
            ui,
            "rotation",
            &mut value.rotation,
            Some(default.rotation),
            0.5,
        ) | vector(ui, "scale", &mut value.scale, Some(default.scale), 0.01)
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
