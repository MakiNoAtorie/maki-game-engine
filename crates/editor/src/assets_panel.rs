//! The asset browser: the folders and files of the asset folder (the directory of the
//! room files), with search, a filter by kind, and the file operations.
//!
//! A file is dragged out of the panel: into the picture (a model is a prop, a prefab an
//! instance, on the floor at the drop; a picture goes on the surface under the
//! pointer), onto a file field of the inspector, or onto a folder (a move). A file
//! that is dropped on the window from Windows is copied into the shown folder. A new
//! name or a move keeps the rooms right: a reference finds its file by the id
//! (`crate::ids`), and the id file goes along.

use std::path::{Path, PathBuf};

use bevy_ecs::world::{Mut, World};
use glam::Vec3;
use maki_assets::FileId;
use maki_core::fields::Value;
use maki_ui::THEME;
use maki_ui::egui::{self, Ui};
use maki_ui::theme::icon;

use crate::widgets::{MODELS, PICTURES, SOUNDS};
use crate::{Command, Editor, Item, Kind};

/// What a drag of the panel carries: a file, from the asset folder, with `/`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AssetDrag(pub PathBuf);

/// The kinds of the filter, with their file name ends.
const KINDS: [(&str, &[&str]); 8] = [
    ("All", &[]),
    ("Models", &MODELS),
    ("Pictures", &PICTURES),
    ("Sounds", &SOUNDS),
    ("Prefabs", &["prefab"]),
    ("Rooms", &["ron"]),
    ("Data", &["ron"]),
    ("Sources", &["blend", "obj", "psd"]),
];

/// The state of the panel between frames.
#[derive(Default)]
pub(crate) struct AssetPanel {
    /// The shown folder, from the asset folder. Empty: the asset folder.
    folder: PathBuf,
    search: String,
    /// An index of [`KINDS`].
    kind: usize,
    selected: Option<PathBuf>,
    /// The file and the new name that is typed.
    renaming: Option<(PathBuf, String)>,
    /// The file to delete, and the files that name it.
    deleting: Option<(PathBuf, Vec<String>)>,
    /// The name of a new folder that is typed.
    new_folder: Option<String>,
}

/// True: a file name ends with one of the extensions (`.prefab`, `.glb`).
fn has_end(name: &str, ends: &[&str]) -> bool {
    let name = name.to_lowercase();
    ends.iter().any(|end| name.ends_with(&format!(".{end}")))
}

/// A room file is `name.ron`; a data file has a type before `.ron` (`key.item.ron`).
fn is_room(name: &str) -> bool {
    name.ends_with(".ron") && !name.trim_end_matches(".ron").contains('.')
}

fn of_kind(name: &str, kind: usize) -> bool {
    match KINDS[kind].0 {
        "All" => true,
        "Rooms" => is_room(name),
        "Data" => has_end(name, &["ron"]) && !is_room(name),
        _ => has_end(name, KINDS[kind].1),
    }
}

/// A typed name as one file name. `None`: empty, or with a path in it (it would leave the folder).
fn plain_name(name: &str) -> Option<&str> {
    let name = name.trim();
    let bad = name.is_empty() || name.contains(['/', '\\', ':']) || name.contains("..");
    (!bad).then_some(name)
}

/// The path from the asset folder, with `/`.
fn relative(root: &Path, path: &Path) -> PathBuf {
    let path = path.strip_prefix(root).unwrap_or(path);
    PathBuf::from(path.to_string_lossy().replace('\\', "/"))
}

/// The folders and the files of a folder, folders first, by name. No id files.
fn entries(folder: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .collect();
    paths.sort();
    let is_id = |path: &PathBuf| path.extension().is_some_and(|end| end == "id");
    let (folders, files) = paths.into_iter().partition(|path| path.is_dir());
    let files = Vec::into_iter(files).filter(|path| !is_id(path)).collect();
    (folders, files)
}

