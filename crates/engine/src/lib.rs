//! The engine as one crate: the last engine crate, and the only one that a project
//! names.
//!
//! - The engine crates by their short names ([`core`], [`scene`], [`events`], ...):
//!   the script crate of a project takes all of the engine from here.
//! - [`Project`]: the file `project.toml` of a project folder.
//! - The module list of a game and of the editor, the command line, and the log
//!   setup. No constant of a game is here: the project file has them.
//! - Two entry functions. [`run_game`]: the program of a project calls it with the
//!   entry function of its script crate; engine and script crate are linked
//!   statically, with no editor. `run_editor` (feature `dynamic`): the editor
//!   program, with no code of a game. It builds the script crate of the project as a
//!   DLL and loads it (`loader` module).
//!
//! The two builds:
//!
//! | Build | Command | Engine crates | Script crate |
//! |---|---|---|---|
//! | Static | `cargo build -p <game>` | In the program | In the program |
//! | Dynamic | [`BUILD`] and the DLL package of the project | `maki_engine_dylib.dll` | A DLL that the editor loads |

#[cfg(feature = "dynamic")]
mod editor_program;
#[cfg(feature = "dynamic")]
mod loader;
mod project;
#[cfg(feature = "dynamic")]
mod reload;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use maki_core::{App, ConfigFile};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

pub use maki_assets as assets;
pub use maki_audio as audio;
pub use maki_core as core;
pub use maki_devtools as devtools;
pub use maki_events as events;
pub use maki_gameplay as gameplay;
pub use maki_platform as platform;
pub use maki_render as render;
pub use maki_scene as scene;
pub use maki_ui as ui;

#[cfg(feature = "dynamic")]
pub use editor_program::{BUILD, run_editor, script_dll};
#[cfg(feature = "dynamic")]
pub use loader::{ENTRY, load_project};
#[cfg(feature = "dynamic")]
pub use maki_editor as editor;
pub use project::Project;

// Without this line nothing names the engine DLL, and a program or a script DLL gets
// its own copy of each engine crate, with no error.
#[cfg(feature = "dynamic")]
extern crate maki_engine_dylib as _;

/// What the `main` function of a program returns.
pub type Result<T = ()> = anyhow::Result<T>;

/// The entry function of a project: it adds the modules of its script crate. The
/// program of the game calls it; the DLL of the project exports it for the editor
/// ([`export_project`]).
pub type Entry = fn(&mut App);

/// In the DLL crate of a project: exports the entry function of the script crate,
/// for the editor.
///
/// ```ignore
/// maki_engine::export_project!(my_scripts::add_modules);
/// ```
#[macro_export]
macro_rules! export_project {
    ($entry:path) => {
        /// What the editor calls after it loaded the DLL.
        #[unsafe(no_mangle)]
        pub fn maki_project(app: &mut $crate::core::App) {
            $entry(app)
        }
    };
}

/// Log filter variable, same syntax as `RUST_LOG` (for example `debug` or `maki_core=trace`).
const LOG_ENV: &str = "MAKI_LOG";

/// The log setup of a program: text to the terminal, with the filter of `MAKI_LOG`.
pub fn init_logging() {
    let filter = EnvFilter::try_from_env(LOG_ENV).unwrap_or_else(|_| EnvFilter::new("info"));
    // The filter is on the text output only. The profiler gets the spans of the
    // schedules and passes (trace level) at each log level.
    let subscriber =
        tracing_subscriber::registry().with(tracing_subscriber::fmt::layer().with_filter(filter));
    #[cfg(feature = "profile")]
    let subscriber = subscriber.with(tracing_tracy::TracyLayer::default());
    subscriber.init();
}

/// The config path of a command line, after the options were taken out of it.
fn config_path(args: Vec<OsString>) -> Result<PathBuf> {
    let mut path = None;
    for arg in args {
        if arg.to_string_lossy().starts_with("--") {
            bail!("unknown option {arg:?}");
        } else if path.replace(PathBuf::from(arg)).is_some() {
            bail!("more than one config path");
        }
    }
    Ok(path.unwrap_or_else(|| PathBuf::from(ConfigFile::DEFAULT_PATH)))
}

/// The modules that a game and the editor have, in their order: it is the order of
/// the keys in the help panel, and of the debug panels. `project_modules` adds the
/// modules of the script crate, at their place in the list. `editor`: the program is
/// the editor.
fn add_modules(
    app: &mut App,
    project: &Project,
    editor: bool,
    project_modules: impl FnOnce(&mut App),
) {
    app.add_module(maki_platform::PlatformModule)
        .add_module(maki_assets::AssetsModule)
        .add_module(maki_scene::SceneModule)
        .add_module(maki_events::EventsModule)
        .add_module(maki_audio::AudioModule)
        .add_module(maki_gameplay::ControlsModule)
        .add_module(maki_gameplay::PlayerModule {
            rooms: project.rooms.clone(),
            prefab: project.player.clone(),
        })
        .add_module(maki_gameplay::CameraModule)
        .add_module(maki_gameplay::RoomsModule {
            start_room: project.start_room.clone(),
            start_spawn: project.start_spawn.clone(),
        })
        // Not with the editor: it plays a room that is not on disk, and its play
        // must not write the save of the game.
        .add_module(maki_gameplay::SaveModule {
            file: project.save.clone(),
            active: !editor,
        });
    project_modules(app);
    app.add_module(maki_render::RenderModule)
        .add_module(maki_ui::UiModule)
        // Debug tools, but for the message window of the events.
        .add_module(maki_ui::StatsPanelModule)
        .add_module(maki_ui::RenderPanelModule)
        .add_module(maki_devtools::GameplayPanelModule)
        .add_module(maki_ui::EventsUiModule);
}

/// Runs the game of a project. `project`: the project folder (the folder of the
/// program crate: `env!("CARGO_MANIFEST_DIR")`). `entry`: the entry function of its
/// script crate.
///
/// The command line: `[CONFIG]` and the options of the benchmark.
pub fn run_game(project: &str, entry: Entry) -> Result {
    init_logging();
    let usage = || format!("usage: [CONFIG] {}", maki_devtools::USAGE);
    let args = std::env::args_os().skip(1);
    let (bench, args) = maki_devtools::parse_args(args).with_context(usage)?;
    let config_path = config_path(args).with_context(usage)?;
    let project = Project::load(Path::new(project))?;

    let mut app = App::new(ConfigFile::load(&config_path)?)?;
    add_modules(&mut app, &project, false, entry);
    // The stage list goes to a room with no word to the editor: the editor has
    // "Open" in its place.
    app.add_module(maki_devtools::StagesModule)
        .add_module(maki_devtools::BenchModule { start: bench })
        .add_module(maki_gameplay::LightControlModule)
        .add_module(maki_gameplay::MoverToggleModule);

    // All modules are added: the errors of all config tables, with the table and
    // the key. Then the start room: a bad one, and the game does not start. The
    // error has the file and the line.
    app.check_config()?;
    maki_devtools::check_bench(&app).with_context(usage)?;
    maki_gameplay::check_start_room(&app)?;
    tracing::info!(project = %project.name, config = %config_path.display(), "config loaded");

    maki_platform::run(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_line_gives_the_config_path() {
        let path = |args: &[&str]| config_path(args.iter().map(OsString::from).collect());
        assert_eq!(path(&[]).unwrap(), PathBuf::from(ConfigFile::DEFAULT_PATH));
        assert_eq!(path(&["my.toml"]).unwrap(), PathBuf::from("my.toml"));
        assert!(path(&["--bench"]).is_err());
        assert!(path(&["a.toml", "b.toml"]).is_err());
    }
}
