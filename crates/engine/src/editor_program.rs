//! The editor program: the engine and the editor, with no code of a game. It opens a
//! project: it builds the script crate of the project as a DLL, loads it, and adds
//! its modules at their place in the module list. The `reload` module does the same
//! again while the editor runs.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, bail};
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Local, Res};
use maki_core::{App, ConfigFile, Update};
use maki_editor::Session;
use maki_ui::{NoticeLevel, Notices};

use crate::new_script::NewScriptModule;
use crate::reload::{BuildReport, ProjectModule, build, open_in_code_editor};
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

/// What each app of one run of the editor is made from.
#[derive(Debug, Clone)]
pub(crate) struct Setup {
    pub project: Project,
    pub config_path: PathBuf,
    /// The folder of the cargo workspace of the script crate: a build names a file
    /// from here.
    pub workspace: PathBuf,
}

/// Where the build command puts the script DLL of a project: beside this program.
pub fn script_dll(project: &Project) -> Result<PathBuf> {
    let name = libloading::library_filename(project.scripts.replace('-', "_"));
    Ok(std::env::current_exe()?.with_file_name(name))
}

/// The folder of the cargo workspace that a project folder is in.
fn workspace_of(project: &Project) -> PathBuf {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command.args(["locate-project", "--workspace", "--message-format", "plain"]);
    let output = command.current_dir(&project.dir).output();
    let output = output.ok().filter(|output| output.status.success());
    let manifest = output.map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned());
    let folder = manifest.and_then(|manifest| Some(PathBuf::from(manifest).parent()?.to_owned()));
    // No cargo: the build fails too, and its error says so.
    folder.unwrap_or_default()
}

/// An editor app with the modules of the script DLL as the last build wrote it, not
/// started. `session`: the document of the app before it. `report`: the build that
/// the app is made after; a build that failed loads no DLL.
///
/// Returns the app, and why the project has no code in it.
pub(crate) fn editor_app(
    setup: &Setup,
    session: Option<Session>,
    report: BuildReport,
) -> Result<(App, Option<String>)> {
    let project = &setup.project;
    let mut app = App::new(ConfigFile::load(&setup.config_path)?)?;
    let mut failure = None;
    add_modules(&mut app, project, true, |app| {
        failure = match report.ok {
            true => {
                let loaded = script_dll(project).and_then(|dll| load_project(app, &dll));
                loaded.err().map(|error| format!("{error:#}"))
            }
            false => Some(format!(
                "The build of the script crate `{}` failed.\n{}",
                project.scripts,
                report.errors()
            )),
        };
    });
    // After the modules of the project: the editor reads the room files with their
    // event commands. No benchmark and no stage list: the game has them.
    app.add_module(maki_gameplay::LightControlModule)
        .add_module(maki_gameplay::MoverToggleModule)
        .add_module(maki_editor::EditorModule {
            rooms: project.rooms.clone(),
            project: project.name.clone(),
            session,
        })
        .add_module(ProjectModule {
            setup: setup.clone(),
            report,
        })
        .add_module(NewScriptModule {
            project: project.dir.clone(),
        })
        .add_module(maki_devtools::LogPanelModule {
            open: open_in_code_editor,
        })
        // The log panel shows the lines of one game: it is empty at "Play".
        .add_systems(
            Update,
            (|editor: Res<maki_editor::Editor>, mut played: Local<bool>| {
                if editor.playing() && !*played {
                    maki_devtools::LOG.clear();
                }
                *played = editor.playing();
            })
            // After "Play", before the room of the game loads and logs.
            .after(maki_editor::run_commands)
            .before(maki_scene::load_requested_room),
        );
    app.check_config()?;
    Ok((app, failure))
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
    let setup = Setup {
        workspace: workspace_of(&project),
        project,
        config_path,
    };

    // The first build, before the window: the first app has the code of the project.
    let (mut app, failure) = editor_app(&setup, None, build(&setup))?;
    if let Some(failure) = failure {
        tracing::error!("{failure}");
        let text = format!("{failure}\nThe rooms open with no command of the project.");
        app.init_resource::<Notices>();
        let mut notices = app.resource_mut::<Notices>();
        notices.push(NoticeLevel::Error, text);
    }
    if !resave.is_empty() {
        return maki_editor::resave(&app, &resave);
    }
    let (project, config) = (&setup.project.name, setup.config_path.display());
    tracing::info!(%project, %config, "config loaded");
    maki_platform::run(app)
}
