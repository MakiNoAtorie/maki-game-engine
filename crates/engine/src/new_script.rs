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
    ("Chaser", include_str!("../templates/chaser.rs")),
    ("Talker", include_str!("../templates/talker.rs")),
    (
        "State machine",
        include_str!("../templates/state_machine.rs"),
    ),
    (
        "Flickering light",
        include_str!("../templates/flickering_light.rs"),
    ),
    (
        "Cutscene trigger",
        include_str!("../templates/cutscene_trigger.rs"),
    ),
    (
        "Event command",
        include_str!("../templates/event_command.rs"),
    ),
    (
        "Engine module",
        include_str!("../templates/engine_module.rs"),
    ),
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
    // A letter with a mark would be dropped silently, so it is refused.
    if let Some(c) = input
        .trim()
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || " -_".contains(*c)))
    {
        return Err(format!(
            "`{c}` is not allowed. Use only a-z, 0-9, space, - or _."
        ));
    }
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
        "as",
        "async",
        "await",
        "box",
        "break",
        "const",
        "continue",
        "crate",
        "dyn",
        "else",
        "enum",
        "extern",
        "false",
        "fn",
        "for",
        "if",
        "impl",
        "in",
        "let",
        "loop",
        "match",
        "mod",
        "move",
        "mut",
        "pub",
        "ref",
        "return",
        "self",
        "static",
        "struct",
        "super",
        "trait",
        "true",
        "type",
        "unsafe",
        "use",
        "where",
        "while",
        "yield",
        "lib",
        "main",
        "abstract",
        "become",
        "do",
        "final",
        "macro",
        "override",
        "priv",
        "try",
        "typeof",
        "unsized",
        "virtual",
        "std",
        "core",
        "alloc",
        "serde",
        "glam",
        "bevy_ecs",
        "maki_engine",
    ];
    if KEYWORDS.contains(&snake.as_str()) {
        return Err(format!(
            "`{snake}` is a word of Rust, or a file of the crate."
        ));
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
/// no `add_modules`.
fn register(lib: &str, snake: &str) -> Option<String> {
    let lines: Vec<&str> = lib.lines().collect();
    // The code of a line without its visibility and its end comment.
    let code = |line: &str| {
        let line = line.split("//").next().unwrap_or("").trim_end();
        let line = line.strip_prefix("pub").map_or(line, |rest| {
            rest.strip_prefix("(crate)")
                .or(rest.strip_prefix("(super)"))
                .unwrap_or(rest)
                .trim_start()
        });
        line.to_owned()
    };
    // After the last `mod x;`, else the last `use`, else the inner lines that must stay first.
    let last_mod = lines.iter().rposition(|l| {
        let code = code(l);
        code.starts_with("mod ") && code.ends_with(';')
    });
    let last_use = lines
        .iter()
        .rposition(|l| code(l).starts_with("use "))
        .map(|start| {
            let end = lines[start..].iter().position(|l| code(l).ends_with(';'));
            start + end.unwrap_or(0)
        });
    let inner = |l: &&&str| l.starts_with("//!") || l.starts_with("#![") || l.trim().is_empty();
    let head = lines.iter().take_while(inner).count();
    let top = lines[..head].iter().rposition(|l| !l.trim().is_empty());
    let at = last_mod.or(last_use).or(top);
    let entry = lines.iter().position(|line| line.trim() == ENTRY)?;
    let mod_line = format!("mod {snake};");
    let add_line = format!("    {snake}::add(app);");
    let mut out: Vec<&str> = Vec::with_capacity(lines.len() + 2);
    if at.is_none() {
        out.push(&mod_line);
    }
    for (index, line) in lines.iter().enumerate() {
        out.push(line);
        if Some(index) == at {
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
        return Err(format!(
            "src/{snake}.rs exists already. Choose another name."
        ));
    }
    std::fs::write(&file, fill(template, &snake, &pascal))
        .map_err(|error| format!("{} not written: {error}", file.display()))?;
    let lib_path = src.join("lib.rs");
    // Git on Windows can give the file CRLF line ends: they stay.
    let lib = std::fs::read_to_string(&lib_path).unwrap_or_default();
    let crlf = lib.contains("\r\n");
    let registered = register(&lib.replace("\r\n", "\n"), &snake)
        .map(|text| {
            if crlf {
                text.replace('\n', "\r\n")
            } else {
                text
            }
        })
        .and_then(|text| std::fs::write(&lib_path, text).ok());
    let to_add = match registered {
        Some(()) => String::new(),
        None => format!(
            "add `mod {snake};` at the top of {} and `{snake}::add(app);` in its `add_modules`",
            lib_path.display()
        ),
    };
    Ok(Created { file, to_add })
}

/// The panel and the command of the editor.
#[cfg(feature = "dynamic")]
pub(crate) use panel::NewScriptModule;

#[cfg(feature = "dynamic")]
mod panel {
    use std::path::{Path, PathBuf};

    use bevy_ecs::resource::Resource;
    use bevy_ecs::world::World;
    use maki_core::{App, FieldTypes, Module};
    use maki_ui::egui;
    use maki_ui::theme::icon;
    use maki_ui::{
        NoticeLevel, Panels, THEME, ToolCommand, ToolPanel, ToolPlace, add_command, add_tool_panel,
        notify,
    };

    use super::{Created, create, names, templates};
    use crate::reload::open_after_build;

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
        /// The name field takes the keyboard in the next frame.
        focus: bool,
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
                // A template file of the project can be gone since the last time.
                form.template = form.template.min(form.templates.len().saturating_sub(1));
                form.result = None;
                form.focus = true;
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

    /// A name, a template, and "Create": the file goes into the script crate. It opens
    /// in the code editor when the build passes; if not, the panel "Build" shows.
    fn new_script_panel(ui: &mut egui::Ui, world: &mut World) {
        // A new type under the name of another would take its place in each room.
        let taken: Vec<&str> = world
            .resource::<FieldTypes>()
            .iter()
            .map(|(name, _)| name)
            .collect();
        let mut form = world.resource_mut::<Form>();
        let form = &mut *form;
        if form.templates.is_empty() {
            form.templates = templates(&form.project);
        }
        let before = (form.name.clone(), form.template);
        let mut entered = false;
        egui::Grid::new("new_script_form")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Name");
                let field = egui::TextEdit::singleline(&mut form.name)
                    .hint_text("e.g. door bell")
                    .desired_width(240.0);
                let field = ui.add(field);
                if std::mem::take(&mut form.focus) {
                    field.request_focus();
                }
                entered =
                    field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                ui.end_row();

                ui.label("Template");
                let chosen = form
                    .templates
                    .get(form.template)
                    .map_or("", |t| t.0.as_str());
                egui::ComboBox::from_id_salt("new_script_template")
                    .selected_text(chosen)
                    .width(240.0)
                    // The default height hides the tenth template behind a scroll.
                    .height(f32::INFINITY)
                    .show_ui(ui, |ui| {
                        for (index, (name, text)) in form.templates.iter().enumerate() {
                            ui.selectable_value(&mut form.template, index, name)
                                .on_hover_text(about(text));
                        }
                    });
                ui.end_row();
            });
        let template = form.templates.get(form.template);
        if let Some((_, text)) = template {
            ui.add(egui::Label::new(egui::RichText::new(about(text)).weak()).wrap());
        }
        // Only a template that adds a script makes a component.
        let component = template.is_some_and(|(_, text)| text.contains("add_script::<ScriptName>"));
        let named = names(&form.name).and_then(|(snake, pascal)| {
            match component && taken.contains(&snake.as_str()) {
                true => Err(format!("A component `{snake}` exists already.")),
                false => Ok((snake, pascal)),
            }
        });
        let reason = match (&named, template) {
            (_, None) => Some("Choose a template.".to_owned()),
            (Err(_), _) if form.name.trim().is_empty() => Some("Type a name.".to_owned()),
            (Err(why), _) => Some(why.clone()),
            (Ok(_), Some(_)) => None,
        };
        match &named {
            Ok((snake, pascal)) if component => ui.weak(format!(
                "Makes src/{snake}.rs with the component `{snake}` (type `{pascal}`)."
            )),
            Ok((snake, pascal)) => {
                ui.weak(format!("Makes src/{snake}.rs with the type `{pascal}`."))
            }
            Err(why) if !form.name.trim().is_empty() => ui.colored_label(THEME.warning, why),
            Err(_) => ui.weak("A name of letters and numbers."),
        };
        let mut done = None;
        let create = ui
            .add_enabled(reason.is_none(), egui::Button::new("Create"))
            .on_disabled_hover_text(reason.clone().unwrap_or_default());
        if (create.clicked() || (entered && reason.is_none()))
            && let Some((_, text)) = template
        {
            let crate_dir = form.project.join(SCRIPT_CRATE);
            done = Some(create_file(&crate_dir, &form.name, text, component));
        }
        // A result belongs to the name and the template that made it.
        if (form.name.clone(), form.template) != before {
            form.result = None;
        }
        match &form.result {
            Some((text, true)) => {
                ui.horizontal(|ui| {
                    ui.colored_label(THEME.error, text);
                    if ui.small_button("Copy").clicked() {
                        ui.ctx().copy_text(text.clone());
                    }
                });
            }
            Some((text, false)) => {
                ui.label(text);
            }
            None => {}
        }
        match done {
            Some(Ok((created, text))) => {
                tracing::info!("{text}");
                form.result = Some((text, !created.to_add.is_empty()));
                form.name.clear();
                let name = created.file.file_name().unwrap_or_default().display();
                let text = format!("Created {name}. Building the scripts...");
                notify(world, NoticeLevel::Info, text);
                open_after_build(world, created.file);
            }
            Some(Err(text)) => {
                form.result = Some((text.clone(), true));
                notify(world, NoticeLevel::Error, text);
            }
            None => {}
        }
    }

    /// [`create`], and the text of its result for the panel.
    fn create_file(
        crate_dir: &Path,
        name: &str,
        template: &str,
        component: bool,
    ) -> Result<(Created, String), String> {
        let created = create(crate_dir, name, template)?;
        let file = created
            .file
            .strip_prefix(crate_dir)
            .unwrap_or(&created.file);
        let file = file.display();
        let text = match (created.to_add.is_empty(), component) {
            (false, _) => format!("{file} written, but {}.", created.to_add),
            (true, true) => {
                let (snake, _) = names(name)?;
                format!(
                    "{file} written and registered. After the build, add the component \
                     `{snake}` to an object in the inspector."
                )
            }
            (true, false) => format!("{file} written and registered."),
        };
        Ok((created, text))
    }

    /// The doc lines at the top of a template, without the name in front.
    fn about(template: &str) -> String {
        let lines = template.lines().map_while(|line| line.strip_prefix("//!"));
        let text: Vec<&str> = lines
            .map(str::trim)
            .take_while(|line| !line.is_empty())
            .collect();
        let text = text.join(" ");
        text.strip_prefix("ScriptName: ")
            .unwrap_or(&text)
            .to_owned()
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
        assert!(names("Türschloss").unwrap_err().contains('ü'));
        assert!(names("bevy ecs").is_err() && names("serde").is_err() && names("try").is_err());
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
    fn the_mod_line_is_top_level_never_in_a_tests_block() {
        let entry = "pub fn add_modules(app: &mut App) {\n}\n";
        let lib =
            format!("mod beacon;\n\n{entry}\n#[cfg(test)]\nmod tests {{\n    use super::*;\n}}\n");
        let out = register(&lib, "gate").unwrap();
        assert!(out.starts_with("mod beacon;\nmod gate;\n"), "{out}");
        assert_eq!(out.matches("mod gate;").count(), 1);
        // No `mod` line: after the `use` lines, else at the top (after the `//!` lines).
        let out = register(
            &format!("//! Doc.\n\nuse a::B;\nuse c::D;\n\n{entry}"),
            "gate",
        )
        .unwrap();
        assert!(out.contains("use c::D;\nmod gate;\n"), "{out}");
        let out = register(&format!("//! Doc.\n\n{entry}"), "gate").unwrap();
        assert!(out.starts_with("//! Doc.\nmod gate;\n"), "{out}");
    }

    #[test]
    fn the_mod_line_reads_the_forms_of_mod_use_and_the_inner_lines() {
        let entry = "pub fn add_modules(app: &mut App) {\n}\n";
        let register = |head: &str| register(&format!("{head}\n{entry}"), "gate").unwrap();
        // A visibility and a comment after the `;`: still a top-level `mod` line.
        let out = register("pub mod a;\npub(crate) mod b;\nmod c; // the c\n");
        assert!(out.contains("mod c; // the c\nmod gate;\n"), "{out}");
        // A `use` over many lines: the `mod` line goes after its end.
        let out = register("use a::{\n    B,\n    C,\n};\n");
        assert!(out.contains("};\nmod gate;\n"), "{out}");
        // Inner lines with blank lines among them: after the last of them.
        let out = register("//! Doc.\n\n#![allow(dead_code)]\n\n");
        assert!(
            out.starts_with("//! Doc.\n\n#![allow(dead_code)]\nmod gate;\n"),
            "{out}"
        );
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
        assert_eq!(
            list.last().unwrap(),
            &("Door bell".to_owned(), "// bell".to_owned())
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
