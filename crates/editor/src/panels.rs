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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bevy_ecs::world::{Mut, World};
use maki_core::fields::Value;
use maki_core::{App, FieldTypes};
use maki_events::{EventRunner, Problem};
use maki_scene::ComponentTypes;
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
use crate::fields::Lists;
use crate::widgets::drag;
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
    // The text of the name field of "New routine".
    let mut routine_name = String::new();
    let mut new_data = crate::data_panel::NewData::default();
    let assets = move |ui: &mut Ui, world: &mut World| {
        with_editor(world, |world, editor| {
            crate::routine_panel::routine_menu(ui, editor, &mut routine_name);
            crate::data_panel::data_menu(ui, world, editor, &mut new_data);
        });
    };
    add_menu(app, "Assets", Some(Box::new(assets)));
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
            if !editor.playing() && crate::routine_panel::routine_panel(ui, world, editor) {
                return;
            }
            let index = match editor.selected_event() {
                Some(index) if !editor.playing() => index,
                _ if editor.playing() => {
                    ui.weak("A game runs. The outliner has the events that run.");
                    return;
                }
                _ => {
                    // An object with no event: a button adds one.
                    if let Item::Object(index) = editor.primary() {
                        if ui.button("Add an event").clicked() {
                            editor.document.add_event(index);
                            editor.document.dirty = true;
                        }
                    } else {
                        ui.weak(
                            "Select an object with an event: its pages and its commands show here.",
                        );
                    }
                    return;
                }
            };
            if event_panel(ui, world, editor, index) == Some(true) {
                editor.document.dirty = true;
            }
        });
    });
    crate::text_panel::add(app);
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
fn model_files(room: &Path, converters: &crate::sources::Converters) -> Vec<PathBuf> {
    crate::widgets::files(
        room.parent().unwrap_or(Path::new("")),
        &crate::widgets::MODELS,
        converters,
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
    let is_part = |item: &Item| matches!(item, Item::Object(_));
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
    let spawns = crate::spawn_points(&editor.document.room);
    let current = crate::play_spawn(&spawns, editor.play_from.as_ref()).cloned();
    let shown = current
        .as_ref()
        .map_or("at the view", |(_, name)| name.as_str());
    egui::ComboBox::from_id_salt("play from")
        .selected_text(shown)
        .show_ui(ui, |ui| {
            for (spawn, name) in spawns {
                let selected = current.as_ref().is_some_and(|(chosen, _)| *chosen == spawn);
                if ui.selectable_label(selected, name.as_str()).clicked() {
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
            let models = model_files(&editor.document.path, &editor.converters);
            if models.is_empty() {
                ui.weak("No .gltf, .glb, .blend, or .obj file at the room file");
            }
            let directory = editor.document.path.parent().unwrap_or(Path::new(""));
            let directory = directory.to_owned();
            for path in models {
                let row = ui.horizontal(|ui| {
                    crate::pictures::show(ui, &directory.join(&path));
                    crate::widgets::source_mark(ui, &directory.join(&path), &editor.converters);
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
            let prefabs =
                crate::widgets::files(directory, &crate::widgets::PREFABS, &editor.converters);
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
                let object = editor.document.room.objects.get(problem.object);
                let name = object.and_then(|object| object.name.clone());
                let name = name.unwrap_or_default();
                let place = match &problem.command {
                    Some((page, _)) => format!("{name}, page {}", page + 1),
                    None => name,
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

/// Selects the object of the event of a problem, and shows its page with its
/// command row selected.
fn go_to_problem(editor: &mut Editor, problem: &Problem) {
    editor.select(Item::Object(problem.object), false);
    let Some((page, command)) = &problem.command else {
        return;
    };
    let registry = &editor.registry;
    let Some(Ok(event)) = editor.document.event(problem.object, registry) else {
        return;
    };
    let Some(commands) = event.pages.get(*page).map(|page| &page.commands) else {
        return;
    };
    let rows = crate::events::rows(commands, registry, &[]);
    let of_command = |row: &crate::Row| {
        row.kind == crate::RowKind::Command && row.command(commands, registry) == Some(command)
    };
    let row = rows.into_iter().map(|row| row.0).find(of_command);
    editor.event_ui.go_to(problem.object, *page, row);
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
        let of_item = |problem: &&Problem| Item::Object(problem.object) == item;
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
            // The focus stays on a clicked row and draws a mark on it after the selection moves.
            row.surrender_focus();
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
    // An object with no name: the name of its first component.
    let objects = room.objects.iter().enumerate().map(|(index, object)| {
        let kind = object
            .components
            .keys()
            .next()
            .map_or("object", String::as_str);
        (Item::Object(index), named(&object.name, kind, index))
    });
    vec![
        (None, vec![(Item::Room, "Room".to_owned())]),
        (Some("Objects"), objects.collect()),
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
        let events = editor.document.events(&editor.registry);
        if let Some(found) = events.iter().find(|other| other.name == event) {
            editor.select(Item::Object(found.object), false);
            editor.event_ui.go_to(found.object, page, None);
        }
    }
}

fn properties(ui: &mut Ui, world: &mut World, editor: &mut Editor) {
    // For the file pickers of the widgets.
    let directory = editor.document.path.parent().unwrap_or(Path::new(""));
    crate::widgets::set_room_directory(ui, directory, &editor.converters);
    if let Some(types) = world.get_resource::<maki_assets::DataTypes>()
        && crate::data_panel::data_inspector(ui, types, editor)
    {
        return;
    }
    let item = editor.primary();
    if editor.selection.len() > 1 {
        ui.weak(format!(
            "{} items are selected. The last one:",
            editor.selection.len()
        ));
    }
    let mut renamed = false;
    let shown = item_properties(ui, world, editor, &item, &mut renamed);
    let Some(mut changed) = shown else {
        ui.weak("The room has no such item.");
        return;
    };
    if renamed {
        let more = editor.document.keep_name_unique(&item);
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

/// The rows of one item, from the table of its fields (`maki_core::fields`): no
/// field of an item kind has its name here. True: a value was changed. `None`: the
/// document has no such item.
fn item_properties(
    ui: &mut Ui,
    world: &World,
    editor: &mut Editor,
    item: &Item,
    renamed: &mut bool,
) -> Option<bool> {
    // What is not a field of an item kind.
    match item {
        item if editor.document.mover(item).is_some() => {
            let label = "the gizmo changes the second pose (b) only";
            ui.checkbox(&mut editor.second_pose, label);
        }
        _ => {}
    }

    let (type_name, mut tree) = editor.document.tree(item)?;
    let types = world.get_resource::<FieldTypes>();
    let Some(table) = types.and_then(|types| types.get(type_name)) else {
        ui.weak(format!(
            "No module gave the type \"{type_name}\" its fields."
        ));
        return Some(false);
    };
    let name_before = tree.get("name").cloned();
    let objects = crate::widgets::object_names(&editor.document.room.objects);
    let mut changed = false;
    if crate::fields::show(ui, &table.kind, &mut tree, Lists::objects(&objects)) {
        *renamed = tree.get("name") != name_before.as_ref();
        match editor.document.set_tree(item, tree) {
            Ok(()) => changed = true,
            Err(error) => editor.status = format!("The change was not taken: {error:#}"),
        }
    }
    if let Item::Object(index) = item {
        changed |= components(ui, world, editor, *index);
    }
    Some(changed)
}

/// A group of rows for each component of an object, from the table of its type. A
/// component with no type shows its text, and stays as it is. True: a value was
/// changed.
fn components(ui: &mut Ui, world: &World, editor: &mut Editor, index: usize) -> bool {
    let no_types = ComponentTypes::default();
    let types = world.get_resource::<ComponentTypes>().unwrap_or(&no_types);
    let tables = world.get_resource::<FieldTypes>();
    let names: Vec<String> = match editor.document.room.objects.get(index) {
        Some(object) => object.components.keys().cloned().collect(),
        None => return false,
    };
    let mut changed = false;
    for name in names {
        ui.separator();
        ui.strong(&name);
        // The pages and the commands have the panel "Events".
        if name == "event" {
            event_properties(ui, editor, index);
            continue;
        }
        let tree = editor.document.component_tree(index, &name, types);
        let table = tables.and_then(|tables| tables.get(&name));
        // No type of the name, or a text that the type does not read: the reason.
        let readable = match (tree, table) {
            (Some(Ok(tree)), Some(table)) => Ok((tree, table)),
            (Some(Err(reason)), _) => Err(reason),
            _ => Err(format!("no module gave the type \"{name}\" its fields")),
        };
        let (mut tree, table) = match readable {
            Ok(readable) => readable,
            Err(reason) => {
                ui.weak(format!("{reason}: the text is kept as it is."));
                let object = &editor.document.room.objects[index];
                ui.monospace(object.components[&name].as_str());
                continue;
            }
        };
        // An id of its own: egui makes the id of a widget from its label, and two
        // components can have a field of the same name.
        let objects = crate::widgets::object_names(&editor.document.room.objects);
        let shown = ui.push_id(&name, |ui| {
            crate::fields::show(ui, &table.kind, &mut tree, Lists::objects(&objects))
        });
        if !shown.inner {
            continue;
        }
        match editor
            .document
            .set_component_tree(index, &name, &tree, types)
        {
            Ok(()) => changed = true,
            Err(error) => editor.status = format!("The change was not taken: {error:#}"),
        }
    }
    changed | instance(ui, tables, editor, index, types)
}

/// Takes the value of the prefab file again for `field` of the change `key` (all of
/// it for an empty `field`). A change with no field left goes.
fn reset(changes: &mut BTreeMap<String, Value>, key: &str, field: &str) {
    match changes.get_mut(key) {
        Some(Value::Object(fields)) if !field.is_empty() => {
            fields.remove(field);
            if fields.is_empty() {
                changes.remove(key);
            }
        }
        _ => drop(changes.remove(key)),
    }
}

/// The objects of the prefab file of an instance, a group for each of their
/// components with the changes of the instance merged in. A changed field has a
/// mark and a button that takes the value of the file again. True: a change.
// ponytail: the prefab file is read in each frame while its instance is selected; a
// cache by the time of the file if it shows in a profile.
fn instance(
    ui: &mut Ui,
    tables: Option<&FieldTypes>,
    editor: &mut Editor,
    index: usize,
    types: &ComponentTypes,
) -> bool {
    let Some(Ok(used)) = editor.document.room.objects[index].prefab() else {
        return false;
    };
    let directory = editor.document.path.parent().unwrap_or(Path::new(""));
    let prefab = match crate::ops::prefab_to_edit(&used.file.resolve(directory)) {
        Ok(prefab) => prefab,
        Err(error) => {
            ui.weak(format!("{error:#}"));
            return false;
        }
    };
    let mut changes = used.changes.clone();
    // A reference in a prefab file names an object of that file.
    let objects = crate::widgets::object_names(&prefab.objects);
    for (number, object) in prefab.objects.iter().enumerate() {
        ui.separator();
        let label = object
            .name
            .clone()
            .unwrap_or_else(|| format!("object {number}"));
        ui.strong(format!("{label} (of the prefab)"));
        for (name, text) in &object.components {
            let Some(key) = maki_scene::change_key(object, name) else {
                ui.weak(format!("{name}: the object has no id in the prefab file"));
                continue;
            };
            let (Some(kind), Some(table)) = (types.get(name), tables.and_then(|t| t.get(name)))
            else {
                ui.weak(format!("{name}: no module registered this type"));
                continue;
            };
            let Ok(base) = (kind.tree)(text) else {
                ui.weak(format!("{name}: the file has a value that does not read"));
                continue;
            };
            ui.label(name);
            let mut tree = base.clone();
            if let Some(change) = changes.get(&key) {
                maki_scene::merge(&mut tree, change);
            }
            let shown = ui.push_id(&key, |ui| {
                crate::fields::show(ui, &table.kind, &mut tree, Lists::objects(&objects))
            });
            if shown.inner {
                match maki_scene::difference(&base, &tree) {
                    Some(change) => changes.insert(key.clone(), change),
                    None => changes.remove(&key),
                };
            }
            // The marks of the changed fields, each with its reset.
            let fields: Vec<String> = match changes.get(&key) {
                Some(Value::Object(fields)) => fields.keys().cloned().collect(),
                Some(_) => vec![String::new()],
                None => Vec::new(),
            };
            for field in fields {
                ui.horizontal(|ui| {
                    let what = if field.is_empty() { "all" } else { &field };
                    ui.colored_label(HINT, format!("changed: {what}"));
                    if ui.button("reset").clicked() {
                        reset(&mut changes, &key, &field);
                    }
                });
            }
        }
    }
    if changes == used.changes {
        return false;
    }
    let object = &mut editor.document.room.objects[index];
    let prefab = maki_scene::PrefabFile {
        file: used.file,
        changes,
    };
    object.set_component("prefab", &prefab);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reset_takes_the_value_of_the_file_again() {
        let text = r#"{"1/light": {"intensity": 3.0, "radius": 0.2}, "2/prop": {"Cube": 1.0}}"#;
        let mut changes: BTreeMap<String, Value> =
            maki_scene::ron_options().from_str(text).unwrap();
        reset(&mut changes, "1/light", "intensity");
        assert_eq!(changes["1/light"].as_object().unwrap().len(), 1);
        reset(&mut changes, "1/light", "radius");
        assert!(!changes.contains_key("1/light"));
        reset(&mut changes, "2/prop", "");
        assert!(changes.is_empty());
    }
}
