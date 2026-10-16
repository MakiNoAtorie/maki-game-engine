//! A new build of the script DLL while the editor runs.
//!
//! - Watch: some times in a second, the editor looks at the file times of the Rust
//!   files and the `Cargo.toml` files of the project folder.
//! - Build: a change starts the build command on another thread; the status row
//!   shows the job. The panel "Build" has the result: one row for each error and
//!   warning, and a click opens the file at the line in the code editor.
//! - Load: a build that wrote a new DLL asks for another app in the same window
//!   (`maki_platform::NextApp`), with the modules of the new copy. The open document
//!   goes to it as text (`maki_editor::Session`). While a game runs, the load waits.
//!
//! An app cannot give back the systems and the closures of its modules, and the old
//! copy of the DLL is never unloaded: so the new code gets a new app, and the old
//! app is dropped.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant, SystemTime};

use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::world::{Mut, World};
use maki_core::{App, Module, Update};
use maki_editor::{Document, Editor, EditorSettings};
use maki_events::CommandRegistry;
use maki_platform::NextApp;
use maki_ui::egui;
use maki_ui::theme::icon;
use maki_ui::{
    Jobs, NoticeLevel, Notices, Panels, THEME, ToolCommand, ToolPanel, ToolPlace, add_command,
    add_tool_panel, notify,
};

use crate::editor_program::{BUILD, Setup, editor_app, script_dll};

/// The id of the build panel.
const PANEL: &str = "build";
/// The name of the build in the jobs part of the status row.
const JOB: &str = "build of the scripts";
/// The time between two looks at the files of the project.
const SCAN: Duration = Duration::from_millis(300);

/// One error or warning of a build.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BuildRow {
    pub error: bool,
    /// The file, as cargo names it (from the workspace folder), and the line.
    pub place: Option<(PathBuf, u32)>,
    pub text: String,
}

/// What a build gave.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct BuildReport {
    pub ok: bool,
    pub seconds: f32,
    pub rows: Vec<BuildRow>,
    /// Why cargo did not run, or what the user must do after a build that failed.
    /// Empty: nothing more than the rows.
    pub failure: String,
}

impl BuildReport {
    /// A text for a notice: the errors, each on one line.
    pub fn errors(&self) -> String {
        let rows = self.rows.iter().filter(|row| row.error);
        let mut lines: Vec<String> = rows.map(BuildRow::line).collect();
        if !self.failure.is_empty() {
            lines.insert(0, self.failure.clone());
        }
        lines.join("\n")
    }
}

impl BuildRow {
    fn line(&self) -> String {
        match &self.place {
            Some((file, line)) => format!("{}:{line}: {}", file.display(), self.text),
            None => self.text.clone(),
        }
    }
}

/// The rows of the text that `cargo build --message-format short` writes: a line
/// for each error and warning, `FILE:LINE:COLUMN: error[CODE]: TEXT`, and lines
/// with no place (`error: could not compile ...`).
pub(crate) fn rows(text: &str) -> Vec<BuildRow> {
    let row = |line: &str| -> Option<BuildRow> {
        let (at, error) = match (line.find("error"), line.find("warning")) {
            (Some(error), Some(warning)) => (error.min(warning), error < warning),
            (Some(error), None) => (error, true),
            (None, Some(warning)) => (warning, false),
            (None, None) => return None,
        };
        // The count of the warnings of a crate is not a warning.
        if line.contains("generated") && !error {
            return None;
        }
        // `FILE:LINE:COLUMN: ` before the word. A file can have a `:` (a drive).
        let place = line[..at].strip_suffix(": ").and_then(|place| {
            let mut parts = place.rsplitn(3, ':');
            let (_column, line, file) = (parts.next()?, parts.next()?, parts.next()?);
            Some((PathBuf::from(file), line.parse().ok()?))
        });
        // A word in the middle of another text is not a row.
        (place.is_some() || at == 0).then(|| BuildRow {
            error,
            place,
            text: line[at..].to_owned(),
        })
    };
    text.lines().filter_map(row).collect()
}

/// Builds the editor and the script DLL of the project. It waits for cargo.
pub(crate) fn build(setup: &Setup) -> BuildReport {
    let start = Instant::now();
    // The program that `cargo run` and `cargo test` name, else that of the path.
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .args(BUILD)
        .args(["-p", &setup.project.scripts, "--message-format", "short"]);
    // In the project folder: cargo finds the workspace of the script crate.
    let output = command.current_dir(&setup.project.dir).output();
    let seconds = start.elapsed().as_secs_f32();
    match output {
        Ok(output) => {
            let text = String::from_utf8_lossy(&output.stderr);
            let ok = output.status.success();
            let mut failure = String::new();
            if !ok {
                tracing::error!("the build of the script crate failed:\n{text}");
                // Cargo cannot write a file that this program has loaded.
                if text.contains(crate::loader::ENGINE) {
                    failure = "The engine DLL is in use: a change of an engine crate needs a \
                               new start of the editor."
                        .to_owned();
                }
            }
            BuildReport {
                ok,
                seconds,
                rows: rows(&text),
                failure,
            }
        }
        Err(error) => BuildReport {
            ok: false,
            seconds,
            rows: Vec::new(),
            failure: format!("cargo did not run: {error}"),
        },
    }
}

