//! "New script": a file of the script crate of a project, made from a template.
//!
//! A template is a `.rs` file with `ScriptName` and `script_name` in the places of the
//! name. The templates of the engine are in `crates/engine/templates/`; a folder
//! `templates/` of the project adds its own, and a file of the same name takes the
//! place of the engine's. Each template has `pub fn add(app: &mut App)`: the new
//! file is registered with a line in `add_modules` of `src/lib.rs`.

use std::path::{Path, PathBuf};

/// The templates of the engine: the name in the list, and the text.
pub const TEMPLATES: &[(&str, &str)] = &[
    ("Empty", include_str!("../templates/empty.rs")),
    ("Interactable", include_str!("../templates/interactable.rs")),
    ("Trigger area", include_str!("../templates/trigger_area.rs")),
    ("Movement", include_str!("../templates/movement.rs")),
    ("Patrol", include_str!("../templates/patrol.rs")),
    ("Talker", include_str!("../templates/talker.rs")),
    ("State machine", include_str!("../templates/state_machine.rs")),
    ("Flickering light", include_str!("../templates/flickering_light.rs")),
    ("Event command", include_str!("../templates/event_command.rs")),
    ("Engine module", include_str!("../templates/engine_module.rs")),
];

/// The function of `src/lib.rs` that the new line goes into.
const ENTRY: &str = "pub fn add_modules(app: &mut App) {";

/// The name of a template from its file: `trigger_area.rs` is "Trigger area".
fn title(file: &Path) -> Option<String> {
    let stem = file.file_stem()?.to_str()?.replace('_', " ");
    let mut chars = stem.chars();
    let first = chars.next()?.to_uppercase();
    Some(first.chain(chars).collect())
}

/// The templates of the engine and of the folder `templates/` of the project, in the
/// order of the list: the engine's, then the others by name.
pub fn templates(project: &Path) -> Vec<(String, String)> {
    let mut list: Vec<(String, String)> = TEMPLATES
        .iter()
        .map(|(name, text)| (name.to_string(), text.to_string()))
        .collect();
    let Ok(files) = std::fs::read_dir(project.join("templates")) else {
        return list;
    };
    let mut files: Vec<PathBuf> = files.filter_map(|file| Some(file.ok()?.path())).collect();
    files.sort();
    for file in files {
        let rust = file.extension().is_some_and(|extension| extension == "rs");
        let (Some(name), true) = (title(&file), rust) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&file) else {
            tracing::warn!(file = %file.display(), "template not read");
            continue;
        };
        match list.iter_mut().find(|(old, _)| *old == name) {
            Some(old) => old.1 = text,
            None => list.push((name, text)),
        }
    }
    list
}

/// The name of a script as a module (`door_bell`) and as a type (`DoorBell`), from
/// what the user wrote: "door bell", "DoorBell", "door-bell". `Err`: why it cannot
/// be a name.
pub fn names(input: &str) -> Result<(String, String), String> {
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut last_lower = false;
    for c in input.trim().chars() {
        if !c.is_ascii_alphanumeric() {
            words.extend((!word.is_empty()).then(|| std::mem::take(&mut word)));
            last_lower = false;
            continue;
        }
        // A capital after a small letter starts a word: "DoorBell".
        if c.is_ascii_uppercase() && last_lower {
            words.push(std::mem::take(&mut word));
        }
        last_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
        word.push(c.to_ascii_lowercase());
    }
    words.extend((!word.is_empty()).then_some(word));
    let snake = words.join("_");
    if !snake.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return Err("A name starts with a letter, and has letters and numbers.".to_owned());
    }
    // ponytail: the keywords that a word of a name can be; `r#` names are not made.
    const KEYWORDS: &[&str] = &[
        "as", "async", "await", "box", "break", "const", "continue", "crate", "dyn", "else",
        "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match",
        "mod", "move", "mut", "pub", "ref", "return", "self", "static", "struct", "super",
        "trait", "true", "type", "unsafe", "use", "where", "while", "yield", "lib", "main",
    ];
    if KEYWORDS.contains(&snake.as_str()) {
        return Err(format!("`{snake}` is a word of Rust, or a file of the crate."));
    }
    let pascal = words
        .iter()
        .map(|word| {
            let mut chars = word.chars();
            let first = chars.next().map(|c| c.to_ascii_uppercase());
            first.into_iter().chain(chars).collect::<String>()
        })
        .collect();
    Ok((snake, pascal))
}

/// The text of a template with a name in it.
pub fn fill(template: &str, snake: &str, pascal: &str) -> String {
    template
        .replace("ScriptName", pascal)
        .replace("script_name", snake)
}

