//! The loader of the script DLL, with the DLL of the game of this repository. Run
//! with the build command of the editor, as a test:
//!
//!     cargo test --profile dynamic -p maki-editor-app -p maki-game-dll

use std::path::{Path, PathBuf};

use maki_engine::core::{App, ConfigFile};
use maki_engine::events::CommandRegistry;
use maki_engine::{events, gameplay, platform, scene};

/// The modules that the script crate of the game needs.
fn app() -> App {
    let mut app = App::new(ConfigFile::from_text("")).unwrap();
    app.add_module(platform::PlatformModule)
        .add_module(scene::SceneModule)
        .add_module(events::EventsModule)
        .add_module(gameplay::CameraModule);
    app
}

/// A DLL of this build: beside this test program.
fn beside(name: &str) -> PathBuf {
    std::env::current_exe().unwrap().with_file_name(name)
}

/// True: the app has the event command of the script crate.
fn has_shake(app: &App) -> bool {
    let registry = app.world().resource::<CommandRegistry>();
    registry.parse_command("ShakeCamera(0.5, 0.05)").is_ok()
}

fn error(app: &mut App, dll: &Path) -> String {
    let error = maki_engine::load_project(app, dll).unwrap_err();
    assert!(!has_shake(app), "the DLL was refused, but its code ran");
    format!("{error:#}")
}

#[test]
fn the_script_dll_of_the_project_is_loaded_and_a_wrong_dll_is_refused() {
    let project = maki_engine::Project::load(Path::new("../../game")).unwrap();
    let dll = beside(&format!("{}.dll", project.scripts.replace('-', "_")));

    // The DLL of this build: its command is in the app.
    let mut loaded = app();
    assert!(!has_shake(&loaded));
    maki_engine::load_project(&mut loaded, &dll).unwrap();
    assert!(has_shake(&loaded));

    // An older file than the engine DLL: of another build of the engine.
    let old = std::env::temp_dir().join(format!("maki-old-{}.dll", std::process::id()));
    std::fs::copy(&dll, &old).unwrap();
    let before = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
    let file = std::fs::File::options().write(true).open(&old).unwrap();
    file.set_modified(before).unwrap();
    drop(file);
    let text = error(&mut app(), &old);
    assert!(text.contains("another build of the engine"), "{text}");
    std::fs::remove_file(&old).unwrap();

    // A DLL with the engine crates in it (the engine DLL is one): its own copy.
    let text = error(&mut app(), &beside("maki_engine_dylib.dll"));
    assert!(text.contains("its own copy of the engine"), "{text}");

    // A new file that is no DLL.
    std::fs::write(&old, "not a DLL").unwrap();
    let text = error(&mut app(), &old);
    assert!(text.contains("does not load"), "{text}");
    std::fs::remove_file(&old).unwrap();
}
