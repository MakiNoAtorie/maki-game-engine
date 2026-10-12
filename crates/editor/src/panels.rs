//! The panels of the editor, in the shell of `maki_ui`: the menus of the menu row;
//! in the tool row the tools and the snap at the left, and play and stop in the
//! middle; the outliner (each item of the room, by name) in the left dock; the
//! inspector (the values of the selected item) in the right dock; the events panel in
//! the bottom dock; the status row. While the game is played, the running events and
//! the game state take the place of the outliner and the inspector.
//!
//! A panel writes the document and the command queue of the [`Editor`], never the
//! entities of the room. A button of a command is made from the command list
//! (`maki_ui::ToolCommands`): its name and its key are written at one place.

use std::path::{Path, PathBuf};

use bevy_ecs::world::{Mut, World};
use maki_assets::Wall;
use maki_core::App;
use maki_events::{EventRunner, Problem};
use maki_scene::{MeshDef, PropDef, SoundDef};
use maki_ui::egui::{self, Ui};
use maki_ui::theme::icon;
use maki_ui::widgets::{
    TreeRow, frame_time_label, jobs_label, property_grid, tool_button, tree_filter, tree_group,
    tree_row, tree_shows,
};
use maki_ui::{
    Layout, THEME, ToolCommands, ToolPanel, ToolPlace, add_menu, add_modal, add_overlay,
    add_tool_panel, game_state_panel,
};

use crate::event_panel::{ERROR, HINT, NEVER_WRITTEN, event_panel, event_properties};
use crate::widgets::{
    choice, color, drag, look, mesh, name, number, number_or, pose, texture, toggle, vector,
};
use crate::{Answer, Command, Document, Editor, EditorSettings, Item, Kind, Tool};

pub(crate) const OUTLINER: &str = "outliner";
pub(crate) const INSPECTOR: &str = "inspector";
/// The panel of the pages and the commands of the selected event.
pub(crate) const EVENT: &str = "events";
pub(crate) const CONFIRM: &str = "Question";

/// The named layouts. With the panels of now, "Animate" is as "Build": a later
/// panel (the timeline) makes the difference.
pub(crate) const LAYOUTS: [Layout; 5] = [
    // To make a room: the outliner, the inspector, and the events.
    layout("Build", &[], false, 280.0),
    // To write events: more room for the event panel.
    layout("Logic", &[], false, 440.0),
    // To set light and materials: the picture has the room of the event panel.
    layout("Look", &[EVENT], false, 280.0),
    layout("Animate", &[], false, 280.0),
    // Each debug panel of the game, as tabs under the picture.
    layout("Debug", &[], true, 360.0),
];

const fn layout(
    name: &'static str,
    closed: &'static [&'static str],
    debug: bool,
    bottom: f32,
) -> Layout {
    Layout {
        name,
        closed,
        debug,
        sizes: [240.0, 340.0, bottom],
    }
}

