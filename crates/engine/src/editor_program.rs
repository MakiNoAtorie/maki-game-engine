//! The editor program: the engine and the editor, with no code of a game. It opens a
//! project: it builds the script crate of the project as a DLL, loads it, and adds
//! its modules at their place in the module list.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, bail};
use maki_core::{App, ConfigFile};
use maki_ui::{NoticeLevel, Notices};

use crate::{Project, Result, add_modules, config_path, init_logging, load_project};

/// The build command of the editor and of the script DLL of a project: the arguments
/// of `cargo`. The package of the project comes after them (`-p NAME`).
///
/// One call with one package set, always: with another set, cargo can give some
/// crates other features and build the engine DLL again, and the editor that runs
/// has that file. `editor.bat` builds with the same command.
pub const BUILD: [&str; 5] = ["build", "--profile", "dynamic", "-p", "maki-editor-app"];

/// The project folder if the command line names none.
const DEFAULT_PROJECT: &str = "game";

const USAGE: &str = "usage: maki-editor [PROJECT [CONFIG]] [--resave ROOM]...";

/// The lines of a build error that a notice shows. The log has all of them.
const ERROR_LINES: usize = 12;

/// Builds the script DLL of the project. Returns its path: beside this program.
fn build_project(project: &Project) -> Result<PathBuf> {
    // The program that `cargo run` and `cargo test` name, else that of the path.
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command.args(BUILD).args(["-p", &project.scripts]);
    // In the project folder: cargo finds the workspace of the script crate.
    let output = command
        .current_dir(&project.dir)
        .output()
        .context("cannot run cargo to build the script crate")?;
    if !output.status.success() {
        let text = String::from_utf8_lossy(&output.stderr);
        tracing::error!("the build of the script crate failed:\n{text}");
        let lines: Vec<&str> = text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect();
        let last = &lines[lines.len().saturating_sub(ERROR_LINES)..];
        bail!(
            "The build of the script crate `{}` failed. The rooms open with no command of the \
             project.\n{}",
            project.scripts,
            last.join("\n")
        );
    }
    script_dll(project)
}

/// Where the build command puts the script DLL of a project: beside this program.
pub fn script_dll(project: &Project) -> Result<PathBuf> {
    let name = libloading::library_filename(project.scripts.replace("-", "_"));
    Ok(std::env::current_exe()?.with_file_name(name))
}

/// Runs the editor. The command line: the project folder (not given: `game`), the
/// config file, and `--resave ROOM` (one or more times): each room file is read and
/// written again as the editor saves it, and the program ends.
pub fn run_editor() -> Result {
    init_logging();
    let (mut resave, mut rest) = (Vec::new(), Vec::new());
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--resave" {
            let path = args.next().context("--resave needs a room file");
            resave.push(PathBuf::from(path.context(USAGE)?));
        } else {
            rest.push(arg);
        }
    }
    let mut rest = rest.into_iter();
    let dir = rest.next().unwrap_or_else(|| DEFAULT_PROJECT.into());
    if dir.to_string_lossy().starts_with("--") {
        bail!("unknown option {dir:?}\n{USAGE}");
    }
    let config_path = config_path(rest.collect()).context(USAGE)?;
    let project = Project::load(dir.as_ref())?;

    let mut app = App::new(ConfigFile::load(&config_path)?)?;
    let built = build_project(&project);
    // Why the project has no code in this run.
    let mut failure = None;
    add_modules(&mut app, &project, true, |app| {
        let loaded = built.and_then(|dll| {
            load_project(app, &dll).inspect_err(|error| tracing::error!("{error:#}"))
        });
        failure = loaded.err();
    });
    // After the modules of the project: the editor reads the room files with their
    // event commands. No benchmark and no stage list: the game has them.
    app.add_module(maki_gameplay::LightControlModule)
        .add_module(maki_gameplay::MoverToggleModule)
        .add_module(maki_editor::EditorModule {
            rooms: project.rooms.clone(),
            project: project.name.clone(),
        });
    if let Some(error) = failure {
        app.init_resource::<Notices>();
        let mut notices = app.resource_mut::<Notices>();
        notices.push(NoticeLevel::Error, format!("{error:#}"));
    }

    app.check_config()?;
    if !resave.is_empty() {
        return maki_editor::resave(&app, &resave);
    }
    tracing::info!(project = %project.name, config = %config_path.display(), "config loaded");
    maki_platform::run(app)
}
