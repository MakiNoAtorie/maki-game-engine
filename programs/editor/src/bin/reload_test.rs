//! The reload test of the loader (`tools/reload_test.py` runs it): a program with no
//! code of a game loads the script DLL of a project, shows what its code does, and
//! does so again after each new build of the DLL. The app of the first load stays,
//! with the old code.
//!
//! `reload_test PROJECT [LOADS]`: ends after that many loads (default 2).

use std::path::Path;
use std::time::Duration;

use maki_engine::core::{App, ConfigFile};
use maki_engine::events::CommandRegistry;
use maki_engine::{Project, Result, events, gameplay, platform, scene};

/// An event with the command of the script crate of the game.
const EVENTS: &str = r#"(events: [(name: "e", pages: [(trigger: Autorun, commands: [
    ShakeCamera(0.5, 0.05)])])])"#;

/// A new app with some engine modules and the modules of the DLL.
fn build(dll: &Path) -> Result<App> {
    let mut app = App::new(ConfigFile::from_text(""))?;
    app.add_module(platform::PlatformModule)
        .add_module(scene::SceneModule)
        .add_module(events::EventsModule)
        .add_module(gameplay::CameraModule);
    maki_engine::load_project(&mut app, dll)?;
    app.check_config()?;
    Ok(app.run(|app| app))
}

/// The rows of the commands (closures of the DLL), and one frame (its systems).
fn report(name: &str, app: &mut App) -> Result {
    let registry = app.world().resource::<CommandRegistry>();
    let events = registry.parse_events(EVENTS)?;
    for command in &events[0].pages[0].commands {
        let text = registry.row_text(command);
        println!("{name}: {}", text.as_deref().unwrap_or("no row text"));
    }
    app.advance(Duration::from_millis(16));
    Ok(())
}

fn main() -> Result {
    maki_engine::init_logging();
    let mut args = std::env::args().skip(1);
    let dir = args.next().unwrap_or_else(|| "game".to_owned());
    let project = Project::load(dir.as_ref())?;
    let loads = args.next().map_or(Ok(2), |text| text.parse())?;
    let dll = maki_engine::script_dll(&project)?;

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