/// The files with `.ron` under the asset folder that name a file: by its id, or by
/// its name if it has none. From the asset folder.
pub(crate) fn users(root: &Path, file: &Path) -> Vec<String> {
    let needle = match FileId::read(file) {
        Some(id) => id.to_string(),
        None => file
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
    };
    let mut found = Vec::new();
    let mut folders = vec![root.to_owned()];
    while let Some(folder) = folders.pop() {
        let (inner, files) = entries(&folder);
        folders.extend(inner);
        for path in files {
            let ron = has_end(&path.to_string_lossy(), &["ron"]);
            if ron
                && path != file
                && std::fs::read_to_string(&path).is_ok_and(|t| t.contains(&needle))
            {
                found.push(relative(root, &path).to_string_lossy().into_owned());
            }
        }
    }
    found.sort();
    found
}

/// A path beside `path` that no file has: `name_2.ext`, `name_3.ext`, ...
fn free_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_owned();
    }
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    // The end after the first dot: `key.item.ron` keeps `.item.ron`.
    let (stem, end) = name.split_once('.').map_or((&*name, ""), |(s, e)| (s, e));
    (2..)
        .map(|number| match end {
            "" => path.with_file_name(format!("{stem}_{number}")),
            end => path.with_file_name(format!("{stem}_{number}.{end}")),
        })
        .find(|path| !path.exists())
        .expect("the range has no end")
}

/// Moves a file and its id file. `to` must not exist.
pub(crate) fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if to.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "a file has that name",
        ));
    }
    std::fs::rename(from, to)?;
    let id = FileId::file_of(from);
    if id.exists() {
        std::fs::rename(id, FileId::file_of(to))?;
    }
    Ok(())
}

/// Copies a file with a new name beside it; the copy gets a new id (no id file).
fn copy_file(from: &Path) -> std::io::Result<PathBuf> {
    let to = free_path(from);
    std::fs::copy(from, &to)?;
    Ok(to)
}

/// Deletes a file and its id file, or an empty folder.
fn delete_file(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        return std::fs::remove_dir(path);
    }
    std::fs::remove_file(path)?;
    let id = FileId::file_of(path);
    if id.exists() {
        std::fs::remove_file(id)?;
    }
    Ok(())
}

/// Shows a file in the file manager of the system.
fn show_in_explorer(path: &Path) {
    #[cfg(windows)]
    let started = std::process::Command::new("explorer")
        .arg(format!("/select,{}", path.display()))
        .spawn();
    #[cfg(target_os = "macos")]
    let started = std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn();
    #[cfg(not(any(windows, target_os = "macos")))]
    let started = std::process::Command::new("xdg-open")
        .arg(path.parent().unwrap_or(path))
        .spawn();
    if let Err(error) = started {
        tracing::warn!(path = %path.display(), %error, "no file manager");
    }
}

/// The panel function.
pub(crate) fn assets_panel(ui: &mut Ui, world: &mut World, state: &mut AssetPanel) {
    let root = world.resource::<Editor>().rooms.clone();
    if !root.join(&state.folder).is_dir() {
        state.folder = PathBuf::new();
    }
    let folder = root.join(&state.folder);
    let mut status = None;

    // Files from Windows go into the shown folder.
    let dropped: Vec<PathBuf> = ui.input(|input| {
        let files = input.raw.dropped_files.iter();
        files.map(|file| file.path().to_owned()).collect()
    });
    for from in dropped {
        let to = free_path(&folder.join(from.file_name().unwrap_or_default()));
        status = Some(match std::fs::copy(&from, &to) {
            Ok(_) => format!("Copied in: {}", relative(&root, &to).display()),
            Err(error) => format!("Not copied: {} ({error})", from.display()),
        });
    }

    top_row(ui, &root, state, &mut status);
    question_rows(ui, &root, state, &mut status);
    ui.separator();
    ui.horizontal_top(|ui| {
        egui::ScrollArea::vertical()
            .id_salt("asset folders")
            .max_width(180.0)
            .auto_shrink([true, false])
            .show(ui, |ui| {
                ui.set_min_width(160.0);
                folder_tree(ui, &root, &root, state, &mut status);
            });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("asset files")
            .auto_shrink(false)
            .show(ui, |ui| {
                file_grid(ui, world, &root, &folder, state, &mut status);
            });
    });
    // The file under the pointer while it is dragged.
    if let Some(drag) = egui::DragAndDrop::payload::<AssetDrag>(ui.ctx())
        && let Some(at) = ui.ctx().pointer_latest_pos()
    {
        let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("asset drag"));
        let name = drag.0.file_name().unwrap_or_default().to_string_lossy();
        let font = egui::FontId::proportional(13.0);
        let painter = ui.ctx().layer_painter(layer);
        painter.text(
            at + egui::vec2(12.0, 0.0),
            egui::Align2::LEFT_CENTER,
            name,
            font,
            THEME.play,
        );
    }
    if let Some(status) = status {
        world.resource_mut::<Editor>().status = status;
    }
}

