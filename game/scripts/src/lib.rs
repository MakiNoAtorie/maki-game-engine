//! The script crate of the project: the Rust code of this game. Its event command
//! (`shake`), and its benchmark scripts (`bench`).
//!
//! The game program and the editor call [`add_modules`]: the program has this crate
//! linked in, the editor loads it as a DLL (`dll/`).

mod bench;
mod shake;

use maki_engine::core::App;

/// The entry function of the project: adds the modules of this crate.
pub fn add_modules(app: &mut App) {
    app.add_module(shake::ShakeModule);
    bench::add_scripts(app);
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use maki_engine::core::ConfigFile;
    use maki_engine::{audio, events, gameplay, platform, scene};

    use super::*;

    /// The editor writes the room files of the game with all that they hold: the
    /// room, the events (also the commands of the audio module and of this game),
    /// and the comments. The loader finds nothing wrong in them.
    #[test]
    fn the_editor_writes_each_room_file_with_the_same_content() {
        // No sound device in a test.
        let mut app = App::new(ConfigFile::from_text(
            "[audio]
output = false",
        ))
        .unwrap();
        app.add_module(platform::PlatformModule)
            .add_module(scene::SceneModule)
            .add_module(events::EventsModule)
            .add_module(audio::AudioModule)
            .add_module(gameplay::CameraModule)
            .add_module(gameplay::SaveModule {
                file: PathBuf::from("save.ron"),
                active: false,
            });
        add_modules(&mut app);
        let registry = app.world().resource::<events::CommandRegistry>();
        let mut count = 0;
        for name in ["room_a.ron", "room_b.ron", "room_c.ron"] {
            // A test runs in the directory of its package.
            let path = PathBuf::from("../assets/rooms").join(name);
            let validators = app.world().resource::<scene::RoomValidators>();
            scene::read_room(&path, validators).unwrap();
            let document = maki_editor::Document::read(&path, registry).unwrap();
            let text = document.text();
            assert_eq!(scene::parse_room(&text).unwrap(), document.room);
            assert_eq!(registry.parse_events(&text).unwrap(), document.events);
            assert_eq!(maki_editor::read_notes(&text), document.notes);
            assert!(!document.notes.room.is_empty() && !document.notes.items.is_empty());
            count += document.events.len();
        }
        assert_eq!(count, 7);
    }
}
