//! The game: its module list, its benchmark scripts, and its own event command. The
//! rooms and their events are data, in `game/assets/rooms`.

mod scripts;
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

fn usage() -> String {
    format!("usage: maki-game [CONFIG] {}", maki_devtools::USAGE)
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
    let config_path = config_path(args).with_context(usage)?;

    let mut app = App::new(ConfigFile::load(&config_path)?)?;
    // The order of the modules is the order of the keys in the help panel, and of
    // the debug panels in their column.
    app.add_module(maki_platform::PlatformModule)
        .add_module(maki_assets::AssetsModule)
        .add_module(maki_scene::SceneModule)
        .add_module(maki_events::EventsModule)
        .add_module(maki_gameplay::ControlsModule)
        .add_module(maki_gameplay::PlayerModule)
        .add_module(maki_gameplay::CameraModule)
        .add_module(maki_gameplay::RoomsModule {
            start_room: PathBuf::from(START_ROOM),
            start_spawn: START_SPAWN.to_owned(),
        })
        .add_module(shake::ShakeModule)
        .add_module(maki_render::RenderModule)
        .add_module(maki_ui::UiModule)
        // Debug tools, but for the message window of the events.
        .add_module(maki_ui::StatsPanelModule)
        .add_module(maki_ui::RenderPanelModule)
        .add_module(maki_devtools::GameplayPanelModule)
        .add_module(maki_ui::EventsUiModule)
        .add_module(maki_devtools::StagesModule)
        .add_module(maki_devtools::BenchModule {
            start: bench,
            scripts: scripts::SCRIPTS,
        })
        .add_module(maki_gameplay::LightControlModule)
        .add_module(maki_gameplay::MoverToggleModule);

    // All modules are added: the errors of all config tables, with the table and
    // the key. Then the start room: a bad one, and the game does not start. The
    // error has the file and the line.
    app.check_config()?;
    maki_gameplay::check_start_room(&app)?;
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
}