/// The Rust files and the `Cargo.toml` files of a project folder, with their file
/// times. Not the assets, not a build directory, not a hidden directory.
pub(crate) fn scan(dir: &Path) -> Vec<(PathBuf, SystemTime)> {
    let mut found = Vec::new();
    let mut dirs = vec![dir.to_owned()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let (path, name) = (entry.path(), entry.file_name());
            let name = name.to_string_lossy();
            if path.is_dir() {
                if !(name == "assets" || name == "target" || name.starts_with('.')) {
                    dirs.push(path);
                }
            } else if name == "Cargo.toml" || name.ends_with(".rs") {
                let written = entry.metadata().and_then(|data| data.modified());
                found.push((path, written.unwrap_or(SystemTime::UNIX_EPOCH)));
            }
        }
    }
    found.sort();
    found
}

fn written(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|data| data.modified())
        .ok()
}

/// The state of the watch, of the build that runs, and of the load that waits.
#[derive(Resource)]
struct ProjectBuild {
    setup: Setup,
    /// The files of the last look, and the time of the next one.
    files: Vec<(PathBuf, SystemTime)>,
    next_scan: Instant,
    /// A build is asked for: a file changed, or the user asked.
    request: bool,
    /// The build that runs, on its thread.
    running: Option<Mutex<Receiver<BuildReport>>>,
    /// The last build that ended.
    report: BuildReport,
    /// The file time of the script DLL that this app loaded.
    loaded: Option<SystemTime>,
    /// A new DLL waits for its load: a game runs.
    load: bool,
    /// A new script: its file opens in the code editor when a build passes.
    open_when_ok: Option<PathBuf>,
}

/// Asks for a build, and opens `file` in the code editor when it passes. A failed
/// build opens the panel "Build" instead.
pub(crate) fn open_after_build(world: &mut World, file: PathBuf) {
    let mut build = world.resource_mut::<ProjectBuild>();
    build.open_when_ok = Some(file);
    build.request = true;
}

/// The watch, the build, and the load of the script crate of the project, and the
/// panel "Build". Needs `maki_editor::EditorModule`.
pub(crate) struct ProjectModule {
    pub setup: Setup,
    /// The build that this app was made after.
    pub report: BuildReport,
}

impl Module for ProjectModule {
    fn build(&self, app: &mut App) {
        let dll = script_dll(&self.setup.project).ok();
        app.insert_resource(ProjectBuild {
            files: scan(&self.setup.project.dir),
            next_scan: Instant::now() + SCAN,
            setup: self.setup.clone(),
            request: false,
            running: None,
            report: self.report.clone(),
            loaded: dll.as_deref().and_then(written),
            load: false,
            open_when_ok: None,
        })
        .init_resource::<Jobs>()
        // It has the full world: the editor, the panels, the request for an app.
        .add_systems(Update, watch_build_load.ambiguous_with_all());

        let panel = ToolPanel {
            id: PANEL,
            title: "Build",
            icon: icon::BUILD,
            place: ToolPlace::Bottom,
            open: false,
        };
        add_tool_panel(app, panel, build_panel);
        let build = |world: &mut World| world.resource_mut::<ProjectBuild>().request = true;
        add_command(
            app,
            ToolCommand {
                menu: "File",
                ..ToolCommand::new("project.build", "Build the scripts", build)
            },
        );
        let name = "Open the project in the code editor";
        add_command(
            app,
            ToolCommand {
                menu: "File",
                ..ToolCommand::new("project.open_code", name, open_project)
            },
        );
    }
}