/// `src/lib.rs` with the module and its line in `add_modules`. `None`: the text has
/// no top-level `mod` line or no `add_modules`.
fn register(lib: &str, snake: &str) -> Option<String> {
    let lines: Vec<&str> = lib.lines().collect();
    let last_mod = lines.iter().rposition(|line| line.starts_with("mod "))?;
    let entry = lines.iter().position(|line| line.trim() == ENTRY)?;
    let mod_line = format!("mod {snake};");
    let add_line = format!("    {snake}::add(app);");
    let mut out: Vec<&str> = Vec::with_capacity(lines.len() + 2);
    for (index, line) in lines.iter().enumerate() {
        out.push(line);
        if index == last_mod {
            out.push(&mod_line);
        }
        if index == entry {
            out.push(&add_line);
        }
    }
    let mut text = out.join("\n");
    text.push('\n');
    Some(text)
}

/// What "Create" did.
#[derive(Debug, PartialEq)]
pub struct Created {
    pub file: PathBuf,
    /// Empty: the file is registered. Else what the user must add to `src/lib.rs`.
    pub to_add: String,
}

/// Writes the file of a new script into the script crate (`crate_dir`), and
/// registers it in `src/lib.rs`. `Err`: why no file was written.
pub fn create(crate_dir: &Path, input: &str, template: &str) -> Result<Created, String> {
    let (snake, pascal) = names(input)?;
    let src = crate_dir.join("src");
    let file = src.join(format!("{snake}.rs"));
    if file.exists() {
        return Err(format!("{} is there already.", file.display()));
    }
    std::fs::write(&file, fill(template, &snake, &pascal))
        .map_err(|error| format!("{} not written: {error}", file.display()))?;
    let lib_path = src.join("lib.rs");
    // Git on Windows can give the file CRLF line ends: they stay.
    let lib = std::fs::read_to_string(&lib_path).unwrap_or_default();
    let crlf = lib.contains("\r\n");
    let registered = register(&lib.replace("\r\n", "\n"), &snake)
        .map(|text| if crlf { text.replace('\n', "\r\n") } else { text })
        .and_then(|text| std::fs::write(&lib_path, text).ok());
    let to_add = match registered {
        Some(()) => String::new(),
        None => format!("add `mod {snake};` and `{snake}::add(app);` to {}", lib_path.display()),
    };
    Ok(Created { file, to_add })
}

/// The panel and the command of the editor.
#[cfg(feature = "dynamic")]
pub(crate) use panel::NewScriptModule;

#[cfg(feature = "dynamic")]
mod panel {
    use std::path::PathBuf;

    use bevy_ecs::resource::Resource;
    use bevy_ecs::world::World;
    use maki_core::{App, Module};
    use maki_ui::egui;
    use maki_ui::theme::icon;
    use maki_ui::{
        NoticeLevel, Panels, THEME, ToolCommand, ToolPanel, ToolPlace, add_command,
        add_tool_panel, notify,
    };

    use super::{create, names, templates};
    use crate::reload::open_in_code_editor;

    const PANEL: &str = "new_script";
    /// The folder of the script crate in a project folder.
    const SCRIPT_CRATE: &str = "scripts";

    /// Adds the panel "New script" and its command to the editor.
    pub(crate) struct NewScriptModule {
        /// The project folder.
        pub project: PathBuf,
    }

    /// What the panel holds between two frames.
    #[derive(Resource, Default)]
    struct Form {
        project: PathBuf,
        name: String,
        template: usize,
        /// The templates, read when the command opens the panel.
        templates: Vec<(String, String)>,
        /// What the last "Create" did, and if it was an error.
        result: Option<(String, bool)>,
    }

    impl Module for NewScriptModule {
        fn build(&self, app: &mut App) {
            app.insert_resource(Form {
                project: self.project.clone(),
                ..Form::default()
            });
            let panel = ToolPanel {
                id: PANEL,
                title: "New script",
                icon: icon::BUILD,
                place: ToolPlace::Bottom,
                open: false,
            };
            add_tool_panel(app, panel, new_script_panel);
            let open = |world: &mut World| {
                let mut form = world.resource_mut::<Form>();
                form.templates = templates(&form.project);
                form.result = None;
                world.resource_mut::<Panels>().show(PANEL);
            };
            add_command(
                app,
                ToolCommand {
                    menu: "File",
                    ..ToolCommand::new("project.new_script", "New script", open)
                },
            );
        }
    }

