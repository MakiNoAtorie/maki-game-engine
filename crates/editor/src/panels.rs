//! The panels of the editor, around the picture: at the top the menus and the row
//! with play, the tools, and the snap; at the left the outliner (each item of the
//! room, by name); at the right the properties of the selected item; under the
//! picture the event panel, while an event is selected; at the bottom the status
//! row. While the game is played, the running events and the game state take the
//! place of the outliner and the properties.
//!
//! A panel writes the document and the command queue of the [`Editor`], never the
//! entities of the room.

use std::path::{Path, PathBuf};

use bevy_ecs::world::{Mut, World};
use maki_assets::Wall;
use maki_core::App;
use maki_events::{Builtin, EventRunner, Problem};
use maki_scene::{MeshDef, SoundDef};
use maki_ui::egui::{self, Ui};
use maki_ui::{ToolPlace, add_modal, add_overlay, add_tool_panel, game_state_panel};

use crate::event_panel::{ERROR, HINT, NEVER_WRITTEN, event_panel, event_properties};
use crate::widgets::{choice, color, drag, look, mesh, name, number, pose, texture, vector};
use crate::{Answer, Command, Document, Editor, EditorSettings, Item, Kind, Tool, spawn_users};

/// The panel under the picture. Open while an event is selected (`Editor::apply_mode`).
pub(crate) const EVENT: &str = "Event";
pub(crate) const CONFIRM: &str = "Question";

pub(crate) fn build(app: &mut App) {
    // The text of the "Save as" field.
    let mut save_as = String::new();
    add_tool_panel(
        app,
        "Editor menu",
        ToolPlace::Top,
        true,
        move |ui, world| {
            with_editor(world, |world, editor| top(ui, world, editor, &mut save_as));
        },
    );
    // The text of the filter field of the outliner.
    let mut filter = String::new();
    add_tool_panel(app, "Outliner", ToolPlace::Left, true, move |ui, world| {
        with_editor(world, |world, editor| {
            outliner(ui, world, editor, &mut filter)
        });
    });
    // The text of the name field of the game state.
    let mut new_name = String::new();
    add_tool_panel(
        app,
        "Properties",
        ToolPlace::Right,
        true,
        move |ui, world| {
            let scroll = egui::ScrollArea::vertical().auto_shrink(false);
            scroll.show(ui, |ui| {
                // In a game: what the game changes, in place of what the editor changes.
                if world.resource::<Editor>().playing() {
                    ui.strong("Game state");
                    game_state_panel(ui, world, &mut new_name);
                    return;
                }
                with_editor(world, |world, editor| properties(ui, world, editor));
            });
        },
    );
    add_tool_panel(app, EVENT, ToolPlace::Bottom, false, |ui, world| {
        with_editor(world, |world, editor| {
            let Item::Event(index) = editor.primary() else {
                return;
            };
            if event_panel(ui, world, editor, index) == Some(true) {
                editor.document.dirty = true;
            }
        });
    });
    add_tool_panel(app, "Status", ToolPlace::Status, true, |ui, world| {
        with_editor(world, |_, editor| status_bar(ui, editor));
    });
    add_overlay(app, "Editor marks", true, |ui, world| {
        with_editor(world, |world, editor| {
            crate::overlay::draw(ui, world, editor)
        });
    });
    // Open while a command waits for the answer (`Editor::apply_mode`).
    // In the middle of the window, and nothing else gets input: the room does
    // not change while the question waits.
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

/// The rows at the top of the window: the menus, and the row with play, the tools,
/// and the snap. In a game: stop, and the steps of a test.
fn top(ui: &mut Ui, world: &mut World, editor: &mut Editor, save_as: &mut String) {
    // A drag of a value, or a text that is typed: the history waits for its end.
    let pointer_down = ui.ctx().input(|input| input.pointer.any_down());
    editor.busy = pointer_down || ui.ctx().memory(|memory| memory.focused().is_some());
    let playing = editor.playing();
    ui.horizontal(|ui| {
        if !playing {
            ui.menu_button("File", |ui| file_menu(ui, editor, save_as));
            ui.menu_button("Edit", |ui| edit_menu(ui, world, editor));
        }
        ui.menu_button("View", |ui| {
            if ui
                .button("Debug panels")
                .on_hover_text("Help, Stats, Debug, Gameplay, Game state, Bench: one window")
                .clicked()
            {
                editor.request(Command::DebugPanels);
                ui.close();
            }
        });
    });
    ui.horizontal(|ui| {
        if playing {
            play_row(ui, world, editor);
        } else {
            tools(ui, world, editor);
        }
    });
    ui.add_space(2.0);
}

/// The commands of the "Edit" menu, with what each needs.
fn edit_menu(ui: &mut Ui, world: &World, editor: &mut Editor) {
    let some = !editor.selection.is_empty();
    let (undo, redo) = editor.history_steps();
    let buttons = [
        ("Undo", undo > 0, Command::Undo, crate::UNDO),
        ("Redo", redo > 0, Command::Redo, crate::REDO),
        ("Duplicate", some, Command::Duplicate, crate::DUPLICATE),
        ("Delete", some, Command::Delete, crate::DELETE),
        ("Copy command rows", some, Command::Copy, crate::COPY),
        ("Paste command rows", some, Command::Paste, crate::PASTE),
        ("Frame in the view", true, Command::Frame, crate::FRAME),
    ];
    for (text, enabled, command, action) in buttons {
        let button = egui::Button::new(text).shortcut_text(crate::key_of(world, action));
        if ui.add_enabled(enabled, button).clicked() {
            editor.request(command);
            ui.close();
        }
    }
}

/// The row of a game: stop, and in a test the steps.
fn play_row(ui: &mut Ui, world: &mut World, editor: &mut Editor) {
    if ui.button("Stop").clicked() {
        editor.request(Command::Stop);
    }
    // A test: the events wait for a step before each command.
    let mut runner = world.resource_mut::<EventRunner>();
    if runner.step_mode() {
        if ui
            .button("Step")
            .on_hover_text("The next command of the event")
            .clicked()
        {
            runner.step_command();
        }
        if ui
            .button("Run")
            .on_hover_text("No more steps: the events run")
            .clicked()
        {
            runner.set_step_mode(false);
        }
    } else {
        ui.weak("or the quit key");
    }
}

/// The row of the editor: play, the tool of the gizmo, the snap, and "Add".
fn tools(ui: &mut Ui, world: &mut World, editor: &mut Editor) {
    // The key of an action, from the binding table, as the hover text of its button.
    let key = |action| crate::key_of(world, action);
    if ui.button("Play").on_hover_text(key(crate::PLAY)).clicked() {
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
    ui.separator();
    for (tool, text, action) in [
        (Tool::Move, "Move", crate::TOOL_MOVE),
        (Tool::Rotate, "Rotate", crate::TOOL_ROTATE),
        (Tool::Scale, "Scale", crate::TOOL_SCALE),
    ] {
        ui.selectable_value(&mut editor.tool, tool, text)
            .on_hover_text(key(action));
    }
    let local = format!(
        "The handles follow the axes of the item, not those of the room ({})",
        key(crate::TOGGLE_LOCAL)
    );
    ui.checkbox(&mut editor.local, "Local").on_hover_text(local);
    ui.separator();
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
    ui.separator();
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

/// The left panel: each item of the room. In a game: what each event does now.
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
    let field = egui::TextEdit::singleline(filter)
        .hint_text("filter")
        .desired_width(f32::INFINITY);
    ui.add(field);
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            let problems = editor.problems();
            for (title, items) in sections(&editor.document) {
                let Some(title) = title else {
                    rows(ui, editor, items, &problems, "");
                    continue;
                };
                egui::CollapsingHeader::new(format!("{title} ({})", items.len()))
                    .id_salt(title)
                    .default_open(true)
                    .show(ui, |ui| rows(ui, editor, items, &problems, filter));
            }
        });
}

