//! The panels of the editor: at the left of the picture the file menu, play and
//! stop, and the outliner (each item of the room, by name); in the column at the
//! right the properties of the selected item; and the question for changes that are
//! not saved.
//!
//! A panel writes the document and the command queue of the [`Editor`], never the
//! entities of the room.

use std::path::{Path, PathBuf};

use bevy_ecs::world::{Mut, World};
use maki_assets::Wall;
use maki_core::App;
use maki_events::Builtin;
use maki_scene::MeshDef;
use maki_ui::egui::{self, Ui};
use maki_ui::{Panels, add_left_panel, add_overlay, add_panel, add_window};

use crate::widgets::{choice, color, drag, look, mesh, name, number, pose, texture, vector};
use crate::{Answer, Command, Document, Editor, EditorSettings, Item, Kind, Tool, spawn_users};

const OUTLINER: &str = "Editor";
const PROPERTIES: &str = "Properties";
pub(crate) const CONFIRM: &str = "Changes are not saved";

pub(crate) fn build(app: &mut App) {
    // The text of the "Save as" field.
    let mut save_as = String::new();
    add_left_panel(app, OUTLINER, true, move |ui, world| {
        with_editor(world, |world, editor| {
            outliner(ui, world, editor, &mut save_as)
        });
    });
    add_panel(app, PROPERTIES, true, |ui, world| {
        with_editor(world, |_, editor| properties(ui, editor));
    });
    app.resource_mut::<Panels>().unfold(PROPERTIES);
    add_overlay(app, "Editor marks", true, |ui, world| {
        with_editor(world, |world, editor| {
            crate::overlay::draw(ui, world, editor)
        });
    });
    // Open while a command waits for the answer (`Editor::apply_mode`).
    add_window(app, CONFIRM, false, |ui, world| {
        with_editor(world, |_, editor| {
            ui.label(format!("{} has changes.", file_name(&editor.document.path)));
            ui.horizontal(|ui| {
                let answers = [
                    ("Save", Answer::Save),
                    ("Do not save", Answer::DoNotSave),
                    ("Cancel", Answer::Cancel),
                ];
                for (text, answer) in answers {
                    if ui.button(text).clicked() {
                        editor.request(Command::Answer(answer));
                    }
                }
            });
        });
    });
}

fn with_editor(world: &mut World, draw: impl FnOnce(&mut World, &mut Editor)) {
    world.resource_scope(|world, mut editor: Mut<Editor>| draw(world, &mut editor));
}

/// The glTF files in the directory of the room file and one level below it, as a
/// room file names them: from that directory, with `/`.
fn model_files(room: &Path) -> Vec<PathBuf> {
    let directory = room.parent().unwrap_or(Path::new(""));
    let files_of = |directory: &Path| -> Vec<PathBuf> {
        let entries = std::fs::read_dir(directory).into_iter().flatten().flatten();
        entries.map(|entry| entry.path()).collect()
    };
    let top = files_of(directory);
    let below = top
        .iter()
        .filter(|path| path.is_dir())
        .flat_map(|path| files_of(path));
    let is_model = |path: &PathBuf| {
        let extension = path.extension().and_then(|extension| extension.to_str());
        matches!(extension, Some("gltf" | "glb"))
    };
    let mut models: Vec<PathBuf> = top.iter().cloned().chain(below).filter(is_model).collect();
    models.sort();
    let relative = |path: PathBuf| {
        path.strip_prefix(directory)
            .map(Path::to_owned)
            .unwrap_or(path)
    };
    let models = models.into_iter().map(relative);
    models
        .map(|path| PathBuf::from(path.to_string_lossy().replace('\\', "/")))
        .collect()
}