    /// A name, a template, and "Create": the file goes into the script crate and
    /// opens in the code editor. The watch of the build sees it.
    fn new_script_panel(ui: &mut egui::Ui, world: &mut World) {
        let mut form = world.resource_mut::<Form>();
        let form = &mut *form;
        if form.templates.is_empty() {
            form.templates = templates(&form.project);
        }
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.add(egui::TextEdit::singleline(&mut form.name).hint_text("door bell"));
            let chosen = form.templates.get(form.template).map_or("", |t| t.0.as_str());
            egui::ComboBox::from_id_salt("new_script_template")
                .selected_text(chosen)
                .show_ui(ui, |ui| {
                    for (index, (name, _)) in form.templates.iter().enumerate() {
                        ui.selectable_value(&mut form.template, index, name);
                    }
                });
        });
        let named = names(&form.name);
        match &named {
            Ok((snake, pascal)) => ui.weak(format!("src/{snake}.rs, the component `{snake}`, the type `{pascal}`")),
            Err(why) if !form.name.trim().is_empty() => ui.colored_label(THEME.warning, why),
            Err(_) => ui.weak("A name of letters and numbers."),
        };
        let mut done = None;
        if ui.add_enabled(named.is_ok(), egui::Button::new("Create")).clicked()
            && let Some((_, text)) = form.templates.get(form.template)
        {
            let crate_dir = form.project.join(SCRIPT_CRATE);
            done = Some(create(&crate_dir, &form.name, text));
        }
        match &form.result {
            Some((text, true)) => ui.colored_label(THEME.error, text),
            Some((text, false)) => ui.label(text),
            None => ui.weak(""),
        };
        match done {
            Some(Ok(created)) => {
                let text = match created.to_add.is_empty() {
                    true => format!("{} written and registered.", created.file.display()),
                    false => format!("{} written: {}.", created.file.display(), created.to_add),
                };
                tracing::info!("{text}");
                form.result = Some((text, !created.to_add.is_empty()));
                form.name.clear();
                open_in_code_editor(world, &created.file, 1);
            }
            Some(Err(text)) => {
                form.result = Some((text.clone(), true));
                notify(world, NoticeLevel::Error, text);
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_gives_a_module_and_a_type() {
        let pair = |snake: &str, pascal: &str| Ok((snake.to_owned(), pascal.to_owned()));
        assert_eq!(names("door bell"), pair("door_bell", "DoorBell"));
        assert_eq!(names("DoorBell"), pair("door_bell", "DoorBell"));
        assert_eq!(names(" door-bell2 "), pair("door_bell2", "DoorBell2"));
        assert_eq!(names("lamp"), pair("lamp", "Lamp"));
        assert!(names("").is_err());
        assert!(names("2 doors").is_err());
        assert!(names("match").is_err());
        assert!(names("lib").is_err());
    }

    #[test]
    fn a_new_script_is_written_and_registered_and_not_twice() {
        let dir = std::env::temp_dir().join(format!("maki-new-script-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let lib = "//! Scripts.\r\n\r\nmod beacon;\r\nmod shake;\r\n\r\nuse maki_engine::core::App;\r\n\r\npub fn add_modules(app: &mut App) {\r\n    beacon::add(app);\r\n}\r\n";
        std::fs::write(dir.join("src/lib.rs"), lib).unwrap();

        let created = create(&dir, "Door bell", TEMPLATES[0].1).unwrap();
        assert_eq!(created.file, dir.join("src/door_bell.rs"));
        assert_eq!(created.to_add, "");
        let text = std::fs::read_to_string(&created.file).unwrap();
        assert!(text.contains("pub struct DoorBell {"));
        assert!(text.contains(r#"add_script::<DoorBell>(app, "door_bell");"#));
        assert!(!text.contains("ScriptName") && !text.contains("script_name"));
        assert_eq!(
            std::fs::read_to_string(dir.join("src/lib.rs")).unwrap(),
            "//! Scripts.\r\n\r\nmod beacon;\r\nmod shake;\r\nmod door_bell;\r\n\r\nuse maki_engine::core::App;\r\n\r\npub fn add_modules(app: &mut App) {\r\n    door_bell::add(app);\r\n    beacon::add(app);\r\n}\r\n"
        );
        // The same name again: no file is written over.
        assert!(create(&dir, "door_bell", TEMPLATES[0].1).is_err());

        // A `lib.rs` of another form: the file, and what to add by hand.
        std::fs::write(dir.join("src/lib.rs"), "pub fn entry() {}\n").unwrap();
        let created = create(&dir, "gate", TEMPLATES[0].1).unwrap();
        assert!(created.to_add.contains("mod gate;"), "{}", created.to_add);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_template_of_the_project_is_added_or_takes_the_place_of_the_engines() {
        let dir = std::env::temp_dir().join(format!("maki-templates-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("templates")).unwrap();
        std::fs::write(dir.join("templates/empty.rs"), "// mine").unwrap();
        std::fs::write(dir.join("templates/door_bell.rs"), "// bell").unwrap();
        std::fs::write(dir.join("templates/notes.txt"), "no").unwrap();
        let list = templates(&dir);
        assert_eq!(list.len(), TEMPLATES.len() + 1);
        assert_eq!(list[0], ("Empty".to_owned(), "// mine".to_owned()));
        assert_eq!(list.last().unwrap(), &("Door bell".to_owned(), "// bell".to_owned()));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