pub(crate) fn build(app: &mut App) {
    let panel = |id, title, icon, place| ToolPanel {
        id,
        title,
        icon,
        place,
        open: true,
    };
    // The menus: their commands are in the command list (`crate::add_commands`).
    // The text of the "Save as" field.
    let mut save_as = String::new();
    let file = move |ui: &mut Ui, world: &mut World| {
        with_editor(world, |_, editor| file_menu(ui, editor, &mut save_as))
    };
    add_menu(app, "File", Some(Box::new(file)));
    let edit =
        |ui: &mut Ui, world: &mut World| with_editor(world, |_, editor| prefab_menu(ui, editor));
    add_menu(app, "Edit", Some(Box::new(edit)));
    let add =
        |ui: &mut Ui, world: &mut World| with_editor(world, |_, editor| add_files(ui, editor));
    add_menu(app, "Add", Some(Box::new(add)));
    add_menu(app, "View", None);

    add_tool_panel(
        app,
        panel("editor tools", "Tools", "", ToolPlace::Top),
        tools,
    );
    let play = panel("editor play", "Play", "", ToolPlace::TopMiddle);
    add_tool_panel(app, play, play_row);
    // The text of the filter field of the outliner.
    let mut filter = String::new();
    let outliner_panel = panel(OUTLINER, "Outliner", icon::OUTLINER, ToolPlace::Left);
    add_tool_panel(app, outliner_panel, move |ui, world| {
        with_editor(world, |world, editor| {
            outliner(ui, world, editor, &mut filter)
        });
    });
    // The text of the name field of the game state.
    let mut new_name = String::new();
    let inspector = panel(INSPECTOR, "Inspector", icon::INSPECTOR, ToolPlace::Right);
    add_tool_panel(app, inspector, move |ui, world| {
        let scroll = egui::ScrollArea::vertical().auto_shrink(false);
        scroll.show(ui, |ui| {
            // In a game: what the game changes, in place of what the editor changes.
            if world.resource::<Editor>().playing() {
                ui.strong("Game state");
                game_state_panel(ui, world, &mut new_name);
                return;
            }
            with_editor(world, |world, editor| {
                property_grid(ui, |ui| properties(ui, world, editor))
            });
        });
    });
    let events = panel(EVENT, "Events", icon::EVENTS, ToolPlace::Bottom);
    add_tool_panel(app, events, |ui, world| {
        with_editor(world, |world, editor| {
            let index = match editor.primary() {
                Item::Event(index) if !editor.playing() => index,
                _ if editor.playing() => {
                    ui.weak("A game runs. The outliner has the events that run.");
                    return;
                }
                _ => {
                    ui.weak(
                        "Select an event in the outliner: its pages and its commands show here.",
                    );
                    return;
                }
            };
            if event_panel(ui, world, editor, index) == Some(true) {
                editor.document.dirty = true;
            }
        });
    });
    let status = panel("editor status", "Status", "", ToolPlace::Status);
    add_tool_panel(app, status, |ui, world| {
        with_editor(world, |world, editor| status_bar(ui, world, editor));
    });
    add_overlay(app, "Editor marks", true, |ui, world| {
        with_editor(world, |world, editor| {
            crate::overlay::draw(ui, world, editor)
        });
    });
    // Open while a command waits for the answer (`Editor::apply_mode`).
    // In the middle of the window, and nothing else gets input: the room does
    // not change while the question waits. The one window that must be answered:
    // its answer cannot be undone.
    add_modal(app, CONFIRM, false, |ui, world| {
        with_editor(world, |_, editor| {
            let unsaved = [
                ("Save", Answer::Save),
                ("Do not save", Answer::DoNotSave),
                ("Cancel", Answer::Cancel),
            ];
            let replace = [("Replace", Answer::Replace), ("Cancel", Answer::Cancel)];
            let (text, answers) = match editor.pending() {
                Some(Command::SaveAs(path)) => (
                    format!("{} is a file of another room.", file_name(path)),
                    &replace[..],
                ),
                _ => (
                    format!(
                        "{} has changes that are not saved.",
                        file_name(&editor.document.path)
                    ),
                    &unsaved[..],
                ),
            };
            ui.label(text);
            // Enter is the first answer, Esc is "Cancel".
            let key = |key| ui.input(|input| input.key_pressed(key));
            let mut answer = None;
            if key(egui::Key::Enter) {
                answer = Some(answers[0].1);
            }
            if key(egui::Key::Escape) {
                answer = Some(Answer::Cancel);
            }
            ui.horizontal(|ui| {
                for (text, value) in answers {
                    if ui.button(*text).clicked() {
                        answer = Some(*value);
                    }
                }
            });
            ui.weak("Enter: the first answer. Esc: cancel.");
            if let Some(answer) = answer {
                editor.request(Command::Answer(answer));
            }
        });
    });
}

fn with_editor(world: &mut World, draw: impl FnOnce(&mut World, &mut Editor)) {
    world.resource_scope(|world, mut editor: Mut<Editor>| draw(world, &mut editor));
}

/// The glTF files in the directory of the room file and one level below it, as a
/// room file names them: from that directory, with `/`.
fn model_files(room: &Path) -> Vec<PathBuf> {
    crate::widgets::files(
        room.parent().unwrap_or(Path::new("")),
        &crate::widgets::MODELS,
    )
}

/// The row of a command of the command list in a menu of a panel: its name and its
/// key. True: it was clicked. The caller asks the editor for the command: the
/// editor is not in the world here, so the command itself cannot run.
fn command_row(ui: &mut Ui, world: &World, id: &str) -> bool {
    let commands = world.resource::<ToolCommands>().clone();
    let Some(command) = commands.get(id) else {
        return false;
    };
    let button = egui::Button::new(command.name).shortcut_text(command.key(world));
    ui.add(button).clicked()
}