/// The row at the bottom of the window: what the last command did, or in red why a
/// command did not do its work (until the user closes it), and the count of the
/// problems of the events. A click on the count lists them; a click on one goes to
/// its event, page, and command row.
fn status_bar(ui: &mut Ui, editor: &mut Editor) {
    ui.horizontal(|ui| {
        let mut closed = false;
        match &editor.error {
            Some(error) => {
                closed = ui.small_button("x").on_hover_text("Close").clicked();
                ui.colored_label(ERROR, error.as_str());
            }
            None => {
                ui.label(editor.status.as_str());
            }
        }
        if closed {
            editor.error = None;
        }
        let problems = editor.problems();
        let warnings = problems.iter().filter(|problem| problem.warning).count();
        let errors = problems.len() - warnings;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
    editor: &mut Editor,
    items: Vec<(Item, String)>,
    problems: &[Problem],
    filter: &str,
) {
    let filter = filter.to_lowercase();
    for (item, label) in items {
        if !label.to_lowercase().contains(&filter) {
            continue;
        }
        let selected =
            editor.selection.contains(&item) || (item == Item::Room && editor.selection.is_empty());
        let of_item = |problem: &&Problem| Item::Event(problem.event) == item;
        let found: Vec<&Problem> = problems.iter().filter(of_item).collect();
        let text = if found.is_empty() {
            egui::RichText::new(label)
        } else {
            let error = found.iter().any(|problem| !problem.warning);
            egui::RichText::new(label).color(if error { ERROR } else { HINT })
        };
        let mut row = ui.selectable_label(selected, text);
        if !found.is_empty() {
            let texts: Vec<&str> = found.iter().map(|problem| problem.text.as_str()).collect();
            row = row.on_hover_text(texts.join("\n"));
        }
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
            for (text, command) in [
                ("Frame in the view", Command::Frame),
                ("Duplicate", Command::Duplicate),
                ("Delete", Command::Delete),
            ] {
                if ui.button(text).clicked() {
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
                | vector(ui, "position", &mut light.position, 0.01)
                | number(ui, "radius", &mut light.radius, 0.005, 0.01..=10.0)
                | color(ui, "color", &mut light.color)
                | number(ui, "intensity", &mut light.intensity, 0.1, 0.0..=10000.0)
                | ui.checkbox(&mut light.on, "on").changed()
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
                                |ui, to| vector(ui, "", to, 0.01),
                            )
                    })
                }
                // A lamp that hums. An event finds the sound by the name of the light.
                | crate::widgets::optional(ui, "sound", &mut light.sound, SoundDef::default, |ui, sound| {
                    crate::widgets::file(ui, "sound file", &mut sound.file, &crate::widgets::SOUNDS)
                        | number(ui, "volume", &mut sound.volume, 0.01, 0.0..=4.0)
                        | number(ui, "reach (radius)", &mut sound.reach, 0.05, 0.1..=100.0)
                        | ui.checkbox(&mut sound.on, "on at the start").changed()
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
                document.rename_in_events(&registry, |command: &Builtin| match command {
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
        // `event_panel` shows an event.
        Item::Event(_) => return None,
    })
}
