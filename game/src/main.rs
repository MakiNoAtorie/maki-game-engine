mod bench;
mod controls;
mod light;
mod scene;
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

const USAGE: &str = "usage: maki-game [CONFIG] [--bench-frames N]";

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
}

/// `args` is the command line without the program name.
fn parse_args(args: impl IntoIterator<Item = OsString>) -> anyhow::Result<Args> {
    let mut args = args.into_iter();
    let (mut config_path, mut bench_frames) = (None, None);
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
        } else if arg.to_string_lossy().starts_with("--") {
            bail!("unknown option {arg:?}. {USAGE}");
        } else if config_path.replace(PathBuf::from(arg)).is_some() {
            bail!("more than one config path. {USAGE}");
        }
    }
    Ok(Args {
        config_path: config_path.unwrap_or_else(|| PathBuf::from(Config::DEFAULT_PATH)),
        bench_frames,
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

    let mut app = App::new(config)?;
    app.add_module(maki_platform::PlatformModule)
        .add_module(maki_assets::AssetsModule)
        .add_module(maki_render::RenderModule)
        .add_module(maki_ui::UiModule)
        .add_module(maki_ui::StatsPanelModule)
        .add_module(scene::SceneModule)
        .add_module(light::LightModule)
        .add_module(controls::ControlsModule)
        .add_module(tick_logger::TickLoggerModule);
    // The benchmark moves the light by itself, so the keyboard does not.
    match args.bench_frames {
        Some(frames) => app.add_module(bench::BenchModule { frames }),
        None => app.add_module(light::LightControllerModule),
    };

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
                bench_frames: None
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
                    bench_frames: Some(300)
                }
            );
        }
        for bad in [
            &["--bench-frames"][..],
            &["--bench-frames", "0"],
            &["--bench-frames", "many"],
            &["--bench"],
            &["a.toml", "b.toml"],
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }
}