/// Under the commands of the "Edit" menu: the name field of "Make prefab".
fn prefab_menu(ui: &mut Ui, editor: &mut Editor) {
    let is_part = |item: &Item| matches!(item, Item::Prop(_) | Item::Light(_));
    let parts = !editor.playing() && editor.selection.iter().any(is_part);
    ui.horizontal(|ui| {
        let field = egui::TextEdit::singleline(&mut editor.prefab_name)
            .desired_width(120.0)
            .hint_text("name of the prefab");
        ui.add(field);
        let name = editor.prefab_name.trim().to_owned();
        let button = ui
            .add_enabled(parts && !name.is_empty(), egui::Button::new("Make prefab"))
            .on_hover_text(
                "The selected props and lights become the file prefabs/<name>.prefab, and \
                 one prefab in their place. A file of that name is replaced: each room \
                 that names it shows the change. \"Unpack prefab\" puts the props and \
                 lights of the selected prefab back into the room, to change them.",
            );
        if button.clicked() {
            editor.request(Command::MakePrefab(name));
            ui.close();
        }
    });
}

/// The middle of the tool row: play, and where the game starts. In a game: stop,
/// and in a test the steps.
fn play_row(ui: &mut Ui, world: &mut World) {
    if world.resource::<Editor>().playing() {
        let stop = egui::RichText::new(format!("{} Stop", icon::STOP)).color(THEME.play);
        tool_button(ui, world, "editor.stop", stop, false);
        // A test: the events wait for a step before each command.
        if world.resource::<EventRunner>().step_mode() {
            tool_button(
                ui,
                world,
                "editor.step",
                format!("{} Step", icon::STEP),
                false,
            );
            tool_button(
                ui,
                world,
                "editor.run",
                format!("{} Run", icon::PLAY),
                false,
            );
        }
        return;
    }
    tool_button(
        ui,
        world,
        "editor.play",
        format!("{} Play", icon::PLAY),
        false,
    );
    // Where the game starts.
    let mut editor = world.resource_mut::<Editor>();
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
        })
        .response
        .on_hover_text("The spawn point where the game starts");
}