/// Runs in `Update`: the watch, the start and the end of a build, and the load.
fn watch_build_load(world: &mut World) {
    // `open_in_code_editor` reads `ProjectBuild`: it runs after the scope.
    let open = world.resource_scope(|world, mut build: Mut<ProjectBuild>| {
        let mut open = None;
        let now = Instant::now();
        if now >= build.next_scan {
            build.next_scan = now + SCAN;
            let files = scan(&build.setup.project.dir);
            if files != build.files {
                tracing::info!("a file of the script crate changed: build");
                build.files = files;
                build.request = true;
            }
        }
        // One build at a time: a change in a build starts the next one after it.
        if build.request && build.running.is_none() {
            build.request = false;
            let (sender, receiver) = std::sync::mpsc::channel();
            let setup = build.setup.clone();
            std::thread::spawn(move || sender.send(self::build(&setup)));
            build.running = Some(Mutex::new(receiver));
            world.resource_mut::<Jobs>().0.push(JOB.to_owned());
        }
        let ended = build.running.as_ref().and_then(|receiver| {
            let receiver = receiver.lock().expect("no thread has the lock");
            match receiver.try_recv() {
                Ok(report) => Some(report),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(BuildReport {
                    failure: "the build thread ended with no result".to_owned(),
                    ..BuildReport::default()
                }),
            }
        });
        if let Some(report) = ended {
            build.running = None;
            world.resource_mut::<Jobs>().0.retain(|job| job != JOB);
            tracing::info!(
                ok = report.ok,
                seconds = report.seconds,
                "build of the scripts"
            );
            if report.ok {
                // A change of another file of the project writes no new DLL.
                let dll = script_dll(&build.setup.project).ok();
                build.load = dll.as_deref().and_then(written) != build.loaded;
                open = build.open_when_ok.take();
            } else {
                build.open_when_ok = None;
                world.resource_mut::<Panels>().show(PANEL);
                let text = "The build of the script crate failed: see the panel \"Build\"";
                notify(world, NoticeLevel::Warning, text);
            }
            build.report = report;
        }
        // The load, when no build runs or waits and no game runs: a game holds
        // entities and events of the old code.
        let editor = world.resource::<Editor>();
        let free = !editor.playing() && editor.pending().is_none();
        if build.load && free && build.running.is_none() && !build.request {
            build.load = false;
            // Also if the load is refused: the same file is not tried again.
            let dll = script_dll(&build.setup.project).ok();
            build.loaded = dll.as_deref().and_then(written);
            let (setup, report) = (build.setup.clone(), build.report.clone());
            let make = move |old: &mut App| next_app(old, &setup, report);
            world.resource_mut::<NextApp>().0 = Some(Box::new(make));
        }
        open
    });
    if let Some(file) = open {
        open_in_code_editor(world, &file, 1);
    }
}

/// The app with the new code, for `NextApp`. `None`, with a notice in the old app:
/// the new copy does not load, or the open document does not read with it.
fn next_app(old: &mut App, setup: &Setup, report: BuildReport) -> Option<App> {
    let session = maki_editor::session(old.world());
    let made = editor_app(setup, session.clone(), report).map_err(|error| format!("{error:#}"));
    let checked = made.and_then(|(new, failure)| {
        if let Some(failure) = failure {
            return Err(failure);
        }
        // A command that the new code does not have: the document would be lost.
        if let Some(session) = &session {
            let registry = new.world().resource::<CommandRegistry>();
            Document::parse(&session.path, &session.text, registry)
                .map_err(|error| format!("the open room does not read with it: {error:#}"))?;
        }
        Ok(new)
    });
    match checked {
        Ok(new) => {
            tracing::info!("the new code of the project is loaded: a new app");
            Some(new)
        }
        Err(why) => {
            let text = format!("The new code of the project was not loaded: {why}");
            tracing::error!("{text}");
            old.init_resource::<Notices>();
            old.resource_mut::<Notices>().push(NoticeLevel::Error, text);
            None
        }
    }
}

/// Runs a command line with the shell of the system, and does not wait for it.
fn shell(line: &str) -> std::io::Result<()> {
    #[cfg(windows)]
    let mut command = {
        use std::os::windows::process::CommandExt;
        let mut command = Command::new("cmd");
        // As it is: `cmd` does not know the quote rules of other programs.
        command.arg("/C").raw_arg(line);
        command
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut command = Command::new("sh");
        command.args(["-c", line]);
        command
    };
    command.spawn().map(|_| ())
}

/// "Open in editor": runs the command of the `[editor] code_editor` setting for a
/// file and a line. `file`: as cargo names it, from the workspace folder.
pub(crate) fn open_in_code_editor(world: &mut World, file: &Path, line: u32) {
    let template = world.resource::<EditorSettings>().code_editor.clone();
    let file = world.resource::<ProjectBuild>().setup.workspace.join(file);
    let command = template
        .replace("{file}", &file.display().to_string())
        .replace("{line}", &line.to_string());
    tracing::info!(command, "open in the code editor");
    if let Err(error) = shell(&command) {
        let text = format!("The code editor did not start (`{command}`): {error}");
        notify(world, NoticeLevel::Error, text);
    }
}

/// The command "Open the project in the code editor": the program of the setting,
/// with the project folder.
fn open_project(world: &mut World) {
    let template = world.resource::<EditorSettings>().code_editor.clone();
    let program = template.split_whitespace().next().unwrap_or("code");
    let dir = &world.resource::<ProjectBuild>().setup.project.dir;
    let command = format!("{program} \"{}\"", dir.display());
    if let Err(error) = shell(&command) {
        let text = format!("The code editor did not start (`{command}`): {error}");
        notify(world, NoticeLevel::Error, text);
    }
}

