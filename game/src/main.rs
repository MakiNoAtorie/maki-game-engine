mod tick_logger;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use maki_core::{App, Config};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Run time of the headless loop. The window event loop replaces this loop in the next stage.
const HEADLESS_RUN_TIME: Duration = Duration::from_secs(5);

/// Log filter variable, same syntax as `RUST_LOG` (for example `debug` or `maki_core=trace`).
const LOG_ENV: &str = "MAKI_LOG";

fn init_logging() {
    let filter = EnvFilter::try_from_env(LOG_ENV).unwrap_or_else(|_| EnvFilter::new("info"));
    let subscriber = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer());
    #[cfg(feature = "profile")]
    let subscriber = subscriber.with(tracing_tracy::TracyLayer::default());
    subscriber.init();
}

fn main() -> anyhow::Result<()> {
    init_logging();

    // First CLI argument is the config path.
    let config_path = std::env::args_os()
        .nth(1)
        .map_or_else(|| PathBuf::from(Config::DEFAULT_PATH), PathBuf::from);
    let config = Config::load_or_create(&config_path)?;
    tracing::info!(path = %config_path.display(), ?config, "config loaded");

    let mut app = App::new(config)?;
    app.add_module(tick_logger::TickLoggerModule);

    app.run(|mut app| {
        let start = Instant::now();
        while start.elapsed() < HEADLESS_RUN_TIME {
            app.update();
            // No vsync here. Sleep so that the loop does not use a full CPU core.
            std::thread::sleep(Duration::from_millis(1));
        }
    });

    Ok(())
}
