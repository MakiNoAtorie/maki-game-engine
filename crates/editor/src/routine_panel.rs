//! A routine file in the event panel: its arguments at the left, its command rows in
//! the middle, the arguments of the selected row at the right. A change is written to
//! the file at once: the watch of a game that runs loads it again.

use std::path::{Path, PathBuf};

use bevy_ecs::world::World;
use maki_events::{CommandRegistry, ROUTINE_EXTENSION, RoutineDef, read_routine, routine_text};
use maki_ui::egui::{self, Ui};
use maki_ui::theme::icon;

use crate::Editor;
use crate::event_panel::{
    ArgumentEditors, cached_live_lists, command_arguments, command_rows, lists_and_problems,
};

/// The folder of the routine files, in the directory of the room files.
const FOLDER: &str = "routines";

/// A routine file that the event panel edits in place of an event.
#[derive(Debug, Clone, PartialEq)]
pub struct RoutineEdit {
    pub path: PathBuf,
    pub routine: RoutineDef,
}

impl RoutineEdit {
    pub fn open(path: &Path, registry: &CommandRegistry) -> Result<Self, String> {
        Ok(Self {
            path: path.to_owned(),
            routine: read_routine(path, registry)?,
        })
    }

    pub fn save(&self) -> Result<(), String> {
        std::fs::write(&self.path, routine_text(&self.routine))
            .map_err(|error| format!("{} not written: {error}", self.path.display()))
    }
}

/// Writes an empty routine file `routines/<name>.routine.ron` in `directory`: its
/// path. `Err`: why no file was written.
pub fn new_routine(directory: &Path, name: &str) -> Result<PathBuf, String> {
    let name = name.trim();
    let letters = |c: char| c.is_alphanumeric() || c == '_' || c == '-' || c == ' ';
    if name.is_empty() || !name.chars().all(letters) {
        return Err(
            "Use letters, numbers, spaces, `_`, and `-` in the name of a routine.".to_owned(),
        );
    }
    let path = directory
        .join(FOLDER)
        .join(format!("{name}.{ROUTINE_EXTENSION}"));
    if path.exists() {
        return Err(format!("The routine \"{name}\" exists already."));
    }
    let written = std::fs::create_dir_all(directory.join(FOLDER))
        .and_then(|()| std::fs::write(&path, routine_text(&RoutineDef::default())));
    written.map_err(|error| format!("{} not written: {error}", path.display()))?;
    Ok(path)
}

/// The first `argumentN` that no argument has.
fn new_argument_name(arguments: &[String]) -> String {
    (1..)
        .map(|number| format!("argument{number}"))
        .find(|name| !arguments.contains(name))
        .expect("an endless range")
}

impl Editor {
    /// Opens a routine file in the event panel.
    pub fn open_routine(&mut self, path: &Path) {
        match RoutineEdit::open(path, &self.registry) {
            Ok(edit) => {
                self.routine = Some(edit);
                self.menu = None;
                self.event_ui.row = None;
            }
            Err(error) => self.error = Some(error),
        }
    }
}

/// Under the commands of the "Assets" menu: the routine files to open, and "New
/// routine" with its name field.
pub(crate) fn routine_menu(ui: &mut Ui, editor: &mut Editor, name: &mut String) {
    let playing = editor.playing();
    ui.add_enabled_ui(!playing, |ui| {
        let directory = editor.document.directory();
        let open = editor.routine.as_ref().map(|edit| edit.path.clone());
        ui.menu_button("Open routine", |ui| {
            let files =
                crate::widgets::files(&directory, &crate::widgets::ROUTINES, &editor.converters);
            if files.is_empty() {
                ui.weak("No routines yet. Use New routine.");
            }
            for path in files {
                let full = directory.join(&path);
                let selected = open.as_ref() == Some(&full);
                if ui
                    .selectable_label(selected, path.to_string_lossy())
                    .clicked()
                {
                    editor.open_routine(&full);
                    ui.close();
                }
            }
        });
        let made = maki_ui::widgets::name_row(ui, name, "Routine name", "New routine", !playing);
        if let Some(made) = made {
            match new_routine(&directory, &made) {
                Ok(path) => {
                    editor.open_routine(&path);
                    editor.status = format!("Routine \"{made}\" created.");
                }
                Err(error) => editor.error = Some(error),
            }
        }
    })
    .response
    .on_disabled_hover_text("Stop the game to open or create a routine.");
}