/// The folder, search, filter, and the folder operations.
fn top_row(ui: &mut Ui, root: &Path, state: &mut AssetPanel, status: &mut Option<String>) {
    ui.horizontal(|ui| {
        let parent = state.folder.parent().map(Path::to_owned);
        if ui
            .add_enabled(parent.is_some(), egui::Button::new("⬆"))
            .on_hover_text("Up one folder")
            .clicked()
        {
            state.folder = parent.unwrap_or_default();
        }
        ui.strong(format!("assets/{}", state.folder.display()));
        ui.separator();
        ui.label(icon::SEARCH);
        ui.add(
            egui::TextEdit::singleline(&mut state.search)
                .hint_text("search")
                .desired_width(140.0),
        );
        egui::ComboBox::from_id_salt("asset kind")
            .selected_text(KINDS[state.kind].0)
            .show_ui(ui, |ui| {
                for (index, (name, _)) in KINDS.iter().enumerate() {
                    ui.selectable_value(&mut state.kind, index, *name);
                }
            });
        if ui
            .button(format!("{} Folder", icon::ADD))
            .on_hover_text("New folder")
            .clicked()
        {
            state.new_folder = Some(String::new());
        }
        if ui.button("Show in Explorer").clicked() {
            let shown = state
                .selected
                .as_ref()
                .map_or(state.folder.clone(), Clone::clone);
            show_in_explorer(&root.join(shown));
        }
        if state.search.is_empty() && state.kind == 0 {
            ui.weak("Drop files from Windows here to copy them in.");
        }
        let _ = status;
    });
}

/// The rows that wait for a name or an answer: new folder, new name, delete.
fn question_rows(ui: &mut Ui, root: &Path, state: &mut AssetPanel, status: &mut Option<String>) {
    let enter = |ui: &Ui| ui.input(|input| input.key_pressed(egui::Key::Enter));
    if let Some(name) = &mut state.new_folder {
        let mut done = false;
        ui.horizontal(|ui| {
            ui.label("New folder:");
            let field = ui.text_edit_singleline(name);
            field.request_focus();
            if (ui.button("Make").clicked() || enter(ui)) && !name.trim().is_empty() {
                *status = Some(match plain_name(name) {
                    Some(name) => {
                        let path = root.join(&state.folder).join(name);
                        match std::fs::create_dir(&path) {
                            Ok(()) => format!("New folder {}", relative(root, &path).display()),
                            Err(error) => format!("No new folder: {error}"),
                        }
                    }
                    None => format!("\"{name}\" is not a folder name: use no / \\ : or .."),
                });
                done = true;
            }
            done |= ui.button("Cancel").clicked();
        });
        if done {
            state.new_folder = None;
        }
    }
    if let Some((file, name)) = &mut state.renaming {
        let mut done = false;
        ui.horizontal(|ui| {
            ui.label(format!("New name of {}:", file.display()));
            ui.text_edit_singleline(name).request_focus();
            if (ui.button("Rename").clicked() || enter(ui)) && !name.trim().is_empty() {
                let from = root.join(&*file);
                *status = Some(match plain_name(name) {
                    Some(name) => {
                        let to = from.with_file_name(name);
                        match move_file(&from, &to) {
                            Ok(()) => format!("Renamed to {}", relative(root, &to).display()),
                            Err(error) => format!("Not renamed: {error}"),
                        }
                    }
                    None => format!("\"{name}\" is not a file name: use no / \\ : or .."),
                });
                done = true;
            }
            done |= ui.button("Cancel").clicked();
        });
        if done {
            state.renaming = None;
        }
    }
    if let Some((file, users)) = &state.deleting {
        let mut done = false;
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(
                THEME.warning,
                format!("Delete {}? It cannot be undone.", file.display()),
            );
            if !users.is_empty() {
                ui.colored_label(THEME.error, format!("Used by: {}", users.join(", ")));
            }
            if ui.button("Delete").clicked() {
                *status = Some(match delete_file(&root.join(file)) {
                    Ok(()) => format!("Deleted {}", file.display()),
                    Err(error) => format!("Not deleted: {error}"),
                });
                done = true;
            }
            done |= ui.button("Cancel").clicked();
        });
        if done {
            state.deleting = None;
            state.selected = None;
        }
    }
}

