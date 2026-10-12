mod bench;
mod camera;
mod controls;
mod debug;
mod events_ui;
mod light;
mod player;
mod rooms;
mod tick_logger;

use std::ffi::OsString;
use std::path::PathBuf;

use anyhow::{Context, bail};
use maki_core::{App, Config};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

/// Log filter variable, same syntax as `RUST_LOG` (for example `debug` or `maki_core=trace`).
const LOG_ENV: &str = "MAKI_LOG";

const USAGE: &str = "usage: maki-game [CONFIG] [--bench-frames N [--bench-dump FILE [--bench-shot]] [--bench-static | --bench-light-frames N] [--bench-script walk|trip]]";

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

#[derive(Debug, PartialEq)]
struct Args {
    config_path: PathBuf,
    /// `Some`: benchmark mode, for this many frames. See the `bench` module.
    bench_frames: Option<u64>,
    /// With `bench_frames`: the file for the picture of the last frame.
    bench_dump: Option<PathBuf>,
    /// With `bench_dump`: the file gets the picture as the window shows it, with no panel.
    bench_shot: bool,
    /// With `bench_frames`: the light does not move.
    bench_static: bool,
    /// With `bench_frames`: the light moves in this many frames, then it stops.
    bench_light_frames: Option<u64>,
    /// With `bench_frames`: the name of a script that plays the game. See the `bench` module.
    bench_script: Option<String>,
}

/// `args` is the command line without the program name.
fn parse_args(args: impl IntoIterator<Item = OsString>) -> anyhow::Result<Args> {
    let mut args = args.into_iter();
    let (mut config_path, mut bench_frames, mut bench_dump) = (None, None, None);
    let (mut bench_static, mut bench_script, mut bench_light_frames) = (false, None, None);
    let mut bench_shot = false;
    while let Some(arg) = args.next() {
        if arg == "--bench-frames" {
            // The limit keeps a typing error from a table of rows that fills the memory.
            let frames = args
                .next()
                .and_then(|value| value.to_str()?.parse::<u64>().ok())
                .filter(|frames| (1..=1_000_000).contains(frames))
                .with_context(|| {
                    format!("--bench-frames needs a number from 1 to 1000000. {USAGE}")
                })?;
            bench_frames = Some(frames);
        } else if arg == "--bench-static" {
            bench_static = true;
        } else if arg == "--bench-shot" {
            bench_shot = true;
        } else if arg == "--bench-light-frames" {
            let frames = args.next().and_then(|frames| frames.to_str()?.parse().ok());
            bench_light_frames = Some(
                frames.with_context(|| format!("--bench-light-frames needs a number. {USAGE}"))?,
            );
        } else if arg == "--bench-script" {
            let name = args.next().and_then(|name| name.into_string().ok());
            let name = name.filter(|name| bench::script(name).is_some());
            let name =
                name.with_context(|| format!("--bench-script needs a script name. {USAGE}"))?;
            bench_script = Some(name);
        } else if arg == "--bench-dump" {
            let path = args
                .next()
                .with_context(|| format!("--bench-dump needs a file. {USAGE}"))?;
            bench_dump = Some(PathBuf::from(path));
        } else if arg.to_string_lossy().starts_with("--") {
            bail!("unknown option {arg:?}. {USAGE}");
        } else if config_path.replace(PathBuf::from(arg)).is_some() {
            bail!("more than one config path. {USAGE}");
        }
    }
    if (bench_dump.is_some() || bench_static || bench_script.is_some()) && bench_frames.is_none() {
        bail!("--bench-dump, --bench-static, and --bench-script need --bench-frames. {USAGE}");
    }
    Ok(Args {
        config_path: config_path.unwrap_or_else(|| PathBuf::from(Config::DEFAULT_PATH)),
        bench_frames,
        bench_dump,
        bench_shot,
        bench_static,
        bench_light_frames,
        bench_script,
    })
}

fn main() -> anyhow::Result<()> {
    init_logging();

    let args = parse_args(std::env::args_os().skip(1))?;
    let mut config = Config::load_or_create(&args.config_path)?;
    if args.bench_frames.is_some() {
        // With vsync, each frame would have the length of one display refresh.
        config.vsync = false;
    }
    tracing::info!(path = %args.config_path.display(), ?config, "config loaded");

    // A bad start room: the game does not start. The error has the file and the line.
    let checks = maki_scene::RoomValidators(vec![maki_events::validate_events]);
    maki_scene::read_room(&config.start_room, &checks)?;

    let mut app = App::new(config)?;
    app.add_module(maki_platform::PlatformModule)
        .add_module(maki_assets::AssetsModule)
        .add_module(maki_render::RenderModule)
        .add_module(maki_ui::UiModule)
        .add_module(maki_ui::StatsPanelModule)
        .add_module(debug::DebugPanelModule)
        .add_module(maki_scene::SceneModule)
        .add_module(maki_events::EventsModule)
        .add_module(events_ui::EventsUiModule)
        .add_module(rooms::RoomsModule)
        .add_module(controls::ControlsModule)
        .add_module(camera::CameraModule)
        .add_module(player::PlayerControllerModule)
        .add_module(tick_logger::TickLoggerModule)
        .add_module(light::LightControllerModule)
        .add_module(bench::BenchModule {
            start: args.bench_frames.map(|frames| bench::BenchRun {
                frames,
                dump: args.bench_dump,
                shot: args.bench_shot,
                // A light that does not move: one that stops before the first frame.
                light_frames: if args.bench_static {
                    Some(0)
                } else {
                    args.bench_light_frames
                },
                script: args.bench_script.as_deref().and_then(bench::script),
            }),
        });

    maki_platform::run(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> anyhow::Result<Args> {
        parse_args(args.iter().map(OsString::from))
    }

    #[test]
    fn the_command_line_gives_the_config_path_and_the_bench_frames() {
        let default_path = PathBuf::from(Config::DEFAULT_PATH);
        assert_eq!(
            parse(&[]).unwrap(),
            Args {
                config_path: default_path.clone(),
                bench_frames: None,
                bench_dump: None,
                bench_shot: false,
                bench_static: false,
                bench_light_frames: None,
                bench_script: None,
            }
        );
        // In any order.
        for args in [
            ["my.toml", "--bench-frames", "300"],
            ["--bench-frames", "300", "my.toml"],
        ] {
            assert_eq!(
                parse(&args).unwrap(),
                Args {
                    config_path: PathBuf::from("my.toml"),
                    bench_frames: Some(300),
                    bench_dump: None,
                    bench_shot: false,
                    bench_static: false,
                    bench_light_frames: None,
                    bench_script: None,
                }
            );
        }
        assert_eq!(
            parse(&["--bench-frames", "3", "--bench-dump", "a.bin"])
                .unwrap()
                .bench_dump,
            Some(PathBuf::from("a.bin"))
        );
        assert_eq!(
            parse(&["--bench-frames", "3", "--bench-script", "trip"])
                .unwrap()
                .bench_script
                .as_deref(),
            Some("trip")
        );
        for bad in [
            &["--bench-frames"][..],
            &["--bench-frames", "0"],
            &["--bench-frames", "many"],
            &["--bench"],
            &["--bench-dump", "a.bin"],
            &["--bench-static"],
            &["--bench-script", "walk"],
            &["--bench-frames", "3", "--bench-script"],
            &["--bench-frames", "3", "--bench-script", "fly"],
            &["--bench-frames", "3", "--bench-dump"],
            &["a.toml", "b.toml"],
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }
}
