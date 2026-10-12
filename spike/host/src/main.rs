//! Spike of part 6. A program with no code of a game: it loads the script DLL, shows
//! what its code does, and does so again after each new build of the DLL.
//!
//! `spike-host [loads]`: ends after that many loads (default 2).

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use maki_core::{App, ConfigFile};
use maki_events::CommandRegistry;

const DLL: &str = "spike_scripts.dll";
const EVENTS: &str = r#"(events: [(name: "e", pages: [(trigger: Autorun, commands: [
    ShakeCamera(0.5, 0.05), Stamp])])])"#;

/// A new app with the engine modules and the modules of the DLL.
fn build(dll: &Path) -> Result<App> {
    let mut app = App::new(ConfigFile::from_text(""))?;
    app.add_module(maki_platform::PlatformModule)
        .add_module(maki_scene::SceneModule)
        .add_module(maki_events::EventsModule)
        .add_module(maki_gameplay::CameraModule);
    maki_engine::load_project(&mut app, dll)?;
    app.check_config()?;
    Ok(app.run(|app| app))
}

/// The rows of the commands (closures of the DLL), and one frame (its systems).
fn report(name: &str, app: &mut App) -> Result<()> {
    let registry = app.world().resource::<CommandRegistry>();
    let events = registry.parse_events(EVENTS)?;
    for command in &events[0].pages[0].commands {
        println!("{name}: {}", registry.row_text(command).context("no row text")?);
    }
    app.advance(Duration::from_millis(16));
    Ok(())
}

fn main() -> Result<()> {
    tracing_subscriber::fmt().init();
    let loads = std::env::args().nth(1).map_or(Ok(2), |text| text.parse())?;
    let exe = std::env::current_exe()?;
    let dll = exe.parent().context("no directory")?.join(DLL);

    let mut apps: Vec<App> = Vec::new();
    let mut seen = None;
    while apps.len() < loads {
        let written = std::fs::metadata(&dll)?.modified()?;
        if seen == Some(written) {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        // The linker can still have the file: then the next turn of the loop.
        let Ok(mut app) = build(&dll) else { continue };
        seen = Some(written);
        report(&format!("load {}", apps.len()), &mut app)?;
        apps.push(app);
        println!("waiting");
    }
    // The first copy is still loaded: its app runs the old code.
    report("load 0 again", &mut apps[0])?;
    Ok(())
}