/// The left of the tool row: the tool of the gizmo, and the snap.
fn tools(ui: &mut Ui, world: &mut World) {
    // A drag of a value, or a text that is typed: the history waits for its end.
    let pointer_down = ui.ctx().input(|input| input.pointer.any_down());
    let busy = pointer_down || ui.ctx().memory(|memory| memory.focused().is_some());
    let mut editor = world.resource_mut::<Editor>();
    editor.busy = busy;
    let (tool, playing) = (editor.tool, editor.playing());
    for (id, icon, of_button) in [
        ("editor.tool_move", icon::MOVE, Tool::Move),
        ("editor.tool_rotate", icon::ROTATE, Tool::Rotate),
        ("editor.tool_scale", icon::SCALE, Tool::Scale),
    ] {
        tool_button(ui, world, id, icon, tool == of_button);
    }
    tool_button(
        ui,
        world,
        "editor.local",
        "Local",
        world.resource::<Editor>().local,
    );
    ui.separator();
    ui.add_enabled_ui(!playing, |ui| {
        let old = world.resource::<EditorSettings>();
        let mut settings = old.clone();
        ui.checkbox(&mut settings.snap, "Snap")
            .on_hover_text(format!(
                "A drag of a handle snaps to the steps ({} in a drag: the other way)",
                maki_ui::key_label(world, crate::SNAP_SWITCH)
            ));
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
}

/// Under the commands of the "Add" menu: the model files and the prefab files.
fn add_files(ui: &mut Ui, editor: &mut Editor) {
    ui.add_enabled_ui(!editor.playing(), |ui| {
        ui.menu_button("Model file", |ui| {
            let models = model_files(&editor.document.path);
            if models.is_empty() {
                ui.weak("No .gltf or .glb file at the room file");
            }
            let directory = editor.document.path.parent().unwrap_or(Path::new(""));
            let directory = directory.to_owned();
            for path in models {
                let row = ui.horizontal(|ui| {
                    crate::pictures::show(ui, &directory.join(&path));
                    ui.button(path.to_string_lossy()).clicked()
                });
                if row.inner {
                    editor.request(Command::Add(Kind::Model(path)));
                    ui.close();
                }
            }
        });
        ui.menu_button("Prefab file", |ui| {
            let directory = editor.document.path.parent().unwrap_or(Path::new(""));
            let prefabs = crate::widgets::files(directory, &crate::widgets::PREFABS);
            if prefabs.is_empty() {
                ui.weak("No .prefab file at the room file. Edit > Make prefab writes one.");
            }
            for path in prefabs {
                if ui.button(path.to_string_lossy()).clicked() {
                    editor.request(Command::Add(Kind::Prefab(path)));
                    ui.close();
                }
            }
        });
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

/// Under the commands of the "File" menu: the room files to open, and "Save as".
fn file_menu(ui: &mut Ui, editor: &mut Editor, save_as: &mut String) {
    ui.add_enabled_ui(!editor.playing(), |ui| {
        ui.menu_button("Open", |ui| {
            for path in room_files(editor.rooms()) {
                if ui.button(file_name(&path)).clicked() {
                    editor.request(Command::Open(path));
                    ui.close();
                }
            }
        });
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
    });
}

/// The outliner: each item of the room. In a game: what each event does now.
fn outliner(ui: &mut Ui, world: &mut World, editor: &mut Editor, filter: &mut String) {
    if editor.playing() {
        ui.strong("Running events");
        let runner = world.resource::<EventRunner>();
        // In a test: the command that waits for a step.
        for event in runner.running() {
            let command = event
                .current()
                .map_or(String::new(), |command| format!("{command:?}"));
            ui.label(format!("{}: {command}", event.name));
        }
        if runner.running().is_empty() {
            ui.weak("none");
        }
        return;
    }
    tree_filter(ui, filter);
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            let problems = editor.problems();
            for (title, items) in sections(&editor.document) {
                let Some(title) = title else {
                    rows(ui, world, editor, items, &problems, "");
                    continue;
                };
                tree_group(ui, title, items.len(), |ui| {
                    rows(ui, world, editor, items, &problems, filter)
                });
            }
        });
    // A row that does not show (a filter, a folded group) is not renamed later.
    editor.renaming = None;
}

/// The status row, in four parts: what the last command did; the jobs that run; the
/// count of the problems of the events (a click lists them, and a click on one goes
/// to its event, page, and command row); the frame time.
fn status_bar(ui: &mut Ui, world: &World, editor: &mut Editor) {
    ui.label(editor.status.as_str());
    ui.separator();
    jobs_label(ui, world);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        frame_time_label(ui, world);
        ui.separator();
        let problems = editor.problems();
        let warnings = problems.iter().filter(|problem| problem.warning).count();
        let errors = problems.len() - warnings;
        if problems.is_empty() {
            ui.weak("no problem in the events");
            return;
        }
        let color = if errors > 0 { ERROR } else { HINT };
        let count = format!("{errors} errors, {warnings} warnings in the events");
        ui.menu_button(egui::RichText::new(count).color(color), |ui| {
            for problem in &problems {
                let event = &editor.document.events[problem.event];
                let place = match &problem.command {
                    Some((page, _)) => format!("{}, page {}", event.name, page + 1),
                    None => event.name.clone(),
                };
                let color = if problem.warning { HINT } else { ERROR };
                let text = format!("{place}: {}", problem.text);
                if ui.button(egui::RichText::new(text).color(color)).clicked() {
                    go_to_problem(editor, problem);
                    ui.close();
                }
            }
        });
    });
}

/// Selects the event of a problem, and shows its page with its command row selected.
fn go_to_problem(editor: &mut Editor, problem: &Problem) {
    editor.select(Item::Event(problem.event), false);
    let Some((page, command)) = &problem.command else {
        return;
    };
    let commands = &editor.document.events[problem.event].pages[*page].commands;
    let registry = &editor.registry;
    let rows = crate::events::rows(commands, registry);
    let of_command = |row: &crate::Row| {
        row.kind == crate::RowKind::Command && row.command(commands, registry) == Some(command)
    };
    let row = rows.into_iter().map(|row| row.0).find(of_command);
    editor.event_ui.go_to(problem.event, *page, row);
}

/// The rows of one list of the outliner. `filter`: only the items whose label has
/// this text. An event with a problem is red (an error) or yellow, with the
/// problems as its hover text.
fn rows(
    ui: &mut Ui,
    world: &World,
    editor: &mut Editor,
    items: Vec<(Item, String)>,
    problems: &[Problem],
    filter: &str,
) {
    for (item, label) in items {
        if !tree_shows(filter, &label) {
            continue;
        }
        let selected =
            editor.selection.contains(&item) || (item == Item::Room && editor.selection.is_empty());
        let of_item = |problem: &&Problem| Item::Event(problem.event) == item;
        let found: Vec<&Problem> = problems.iter().filter(of_item).collect();
        let error = found.iter().any(|problem| !problem.warning);
        let texts: Vec<&str> = found.iter().map(|problem| problem.text.as_str()).collect();
        let row = TreeRow {
            label: &label,
            selected,
            color: (!found.is_empty()).then_some(if error { ERROR } else { HINT }),
            hover: &texts.join("\n"),
            rename: editor.renaming.as_ref() == Some(&item),
        };
        // The index of an item changes with a delete: the salt is the item.
        let row = tree_row(ui, &item, row);
        if let Some(new) = row.renamed {
            editor.rename(&item, new);
        }
        let row = row.response;
        if row.clicked() {
            // The room is "nothing selected". Shift adds, as in the picture.
            let add = ui.ctx().input(|input| input.modifiers.shift);
            match &item {
                Item::Room => {
                    editor.selection.clear();
                    editor.event_ui.focused = false;
                }
                item => editor.select(item.clone(), add),
            }
        }
        if row.double_clicked() {
            editor.request(Command::Frame);
        }
        if item == Item::Room {
            continue;
        }
        // The menu of the right mouse button: the commands for this item.
        row.context_menu(|ui| {
            for (id, command) in [
                ("editor.rename", Command::Rename),
                ("editor.frame", Command::Frame),
                ("editor.duplicate", Command::Duplicate),
                ("editor.delete", Command::Delete),
            ] {
                if command == Command::Rename && !item.has_name() {
                    continue;
                }
                if command_row(ui, world, id) {
                    if !editor.selection.contains(&item) {
                        editor.select(item.clone(), false);
                    }
                    editor.event_ui.focused = false;
                    editor.request(command);
                    ui.close();
                }
            }
        });
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
    let prefabs = room.prefabs.iter().enumerate().map(|(index, prefab)| {
        (
            Item::Prefab(index),
            named(&prefab.name, &file_name(&prefab.file), index),
        )
    });
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
        (Some("Prefabs"), prefabs.collect()),
        (Some("Spawn points"), spawns.collect()),
        (Some("Events"), events.collect()),
    ]
}

/// Each flag and each variable of the game, with each place that reads or writes
/// it. A place in the open room is a link: a click goes to the event and its page.
fn where_used(ui: &mut Ui, editor: &mut Editor) {
    let others = editor.others().to_vec();
    let names = crate::names(&editor.document, &others, &editor.registry);
    let own = crate::events::file_name(&editor.document);
    let mut go_to = None;
    for (kind, names) in [("Flags", &names.flags), ("Variables", &names.variables)] {
        ui.label(kind);
        for (name, uses) in names {
            let writes = uses.iter().filter(|place| place.writes).count();
            let title = format!("{name}: {} reads, {writes} writes", uses.len() - writes);
            // A name that no event writes: a wrong letter, or a flag for a test.
            let title = if writes == 0 {
                egui::RichText::new(format!("{title}: {NEVER_WRITTEN}")).color(HINT)
            } else {
                egui::RichText::new(title)
            };
            // The name is the identity: the header stays unfolded when a count changes.
            egui::CollapsingHeader::new(title)
                .id_salt((kind, name))
                .show(ui, |ui| {
                    for place in uses {
                        let access = if place.writes { "writes" } else { "reads" };
                        let (room, event, page, text) =
                            (&place.room, &place.event, place.page, &place.text);
                        let line = format!("{access}: {room}, {event}, page {page}: {text}");
                        if *room != own {
                            ui.label(line);
                        } else if ui.link(line).clicked() {
                            go_to = Some((event.clone(), page - 1));
                        }
                    }
                });
        }
        if names.is_empty() {
            ui.weak("none");
        }
    }
    if let Some((event, page)) = go_to {
        let events = editor.document.events.iter();
        if let Some(index) = events.into_iter().position(|other| other.name == event) {
            editor.select(Item::Event(index), false);
            editor.event_ui.go_to(index, page, None);
        }
    }
}

fn properties(ui: &mut Ui, _world: &mut World, editor: &mut Editor) {
    // For the file pickers of the widgets.
    let directory = editor.document.path.parent().unwrap_or(Path::new(""));
    crate::widgets::set_room_directory(ui, directory);
    let item = editor.primary();
    if editor.selection.len() > 1 {
        ui.weak(format!(
            "{} items are selected. The last one:",
            editor.selection.len()
        ));
    }
    // `Some`: the name of the item was changed, from this one.
    let mut old_name = None;
    let shown = match &item {
        Item::Event(index) => event_properties(ui, editor, *index, &mut old_name),
        item => item_properties(ui, editor, item, &mut old_name),
    };
    let Some(mut changed) = shown else {
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

    ui.separator();
    egui::CollapsingHeader::new("Where used").show(ui, |ui| where_used(ui, editor));
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
            // The labels of the outliner.
            let doorways = room.doorways.iter().enumerate();
            let labels: Vec<String> = doorways
                .map(|(index, doorway)| format!("doorway {index} ({:?})", doorway.wall))
                .collect();
            let door = room.doors.get_mut(*index)?;
            let doorways: Vec<(&str, usize)> = labels.iter().map(String::as_str).zip(0..).collect();
            name(ui, &mut door.name)
                | choice(ui, "doorway", &mut door.doorway, &doorways, PartialEq::eq)
                | number(ui, "seconds to open", &mut door.seconds, 0.05, 0.05..=60.0)
                | color(ui, "color", &mut door.color)
                | toggle(ui, "open at the start", &mut door.open, Some(false))
        }
        Item::Prop(index) => {
            let prop = room.props.get_mut(*index)?;
            let is_file = matches!(prop.mesh, MeshDef::File(_));
            name(ui, &mut prop.name)
                | mesh(ui, &mut prop.mesh)
                | pose(ui, "pose", &mut prop.pose)
                // A file has its own colors and materials.
                | (!is_file && look(ui, &mut prop.look))
                | toggle(ui, "blocks the player", &mut prop.collider, Some(true))
                | (is_file && {
                    let default = Some(PropDef::default().light_radius);
                    let radius = &mut prop.light_radius;
                    number_or(ui, "radius of its lights", radius, default, 0.01, 0.01..=10.0)
                })
        }
        Item::Prefab(index) => {
            let prefab = room.prefabs.get_mut(*index)?;
            let files = &crate::widgets::PREFABS;
            name(ui, &mut prefab.name)
                | crate::widgets::file(ui, "prefab file", &mut prefab.file, files)
                | pose(ui, "pose", &mut prefab.pose)
        }
        Item::Mover(index) => {
            let mover = room.movers.get_mut(*index)?;
            ui.checkbox(
                &mut editor.second_pose,
                "the gizmo changes the second pose (b) only",
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
                | vector(ui, "position", &mut light.position, None, 0.01)
                | number(ui, "radius", &mut light.radius, 0.005, 0.01..=10.0)
                | color(ui, "color", &mut light.color)
                | number(ui, "intensity", &mut light.intensity, 0.1, 0.0..=10000.0)
                | toggle(ui, "on", &mut light.on, Some(true))
                // One group: a direction is of use only with a cone.
                | {
                    let (cone, direction) = (&mut light.cone, &mut light.direction);
                    crate::widgets::optional(ui, "cone", cone, || 30.0, |ui, cone| {
                        number(ui, "degrees to the edge", cone, 0.5, 1.0..=180.0)
                            | crate::widgets::optional(
                                ui,
                                "direction (not set: down)",
                                direction,
                                down,
                                |ui, to| vector(ui, "", to, None, 0.01),
                            )
                    })
                }
                // A lamp that hums. An event finds the sound by the name of the light.
                | crate::widgets::optional(ui, "sound", &mut light.sound, SoundDef::default, |ui, sound| {
                    let default = SoundDef::default();
                    crate::widgets::file(ui, "sound file", &mut sound.file, &crate::widgets::SOUNDS)
                        | number_or(ui, "volume", &mut sound.volume, Some(default.volume), 0.01, 0.0..=4.0)
                        | number_or(ui, "reach (radius)", &mut sound.reach, Some(default.reach), 0.05, 0.1..=100.0)
                        | toggle(ui, "on at the start", &mut sound.on, Some(default.on))
                })
        }
        Item::Spawn(spawn) => {
            let mut new = spawn.clone();
            let renamed =
                crate::widgets::text(ui, "name", &mut new) && editor.rename_spawn(spawn, &new);
            let now = if renamed { &new } else { spawn };
            let position = editor.document.room.spawns.get_mut(now)?;
            renamed | vector(ui, "position", position, None, 0.01)
        }
        // `event_panel` shows an event.
        Item::Event(_) => return None,
    })
}