/// The second row of the left panel: the tool of the gizmo, the snap, and the
/// commands that change the list of the items.
fn tools(ui: &mut Ui, world: &mut World, editor: &mut Editor) {
    ui.horizontal(|ui| {
        for (tool, text) in [
            (Tool::Move, "Move"),
            (Tool::Rotate, "Rotate"),
            (Tool::Scale, "Scale"),
        ] {
            ui.selectable_value(&mut editor.tool, tool, text);
        }
        ui.checkbox(&mut editor.local, "Local")
            .on_hover_text("The handles follow the axes of the item, not those of the room");
    });
    ui.horizontal(|ui| {
        let old = world.resource::<EditorSettings>();
        let mut settings = old.clone();
        ui.checkbox(&mut settings.snap, "Snap");
        ui.add(
            drag(&mut settings.grid_step)
                .speed(0.01)
                .range(0.01..=10.0)
                .suffix(" m"),
        );
        ui.add(
            drag(&mut settings.angle_step)
                .speed(0.5)
                .range(1.0..=90.0)
                .suffix(" deg"),
        );
        // Only a different value is a write.
        if settings != *old {
            *world.resource_mut::<EditorSettings>() = settings;
        }
    });
    ui.horizontal(|ui| {
        ui.menu_button("Add", |ui| {
            for (text, kind) in Kind::MENU {
                if ui.button(text).clicked() {
                    editor.request(Command::Add(kind));
                    ui.close();
                }
            }
            ui.menu_button("Model file", |ui| {
                let models = model_files(&editor.document.path);
                if models.is_empty() {
                    ui.weak("No .gltf or .glb file at the room file");
                }
                for path in models {
                    if ui.button(path.to_string_lossy()).clicked() {
                        editor.request(Command::Add(Kind::Model(path)));
                        ui.close();
                    }
                }
            });
        });
        let some = !editor.selection.is_empty();
        let (undo, redo) = editor.history_steps();
        let buttons = [
            ("Delete", some, Command::Delete),
            ("Copy", some, Command::Duplicate),
            ("Undo", undo > 0, Command::Undo),
            ("Redo", redo > 0, Command::Redo),
        ];
        for (text, enabled, command) in buttons {
            if ui.add_enabled(enabled, egui::Button::new(text)).clicked() {
                editor.request(command);
            }
        }
    });
}

fn file_name(path: &Path) -> String {
    let name = path.file_name().unwrap_or(path.as_os_str());
    name.to_string_lossy().into_owned()
}

/// The `.ron` files of a directory, in the order of their names.
fn room_files(directory: &Path) -> Vec<PathBuf> {
    let entries = std::fs::read_dir(directory).into_iter().flatten().flatten();
    let mut files: Vec<_> = entries
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "ron"))
        .collect();
    files.sort();
    files
}

fn file_menu(ui: &mut Ui, editor: &mut Editor, save_as: &mut String) {
    if ui.button("New room").clicked() {
        editor.request(Command::New);
        ui.close();
    }
    ui.menu_button("Open", |ui| {
        for path in room_files(editor.rooms()) {
            if ui.button(file_name(&path)).clicked() {
                editor.request(Command::Open(path));
                ui.close();
            }
        }
    });
    if ui.button("Save").clicked() {
        editor.request(Command::Save);
        ui.close();
    }
    ui.horizontal(|ui| {
        let field = egui::TextEdit::singleline(save_as)
            .desired_width(120.0)
            .hint_text("name.ron");
        ui.add(field);
        let name = save_as.trim();
        if ui.button("Save as").clicked() && !name.is_empty() {
            let path = editor.rooms().join(name).with_extension("ron");
            editor.request(Command::SaveAs(path));
            ui.close();
        }
    });
    if ui.button("Quit").clicked() {
        editor.request(Command::Quit);
        ui.close();
    }
}

