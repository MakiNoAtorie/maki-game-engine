//! The game: its module list, its benchmark scripts, and its own event command. The
//! rooms and their events are data, in `game/assets/rooms`.

mod scripts;
// With `dynamic`, the script DLL has this file.
#[cfg(not(feature = "dynamic"))]
mod shake;

use std::ffi::OsString;
use std::path::PathBuf;

use anyhow::{Context, bail};
use maki_core::{App, ConfigFile};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

/// Log filter variable, same syntax as `RUST_LOG` (for example `debug` or `maki_core=trace`).
const LOG_ENV: &str = "MAKI_LOG";

/// Where the game starts if the config file names no room.
const START_ROOM: &str = "game/assets/rooms/room_a.ron";
const START_SPAWN: &str = "start";
/// The save file, in the working directory.
const SAVE_FILE: &str = "save.ron";
/// The room files of the game: where the editor opens and saves.
#[cfg(feature = "editor")]
const ROOMS: &str = "game/assets/rooms";

fn usage() -> String {
    #[cfg(feature = "editor")]
    let editor = maki_editor::USAGE;
    #[cfg(not(feature = "editor"))]
    let editor = "";
    format!(
        "usage: maki-game [CONFIG] {editor} {}",
        maki_devtools::USAGE
    )
}

fn init_logging() {
    let filter = EnvFilter::try_from_env(LOG_ENV).unwrap_or_else(|_| EnvFilter::new("info"));
    // The filter is on the text output only. The profiler gets the spans of the
    // schedules and passes (trace level) at each log level.
    let subscriber =
        tracing_subscriber::registry().with(tracing_subscriber::fmt::layer().with_filter(filter));
    #[cfg(feature = "profile")]
    let subscriber = subscriber.with(tracing_tracy::TracyLayer::default());
    subscriber.init();
}

/// The config path of the command line, after the benchmark module took its options.
fn config_path(args: Vec<OsString>) -> anyhow::Result<PathBuf> {
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

fn main() -> anyhow::Result<()> {
    init_logging();

    let args = std::env::args_os().skip(1);
    let (bench, args) = maki_devtools::parse_args(args, scripts::SCRIPTS).with_context(usage)?;
    #[cfg(feature = "editor")]
    let (editor, args) = maki_editor::parse_args(args).with_context(usage)?;
    #[cfg(feature = "editor")]
    let (editor, resave) = (editor.open, editor.resave);
    #[cfg(not(feature = "editor"))]
    let editor = false;
    let config_path = config_path(args).with_context(usage)?;

    let mut app = App::new(ConfigFile::load(&config_path)?)?;
    // The order of the modules is the order of the keys in the help panel, and of
    // the debug panels in their column.
    app.add_module(maki_platform::PlatformModule)
        .add_module(maki_assets::AssetsModule)
        .add_module(maki_scene::SceneModule)
        .add_module(maki_events::EventsModule)
        .add_module(maki_audio::AudioModule)
        .add_module(maki_gameplay::ControlsModule)
        .add_module(maki_gameplay::PlayerModule)
        .add_module(maki_gameplay::CameraModule)
        .add_module(maki_gameplay::RoomsModule {
            start_room: PathBuf::from(START_ROOM),
            start_spawn: START_SPAWN.to_owned(),
        })
        // Not with the editor: it plays a room that is not on disk, and its play
        // must not write the save of the game.
        .add_module(maki_gameplay::SaveModule {
            file: PathBuf::from(SAVE_FILE),
            active: !editor,
        });
    #[cfg(not(feature = "dynamic"))]
    app.add_module(shake::ShakeModule);
    // Spike of part 6: the modules of the script DLL, next to the program.
    #[cfg(feature = "dynamic")]
    maki_engine::load_project(
        &mut app,
        &std::env::current_exe()?.with_file_name("spike_scripts.dll"),
    )?;
    app.add_module(maki_render::RenderModule)
        .add_module(maki_ui::UiModule)
        // Debug tools, but for the message window of the events.
        .add_module(maki_ui::StatsPanelModule)
        .add_module(maki_ui::RenderPanelModule)
        .add_module(maki_devtools::GameplayPanelModule)
        .add_module(maki_ui::EventsUiModule);
    // The stage list goes to a room with no word to the editor: the editor has
    // "Open" in its place.
    if !editor {
        app.add_module(maki_devtools::StagesModule);
    }
    app.add_module(maki_devtools::BenchModule {
        start: bench,
        scripts: scripts::SCRIPTS,
    })
    .add_module(maki_gameplay::LightControlModule)
    .add_module(maki_gameplay::MoverToggleModule);
    // After the modules of the game: the editor reads the room files with their
    // event commands.
    #[cfg(feature = "editor")]
    if editor {
        app.add_module(maki_editor::EditorModule {
            rooms: PathBuf::from(ROOMS),
        });
    }

    // All modules are added: the errors of all config tables, with the table and
    // the key. Then the start room: a bad one, and the game does not start. The
    // error has the file and the line.
    app.check_config()?;
    #[cfg(feature = "editor")]
    if !resave.is_empty() {
        return maki_editor::resave(&app, &resave);
    }
    // Not with the editor: it opens a room with a wrong name in an event, so that
    // the name can be set right, and shows a room file that does not read.
    if !editor {
        maki_gameplay::check_start_room(&app)?;
    }
    tracing::info!(path = %config_path.display(), "config loaded");

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

    /// The editor writes the room files of the game with all that they hold: the
    /// room, the events (also the commands of the audio module and of this game),
    /// and the comments. The loader finds nothing wrong in them.
    #[cfg(all(feature = "editor", not(feature = "dynamic")))]
    #[test]
    fn the_editor_writes_each_room_file_with_the_same_content() {
        // No sound device in a test.
        let mut app = App::new(ConfigFile::from_text(
            "[audio]
output = false",
        ))
        .unwrap();
        app.add_module(maki_platform::PlatformModule)
            .add_module(maki_scene::SceneModule)
            .add_module(maki_events::EventsModule)
            .add_module(maki_audio::AudioModule)
            .add_module(maki_gameplay::CameraModule)
            .add_module(maki_gameplay::SaveModule {
                file: PathBuf::from(SAVE_FILE),
                active: false,
            })
            .add_module(shake::ShakeModule);
        let registry = app.world().resource::<maki_events::CommandRegistry>();
        let mut events = 0;
        for name in ["room_a.ron", "room_b.ron", "room_c.ron"] {
            // A test runs in the directory of its package.
            let path = PathBuf::from("assets/rooms").join(name);
            let validators = app.world().resource::<maki_scene::RoomValidators>();
            maki_scene::read_room(&path, validators).unwrap();
            let document = maki_editor::Document::read(&path, registry).unwrap();
            let text = document.text();
            assert_eq!(maki_scene::parse_room(&text).unwrap(), document.room);
            assert_eq!(registry.parse_events(&text).unwrap(), document.events);
            assert_eq!(maki_editor::read_notes(&text), document.notes);
            assert!(!document.notes.room.is_empty() && !document.notes.items.is_empty());
            events += document.events.len();
        }
        assert_eq!(events, 7);
    }
}