/// The event panel with a routine file open. True: the panel showed it.
pub(crate) fn routine_panel(ui: &mut Ui, world: &mut World, editor: &mut Editor) -> bool {
    if editor.routine.is_none() {
        return false;
    }
    let registry = editor.registry.clone();
    // No object: the lists of the room, and no problem of an event.
    let (mut lists, _) = lists_and_problems(editor, None);
    let directory = editor.document.directory();
    let (room, cache) = (&editor.document.room, &mut editor.event_ui.live);
    cached_live_lists(world, room, &directory, cache, &mut lists);
    let editors = world
        .get_resource::<ArgumentEditors>()
        .cloned()
        .unwrap_or_default();
    crate::widgets::set_room_directory(ui, &directory, &editor.converters);
    let Editor {
        routine,
        event_ui,
        document,
        error,
        ..
    } = editor;
    let edit = routine.as_mut().expect("tested above");
    let room = &document.room;
    let (mut changed, mut close) = (false, false);
    let frame = egui::Frame::NONE.inner_margin(6);
    let scroll = |salt: &'static str| egui::ScrollArea::both().id_salt(salt).auto_shrink(false);
    egui::Panel::left("routine arguments")
        .resizable(true)
        .frame(frame)
        .show(ui, |ui| {
            scroll("routine names").show(ui, |ui| {
                ui.horizontal(|ui| {
                    close = ui.button("Close").clicked();
                    let file = edit.path.strip_prefix(&directory).unwrap_or(&edit.path);
                    let name = file.file_name().unwrap_or(file.as_os_str());
                    ui.strong(name.to_string_lossy())
                        .on_hover_text(file.to_string_lossy());
                });
                ui.strong("Arguments");
                ui.weak("Whole numbers. Each one hides the variable of the same name.");
                let mut removed = None;
                let names = edit.routine.arguments.clone();
                for (index, name) in edit.routine.arguments.iter_mut().enumerate() {
                    let taken =
                        name.is_empty() || names.iter().filter(|other| *other == name).count() > 1;
                    ui.horizontal(|ui| {
                        changed |= ui.text_edit_singleline(name).changed();
                        if taken {
                            ui.colored_label(ui.visuals().error_fg_color, "!")
                                .on_hover_text("The name is empty or used twice.");
                        }
                        if ui
                            .button(icon::CLOSE)
                            .on_hover_text("Remove argument")
                            .clicked()
                        {
                            removed = Some(index);
                        }
                    });
                }
                if let Some(index) = removed {
                    edit.routine.arguments.remove(index);
                    changed = true;
                }
                if ui.button("Add argument").clicked() {
                    let name = new_argument_name(&edit.routine.arguments);
                    edit.routine.arguments.push(name);
                    changed = true;
                }
            });
        });
    let list = &mut edit.routine.commands;
    egui::Panel::left("routine commands")
        .resizable(true)
        .frame(frame)
        .show(ui, |ui| {
            scroll("routine rows").show(ui, |ui| {
                changed |= command_rows(ui, list, event_ui, (&lists, &registry, room), &[], 0);
            });
        });
    egui::Frame::NONE.inner_margin(6).show(ui, |ui| {
        scroll("routine row arguments").show(ui, |ui| {
            let places = (&lists, &registry, &editors, room);
            changed |= command_arguments(ui, list, event_ui, places);
        });
    });
    if changed && let Err(text) = edit.save() {
        *error = Some(text);
    }
    if close {
        *routine = None;
    }
    true
}

#[cfg(test)]
mod tests {
    use bevy_ecs::schedule::IntoScheduleConfigs;
    use maki_core::{App, ConfigFile};
    use maki_events::{EventsModule, RoutineCommand};
    use maki_platform::PlatformModule;
    use maki_scene::SceneModule;

    use super::*;