/// The folders under `folder`; a click shows one; a file dropped on one moves there.
fn folder_tree(
    ui: &mut Ui,
    root: &Path,
    folder: &Path,
    state: &mut AssetPanel,
    status: &mut Option<String>,
) {
    let relative_folder = relative(root, folder);
    let name = match folder == root {
        true => "assets".to_owned(),
        false => folder
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
    };
    let (inner, _) = entries(folder);
    let shown = state.folder == relative_folder;
    let label = |ui: &mut Ui, state: &mut AssetPanel, status: &mut Option<String>| {
        let response = ui.selectable_label(shown, format!("📁 {name}"));
        if response.clicked() {
            state.folder = relative_folder.clone();
        }
        drop_into_folder(&response, root, folder, status);
    };
    if inner.is_empty() {
        label(ui, state, status);
        return;
    }
    let id = ui.make_persistent_id(("asset folder", &relative_folder));
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, folder == root)
        .show_header(ui, |ui| label(ui, state, status))
        .body(|ui| {
            for child in &inner {
                folder_tree(ui, root, child, state, status);
            }
        });
}

/// A file of the panel that is dropped on a folder moves into it.
fn drop_into_folder(
    response: &egui::Response,
    root: &Path,
    folder: &Path,
    status: &mut Option<String>,
) {
    let Some(drag) = response.dnd_release_payload::<AssetDrag>() else {
        if response.dnd_hover_payload::<AssetDrag>().is_some() {
            response.clone().highlight();
        }
        return;
    };
    let from = root.join(&drag.0);
    let to = folder.join(from.file_name().unwrap_or_default());
    if from == to {
        return;
    }
    *status = Some(match move_file(&from, &to) {
        Ok(()) => format!("Moved to {}", relative(root, &to).display()),
        Err(error) => format!("Not moved: {error}"),
    });
}

/// The files of the shown folder (with a search: of all folders below it).
fn file_grid(
    ui: &mut Ui,
    world: &mut World,
    root: &Path,
    folder: &Path,
    state: &mut AssetPanel,
    status: &mut Option<String>,
) {
    let search = state.search.trim().to_lowercase();
    let (folders, mut files) = entries(folder);
    if !search.is_empty() {
        // A search looks in the folders below too.
        let mut stack = folders.clone();
        while let Some(inner) = stack.pop() {
            let (more, found) = entries(&inner);
            stack.extend(more);
            files.extend(found);
        }
    }
    let name_of = |path: &Path| {
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    };
    let files: Vec<PathBuf> = files
        .into_iter()
        .filter(|path| of_kind(&name_of(path), state.kind))
        .filter(|path| search.is_empty() || name_of(path).to_lowercase().contains(&search))
        .collect();
    let folders = if search.is_empty() && state.kind == 0 {
        folders
    } else {
        Vec::new()
    };
    if files.is_empty() && folders.is_empty() {
        ui.weak("No files.");
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for path in folders.iter().chain(&files) {
            cell(ui, world, root, path, state, status);
        }
    });
}