fn outliner(ui: &mut Ui, world: &mut World, editor: &mut Editor, save_as: &mut String) {
    // A drag of a value, or a text that is typed: the history waits for its end.
    let pointer_down = ui.ctx().input(|input| input.pointer.any_down());
    editor.busy = pointer_down || ui.ctx().memory(|memory| memory.focused().is_some());
    ui.horizontal(|ui| {
        if editor.playing() {
            if ui.button("Stop").clicked() {
                editor.request(Command::Stop);
            }
            ui.weak("or the quit key");
            return;
        }
        ui.menu_button("File", |ui| file_menu(ui, editor, save_as));
        if ui.button("Play").clicked() {
            editor.request(Command::Play);
        }
        // Where the game starts.
        let spawns: Vec<String> = editor.document.room.spawns.keys().cloned().collect();
        let chosen = editor
            .play_from
            .as_ref()
            .filter(|name| spawns.contains(name));
        let current = chosen.or(spawns.first()).cloned();
        egui::ComboBox::from_id_salt("play from")
            .selected_text(current.as_deref().unwrap_or("at the view"))
            .show_ui(ui, |ui| {
                for spawn in spawns {
                    let selected = current.as_ref() == Some(&spawn);
                    if ui.selectable_label(selected, spawn.as_str()).clicked() {
                        editor.play_from = Some(spawn);
                    }
                }
            });
    });
    let changed = if editor.document.dirty { " *" } else { "" };
    ui.strong(format!("{}{changed}", file_name(&editor.document.path)));
    ui.weak(editor.status.as_str());
    ui.separator();

    let playing = editor.playing();
    ui.add_enabled_ui(!playing, |ui| {
        tools(ui, world, editor);
        ui.separator();
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                for (title, items) in sections(&editor.document) {
                    let Some(title) = title else {
                        rows(ui, editor, items);
                        continue;
                    };
                    egui::CollapsingHeader::new(format!("{title} ({})", items.len()))
                        .id_salt(title)
                        .default_open(true)
                        .show(ui, |ui| rows(ui, editor, items));
                }
            });
    });
}

fn rows(ui: &mut Ui, editor: &mut Editor, items: Vec<(Item, String)>) {
    for (item, label) in items {
        let selected =
            editor.selection.contains(&item) || (item == Item::Room && editor.selection.is_empty());
        if ui.selectable_label(selected, label).clicked() {
            // The room is "nothing selected". Shift adds, as in the picture.
            let add = ui.ctx().input(|input| input.modifiers.shift);
            match item {
                Item::Room => editor.selection.clear(),
                item => editor.select(item, add),
            }
        }
    }
}