/// The panel "Build": the state of the last build, and its errors and warnings. A
/// click on the place of a row opens the file at the line in the code editor.
fn build_panel(ui: &mut egui::Ui, world: &mut World) {
    let build = world.resource::<ProjectBuild>();
    let (report, running, waits) = (build.report.clone(), build.running.is_some(), build.load);
    ui.horizontal(|ui| {
        if running {
            ui.spinner();
            ui.label("The build runs.");
        } else if report.ok {
            ui.label(format!("The last build: ok, {:.1} s.", report.seconds));
        } else {
            ui.colored_label(THEME.error, "The last build failed.");
        }
        if waits {
            ui.colored_label(THEME.warning, "The new code waits for the game to stop.");
        }
        if ui
            .add_enabled(!running, egui::Button::new("Build now"))
            .clicked()
        {
            world.resource_mut::<ProjectBuild>().request = true;
        }
    });
    if !report.failure.is_empty() {
        ui.colored_label(THEME.error, report.failure.as_str());
    }
    let mut open = None;
    let scroll = egui::ScrollArea::vertical().auto_shrink(false);
    scroll.show(ui, |ui| {
        for row in &report.rows {
            ui.horizontal(|ui| {
                let (mark, color) = match row.error {
                    true => (icon::ERROR, THEME.error),
                    false => (icon::WARNING, THEME.warning),
                };
                ui.colored_label(color, mark);
                if let Some((file, line)) = &row.place {
                    let link = ui.link(format!("{}:{line}", file.display()));
                    if link.on_hover_text("Open in the code editor").clicked() {
                        open = Some((file.clone(), *line));
                    }
                }
                ui.label(row.text.as_str());
            });
        }
        if report.rows.is_empty() && report.ok {
            ui.weak("No error and no warning.");
        }
    });
    if let Some((file, line)) = open {
        open_in_code_editor(world, &file, line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rows_of_a_build_have_the_file_and_the_line_of_each_error_and_warning() {
        let text = "   Compiling maki-game-scripts v0.1.0 (C:\\repo\\game\\scripts)
game\\scripts\\src\\shake.rs:190:21: error[E0308]: mismatched types: expected `u32`, found `&str`
C:\\repo\\game\\scripts\\src\\shake.rs:189:25: warning: unused variable: `x`: help: prefix it
warning: `maki-game-scripts` (lib) generated 1 warning
error: could not compile `maki-game-scripts` (lib) due to 1 previous error; 1 warning emitted
";
        let found = rows(text);
        let places: Vec<_> = found.iter().map(|row| row.place.clone()).collect();
        let shake = |file: &str, line| Some((PathBuf::from(file), line));
        assert_eq!(
            places,
            [
                shake("game\\scripts\\src\\shake.rs", 190),
                shake("C:\\repo\\game\\scripts\\src\\shake.rs", 189),
                None
            ]
        );
        assert!(found[0].error && !found[1].error && found[2].error);
        assert!(found[0].text.starts_with("error[E0308]: mismatched types"));
        assert!(found[2].text.starts_with("error: could not compile"));
        // A build with no error has no row, also with these words in a path.
        assert!(rows("   Compiling error-kit v1.0.0\n    Finished `dynamic` profile").is_empty());
    }

    #[test]
    fn a_scan_lists_the_code_files_of_a_project_and_sees_a_change() {
        let dir = std::env::temp_dir().join(format!("maki-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for sub in ["scripts/src", "assets/rooms", "target/debug", ".git"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        for file in [
            "project.toml",
            "Cargo.toml",
            "scripts/Cargo.toml",
            "scripts/src/lib.rs",
            "assets/rooms/a.ron",
            "assets/rooms/tool.rs",
            "target/debug/build.rs",
            ".git/hook.rs",
        ] {
            std::fs::write(dir.join(file), "").unwrap();
        }
        let first = scan(&dir);
        let names: Vec<String> = first
            .iter()
            .map(|(path, _)| {
                path.strip_prefix(&dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert_eq!(
            names,
            ["Cargo.toml", "scripts/Cargo.toml", "scripts/src/lib.rs"]
        );
        assert_eq!(scan(&dir), first);

        // A save: the file has another time.
        let file = std::fs::File::options()
            .write(true)
            .open(dir.join("scripts/src/lib.rs"));
        let later = SystemTime::now() + Duration::from_secs(5);
        file.unwrap().set_modified(later).unwrap();
        assert_ne!(scan(&dir), first);
        // A new file, and a file that is gone.
        std::fs::write(dir.join("scripts/src/new.rs"), "").unwrap();
        assert_eq!(scan(&dir).len(), 4);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