/// One file or folder of the grid: its picture and its name.
fn cell(
    ui: &mut Ui,
    world: &mut World,
    root: &Path,
    path: &Path,
    state: &mut AssetPanel,
    status: &mut Option<String>,
) {
    let rel = relative(root, path);
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let selected = state.selected.as_ref() == Some(&rel);
    let inner = ui.allocate_ui(egui::vec2(80.0, 76.0), |ui| {
        ui.set_width(80.0);
        ui.vertical_centered(|ui| {
            if path.is_dir() {
                ui.label(egui::RichText::new("📁").size(32.0));
            } else if crate::pictures::has_picture(&[path
                .extension()
                .unwrap_or_default()
                .to_str()
                .unwrap_or("")])
            {
                crate::pictures::show(ui, path);
            } else {
                ui.label(egui::RichText::new("📄").size(32.0));
            }
            ui.add(egui::Label::new(&name).truncate());
        });
    });
    let rect = inner.response.rect;
    let id = ui.make_persistent_id(("asset", &rel));
    let response = ui
        .interact(rect, id, egui::Sense::click_and_drag())
        .on_hover_text(rel.display().to_string());
    if selected || response.hovered() {
        let color = if selected {
            THEME.selection
        } else {
            ui.visuals().weak_text_color()
        };
        ui.painter().rect_stroke(
            rect,
            4.0,
            egui::Stroke::new(1.0, color),
            egui::StrokeKind::Inside,
        );
    }
    if response.drag_started() && path.is_file() {
        egui::DragAndDrop::set_payload(ui.ctx(), AssetDrag(rel.clone()));
    }
    if path.is_dir() {
        drop_into_folder(&response, root, path, status);
    }
    if response.clicked() {
        state.selected = Some(rel.clone());
    }
    if response.double_clicked() {
        if path.is_dir() {
            state.folder = rel.clone();
        } else if is_room(&name) {
            world
                .resource_mut::<Editor>()
                .request(Command::Open(path.to_owned()));
        } else {
            *status = Some(format!("{name}: drag it into the picture or onto a field"));
        }
    }
    response.context_menu(|ui| {
        if ui.button("Rename").clicked() {
            state.renaming = Some((rel.clone(), name.clone()));
            ui.close();
        }
        if path.is_file() && ui.button("Copy").clicked() {
            *status = Some(match copy_file(path) {
                Ok(to) => format!("Copied to {}", relative(root, &to).display()),
                Err(error) => format!("Not copied: {error}"),
            });
            ui.close();
        }
        if ui.button("Delete").clicked() {
            state.deleting = Some((rel.clone(), users(root, path)));
            ui.close();
        }
        if ui.button("Show in Explorer").clicked() {
            show_in_explorer(path);
            ui.close();
        }
    });
}

/// A file of the panel dropped on a field with these extensions: its path, from the
/// asset folder. `rect`: the place of the field.
pub(crate) fn take_drop(ui: &Ui, rect: egui::Rect, extensions: &[&str]) -> Option<PathBuf> {
    let drag = egui::DragAndDrop::payload::<AssetDrag>(ui.ctx())?;
    if !has_end(&drag.0.to_string_lossy(), extensions) {
        return None;
    }
    let pointer = ui.ctx().pointer_latest_pos()?;
    // Only the shown part: a field scrolled out of view lies under another panel.
    if !rect.intersect(ui.clip_rect()).contains(pointer) {
        return None;
    }
    ui.painter().rect_stroke(
        rect,
        2.0,
        egui::Stroke::new(1.5, THEME.play),
        egui::StrokeKind::Outside,
    );
    if !ui.input(|input| input.pointer.any_released()) {
        return None;
    }
    egui::DragAndDrop::clear_payload(ui.ctx());
    Some(drag.0.clone())
}