/// One list of the outliner: its title (none for the room itself), and its items
/// with their labels.
type Section = (Option<&'static str>, Vec<(Item, String)>);

/// The lists of the outliner: the title of each, and its items with their labels. An
/// item with no name has its kind and its index.
fn sections(document: &Document) -> Vec<Section> {
    let room = &document.room;
    let named = |name: &Option<String>, kind: &str, index: usize| match name {
        Some(name) => name.clone(),
        None => format!("{kind} {index}"),
    };
    let doorways = room.doorways.iter().enumerate().map(|(index, doorway)| {
        let label = format!("doorway {index} ({:?})", doorway.wall);
        (Item::Doorway(index), label)
    });
    let doors = room
        .doors
        .iter()
        .enumerate()
        .map(|(index, door)| (Item::Door(index), named(&door.name, "door", index)));
    let props = room.props.iter().enumerate().map(|(index, prop)| {
        let kind = match &prop.mesh {
            MeshDef::Cube(_) => "cube".to_owned(),
            MeshDef::Cuboid(..) => "cuboid".to_owned(),
            MeshDef::Sphere { .. } => "sphere".to_owned(),
            MeshDef::File(path) => file_name(path),
        };
        (Item::Prop(index), named(&prop.name, &kind, index))
    });
    let movers = room
        .movers
        .iter()
        .enumerate()
        .map(|(index, mover)| (Item::Mover(index), named(&mover.name, "mover", index)));
    let lights = room
        .lights
        .iter()
        .enumerate()
        .map(|(index, light)| (Item::Light(index), named(&light.name, "light", index)));
    let spawns = room.spawns.keys();
    let spawns = spawns.map(|name| (Item::Spawn(name.clone()), name.clone()));
    let events = document.events.iter().enumerate();
    let events = events.map(|(index, event)| (Item::Event(index), event.name.clone()));
    vec![
        (None, vec![(Item::Room, "Room".to_owned())]),
        (Some("Doorways"), doorways.collect()),
        (Some("Doors"), doors.collect()),
        (Some("Props"), props.collect()),
        (Some("Movers"), movers.collect()),
        (Some("Lights"), lights.collect()),
        (Some("Spawn points"), spawns.collect()),
        (Some("Events"), events.collect()),
    ]
}

fn properties(ui: &mut Ui, editor: &mut Editor) {
    if editor.playing() {
        ui.weak("Stop the game to change the room.");
        return;
    }
    let item = editor.primary();
    if editor.selection.len() > 1 {
        ui.weak(format!(
            "{} items are selected. The last one:",
            editor.selection.len()
        ));
    }
    // `Some`: the name of the item was changed, from this one.
    let mut old_name = None;
    let Some(mut changed) = item_properties(ui, editor, &item, &mut old_name) else {
        ui.weak("The room has no such item.");
        return;
    };
    if let Some(old) = old_name {
        let registry = editor.registry.clone();
        let more = editor.document.renamed(&item, old, &registry);
        if !more.is_empty() {
            editor.status = more;
        }
    }

    // The comment lines of the item in the room file.
    ui.separator();
    ui.label("Notes");
    let document = &mut editor.document;
    let mut notes = document.notes_of(&item).join("\n");
    let field = egui::TextEdit::multiline(&mut notes)
        .desired_rows(2)
        .desired_width(f32::INFINITY);
    if ui.add(field).changed() {
        let lines = notes.split('\n').map(str::to_owned);
        let lines = if notes.is_empty() {
            Vec::new()
        } else {
            lines.collect()
        };
        document.set_notes(&item, lines);
        changed = true;
    }
    if changed {
        document.dirty = true;
    }
}

/// The widgets of one item. True: a value was changed. `None`: the document has no
/// such item.
fn item_properties(
    ui: &mut Ui,
    editor: &mut Editor,
    item: &Item,
    old_name: &mut Option<Option<String>>,
) -> Option<bool> {
    const SIZE: std::ops::RangeInclusive<f32> = 0.1..=100.0;
    let document = &mut editor.document;
    let room = &mut document.room;
    // The name field of an item that events find by its name.
    let mut name = |ui: &mut Ui, value: &mut Option<String>| {
        let old = value.clone();
        let changed = name(ui, value);
        if changed {
            *old_name = Some(old);
        }
        changed
    };
    Some(match item {
        Item::Room => {
            number(ui, "width (X)", &mut room.width, 0.05, 1.0..=100.0)
                | number(ui, "depth (Z)", &mut room.depth, 0.05, 1.0..=100.0)
                | number(ui, "height (Y)", &mut room.height, 0.05, 1.0..=100.0)
                | texture(ui, "floor texture", &mut room.floor)
                | texture(ui, "wall texture", &mut room.walls)
        }
        Item::Doorway(index) => {
            let doorway = room.doorways.get_mut(*index)?;
            let walls = [
                ("NegX (west)", Wall::NegX),
                ("PosX (east)", Wall::PosX),
                ("NegZ (back)", Wall::NegZ),
                ("PosZ (front)", Wall::PosZ),
            ];
            choice(ui, "wall", &mut doorway.wall, &walls, PartialEq::eq)
                | number(ui, "offset", &mut doorway.offset, 0.05, -50.0..=50.0)
                | number(ui, "width", &mut doorway.width, 0.05, SIZE)
                | number(ui, "height", &mut doorway.height, 0.05, SIZE)
                | number(ui, "passage depth", &mut doorway.depth, 0.05, SIZE)
        }
        Item::Door(index) => {
            let count = room.doorways.len();
            let door = room.doors.get_mut(*index)?;
            let labels: Vec<String> = (0..count).map(|index| index.to_string()).collect();
            let doorways: Vec<(&str, usize)> = labels.iter().map(String::as_str).zip(0..).collect();
            name(ui, &mut door.name)
                | choice(ui, "doorway", &mut door.doorway, &doorways, PartialEq::eq)
                | number(ui, "seconds to open", &mut door.seconds, 0.05, 0.05..=60.0)
                | color(ui, "color", &mut door.color)
                | ui.checkbox(&mut door.open, "open at the start").changed()
        }
        Item::Prop(index) => {
            let prop = room.props.get_mut(*index)?;
            let is_file = matches!(prop.mesh, MeshDef::File(_));
            name(ui, &mut prop.name)
                | mesh(ui, &mut prop.mesh)
                | pose(ui, "pose", &mut prop.pose)
                // A file has its own colors and materials.
                | (!is_file && look(ui, &mut prop.look))
                | ui.checkbox(&mut prop.collider, "blocks the player").changed()
                | (is_file
                    && number(ui, "radius of its lights", &mut prop.light_radius, 0.01, 0.01..=10.0))
        }
        Item::Mover(index) => {
            let mover = room.movers.get_mut(*index)?;
            ui.checkbox(
                &mut editor.second_pose,
                "the gizmo moves the second pose (b)",
            );
            name(ui, &mut mover.name)
                | mesh(ui, &mut mover.mesh)
                | look(ui, &mut mover.look)
                | pose(ui, "pose at the start (a)", &mut mover.a)
                | pose(ui, "second pose (b)", &mut mover.b)
                | number(
                    ui,
                    "seconds from a to b",
                    &mut mover.seconds,
                    0.05,
                    0.05..=600.0,
                )
        }
        Item::Light(index) => {
            let light = room.lights.get_mut(*index)?;
            let down = || [0.0, -1.0, 0.0];
            name(ui, &mut light.name)
                | vector(ui, "position", &mut light.position, 0.01)
                | number(ui, "radius", &mut light.radius, 0.005, 0.01..=10.0)
                | color(ui, "color", &mut light.color)
                | number(ui, "intensity", &mut light.intensity, 0.1, 0.0..=10000.0)
                | ui.checkbox(&mut light.on, "on").changed()
                | crate::widgets::optional(
                    ui,
                    "cone",
                    &mut light.cone,
                    || 30.0,
                    |ui, cone| number(ui, "degrees to the edge", cone, 0.5, 1.0..=180.0),
                )
                | crate::widgets::optional(ui, "direction", &mut light.direction, down, |ui, to| {
                    vector(ui, "", to, 0.01)
                })
        }
        Item::Spawn(spawn) => {
            let mut new = spawn.clone();
            let renamed =
                crate::widgets::text(ui, "name", &mut new) && document.rename_spawn(spawn, &new);
            let now = if renamed { &new } else { spawn };
            let position = document.room.spawns.get_mut(now)?;
            let moved = vector(ui, "position", position, 0.01);
            if renamed {
                // A `Transfer` of this room to itself follows. Other room files are
                // not changed: the user gets their names.
                let file = file_name(&document.path);
                let registry = editor.registry.clone();
                document.rename_in_events(&registry, |command| match command {
                    Builtin::Transfer(room, to) if *room == file && to == spawn => {
                        Some(Builtin::Transfer(room.clone(), new.clone()))
                    }
                    _ => None,
                });
                let users = spawn_users(&editor.rooms, &document.path, spawn, &registry);
                if !users.is_empty() {
                    let users = users.join(", ");
                    editor.status = format!("{users}: a Transfer still names `{spawn}`");
                }
                editor.selection = vec![Item::Spawn(new)];
            }
            renamed | moved
        }
        // The event editor is a later stage: the name and the area, and what the
        // pages are, with no widget.
        Item::Event(index) => {
            let event = document.events.get_mut(*index)?;
            let old = event.name.clone();
            let mut changed = crate::widgets::text(ui, "name", &mut event.name);
            if changed {
                *old_name = Some(Some(old));
            }
            let area = &mut event.area;
            for (label, value, least) in [
                ("area center (X, Z)", &mut area.center, -100.0),
                ("area size (X, Z)", &mut area.size, 0.0),
            ] {
                ui.horizontal(|ui| {
                    ui.label(label);
                    for part in value {
                        let drag = drag(part).speed(0.01).range(least..=100.0);
                        changed |= ui.add(drag).changed();
                    }
                });
            }
            for (number, page) in event.pages.iter().enumerate() {
                let title = format!("page {number}: {:?}", page.trigger);
                egui::CollapsingHeader::new(title)
                    .default_open(true)
                    .show(ui, |ui| {
                        for condition in &page.conditions {
                            ui.label(format!("if {condition:?}"));
                        }
                        for command in &page.commands {
                            ui.label(format!("{command:?}"));
                        }
                    });
            }
            changed
        }
    })
}