    #[test]
    fn a_new_routine_is_a_file_that_the_panel_edits_and_a_row_calls() {
        let directory =
            std::env::temp_dir().join(format!("maki-new-routine-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut app = App::new(ConfigFile::default()).unwrap();
        app.add_module(PlatformModule)
            .add_module(SceneModule)
            .add_module(EventsModule);
        app.update();
        let registry = app.world().resource::<CommandRegistry>().clone();

        let path = new_routine(&directory, "count").unwrap();
        assert_eq!(path, directory.join("routines/count.routine.ron"));
        assert!(new_routine(&directory, "count").is_err());
        assert!(new_routine(&directory, "../out").is_err());
        // The panel edits the routine: it is written at once.
        let mut edit = RoutineEdit::open(&path, &registry).unwrap();
        assert_eq!(edit.routine, RoutineDef::default());
        edit.routine.arguments.push("amount".to_owned());
        let row = registry
            .parse_command(r#"AddVariable("amount", 1)"#)
            .unwrap();
        edit.routine.commands.push(row);
        edit.save().unwrap();
        assert_eq!(RoutineEdit::open(&path, &registry).unwrap(), edit);
        // The file picker of a row offers it.
        let files =
            crate::widgets::files(&directory, &crate::widgets::ROUTINES, &Default::default());
        assert_eq!(files, [PathBuf::from("routines/count.routine.ron")]);

        // A row of the event panel calls it: the room check finds the file and the count.
        let call = registry
            .command(RoutineCommand::CallRoutine(
                "routines/count.routine.ron".into(),
                maki_events::Arguments(vec![4]),
            ))
            .unwrap();
        let events = format!(
            r#"(name: "a", pages: [(trigger: Interact, commands: [{}])])"#,
            maki_scene::ron_options().to_string(&call).unwrap()
        );
        let room = maki_scene::room_text(4.0, 4.0, 3.0, &[&maki_events::short_events(&events)]);
        let validators = app.world().resource::<maki_scene::RoomValidators>();
        maki_scene::check_room(&directory.join("a.ron"), &room, validators).unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }

    /// The texts that the event panel showed, with a routine open.
    #[derive(bevy_ecs::resource::Resource, Default)]
    struct Shown(Vec<String>);

    /// Draws the panel in a headless egui frame when a routine is open.
    fn draw(world: &mut World) {
        if world.resource::<Editor>().routine.is_none() {
            return;
        }
        let mut output = egui::Context::default().run_ui(egui::RawInput::default(), |ui| {
            world.resource_scope(|world, mut editor: bevy_ecs::world::Mut<Editor>| {
                assert!(routine_panel(ui, world, &mut editor));
            });
        });
        output.textures_delta.clear();
        let texts = output.shapes.iter().filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        });
        world.resource_mut::<Shown>().0 = texts.collect();
    }

    /// The routine opens in the event panel as a list of rows.
    #[test]
    fn a_routine_shows_in_the_event_panel_as_rows() {
        let (mut app, directory) = crate::tests::editor_app("routine-panel");
        app.init_resource::<Shown>()
            .add_systems(maki_core::Update, draw.ambiguous_with_all());
        app.run(|mut app| {
            crate::tests::frames(&mut app, 3);
            let path = new_routine(&directory, "count").unwrap();
            let text = r#"(arguments: ["amount"], commands: [AddVariable("amount", 1)])"#;
            std::fs::write(&path, text).unwrap();
            app.resource_mut::<Editor>().open_routine(&path);
            crate::tests::frames(&mut app, 1);
            let texts = &app.world().resource::<Shown>().0;
            assert!(texts.iter().any(|text| text == "amount"), "{texts:?}");
            assert!(
                texts
                    .iter()
                    .any(|text| text.contains("Variable amount: add 1")),
                "{texts:?}"
            );
            // Nothing changed: the file is as it was.
            assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        });
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_new_argument_name_is_not_used_twice() {
        let list = |names: &[&str]| {
            names
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(new_argument_name(&[]), "argument1");
        assert_eq!(new_argument_name(&list(&["argument2"])), "argument1");
        assert_eq!(
            new_argument_name(&list(&["argument1", "argument2", "argument4"])),
            "argument3"
        );
    }
}