/// A file of the panel dropped on the picture: a model or a prefab on the floor at the
/// drop, on the grid; a picture on the surface under the pointer. Called by the
/// overlay of the editor, whose clip rectangle is the picture.
pub(crate) fn drop_on_picture(ui: &Ui, world: &mut World) {
    let picture = ui.clip_rect();
    let extensions: Vec<&str> = MODELS
        .iter()
        .chain(&PICTURES)
        .copied()
        .chain(["prefab"])
        .collect();
    let Some(file) = take_drop(ui, picture, &extensions) else {
        return;
    };
    world.resource_scope(|world, mut editor: Mut<Editor>| {
        if editor.playing() {
            editor.status = "Stop the game to add a file".to_owned();
            return;
        }
        let Some(view) = crate::editor_view(world) else {
            return;
        };
        let Some(pointer) = ui.ctx().pointer_latest_pos() else {
            return;
        };
        let pixel = (pointer - picture.min) * ui.ctx().pixels_per_point();
        let ray = view.ray([pixel.x, pixel.y]);
        let name = file.to_string_lossy();
        if has_end(&name, &PICTURES) {
            let Some(Item::Object(index)) = crate::view::pick(world, &editor.document, &ray) else {
                editor.status = "Drop a picture on an object".to_owned();
                return;
            };
            let types = world.resource::<maki_scene::ComponentTypes>().clone();
            editor.status = match set_texture(&mut editor.document, index, &file, &types) {
                Ok(()) => format!("Texture {name}"),
                Err(why) => why,
            };
            return;
        }
        // The floor: the plane y = 0.
        let t = -ray.origin.y / ray.direction.y;
        let at = if t.is_finite() && t > 0.0 {
            ray.at(t)
        } else {
            Vec3::ZERO
        };
        let kind = match has_end(&name, &["prefab"]) {
            true => Kind::Prefab(file.clone()),
            false => Kind::Model(file.clone()),
        };
        editor.request(Command::AddAt(kind, at));
    });
}

/// Puts a picture file on the prop of an object as its texture.
fn set_texture(
    document: &mut crate::Document,
    index: usize,
    file: &Path,
    types: &maki_scene::ComponentTypes,
) -> Result<(), String> {
    let tree = document.component_tree(index, "prop", types);
    let Some(Ok(mut tree)) = tree else {
        return Err("Only an object with a mesh (prop) takes a texture".to_owned());
    };
    let path = Value::String(file.to_string_lossy().into_owned());
    let look = tree
        .as_object_mut()
        .and_then(|prop| prop.get_mut("look"))
        .and_then(|look| look.as_object_mut())
        .ok_or("The prop has no look")?;
    let texture = [("path".to_owned(), path)].into_iter().collect();
    look.insert("texture".into(), Value::Object(texture));
    document
        .set_component_tree(index, "prop", &tree, types)
        .map_err(|error| error.to_string())?;
    document.dirty = true;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("maki_assets_panel_{test}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("models")).unwrap();
        dir
    }

    #[test]
    fn a_move_takes_the_id_file_along_and_the_users_are_found_by_the_id() {
        let root = folder("move");
        let model = root.join("models/chair.glb");
        std::fs::write(&model, b"glb").unwrap();
        let id = FileId(0xabc);
        id.write(&model).unwrap();
        std::fs::write(root.join("a.ron"), format!("(path:\"x\",id:\"{id}\")")).unwrap();
        std::fs::write(root.join("b.ron"), "()").unwrap();
        assert_eq!(users(&root, &model), ["a.ron"]);

        let moved = root.join("seat.glb");
        move_file(&model, &moved).unwrap();
        assert_eq!(FileId::read(&moved), Some(id));
        assert!(!FileId::file_of(&model).exists());
        assert!(move_file(&root.join("a.ron"), &root.join("b.ron")).is_err());

        let copy = copy_file(&moved).unwrap();
        assert_eq!(copy, root.join("seat_2.glb"));
        assert_eq!(FileId::read(&copy), None);
        delete_file(&moved).unwrap();
        assert!(!FileId::file_of(&moved).exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_filter_tells_rooms_from_data_files() {
        let kind = |name: &str| KINDS.iter().position(|(kind, _)| *kind == name).unwrap();
        assert!(of_kind("room_a.ron", kind("Rooms")));
        assert!(!of_kind("key.item.ron", kind("Rooms")));
        assert!(of_kind("key.item.ron", kind("Data")));
        assert!(of_kind("chair.GLB", kind("Models")));
        assert!(of_kind("anything", kind("All")));
        assert_eq!(plain_name(" chair.glb "), Some("chair.glb"));
        for bad in ["", "../x", "a/b", r"a\b", "C:x", ".."] {
            assert_eq!(plain_name(bad), None, "{bad}");
        }
        assert_eq!(
            free_path(Path::new("no/such/key.item.ron")),
            Path::new("no/such/key.item.ron")
        );
    }
}
