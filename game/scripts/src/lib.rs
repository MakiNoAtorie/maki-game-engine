//! The script crate of the project: the Rust code of this game. Its script
//! component (`beacon`), its event command (`shake`), and its benchmark scripts
//! (`bench`).
//!
//! The game program and the editor call [`add_modules`]: the program has this crate
//! linked in, the editor loads it as a DLL (`dll/`).

mod beacon;
mod bench;
mod shake;

use maki_engine::core::App;

/// The entry function of the project: adds the modules of this crate.
pub fn add_modules(app: &mut App) {
    beacon::add(app);
    app.add_module(shake::ShakeModule);
    bench::add_scripts(app);
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use maki_engine::core::ConfigFile;
    use maki_engine::{audio, events, gameplay, platform, scene};

    use super::*;

    /// The rooms of the game: from the directory of this package, where a test runs.
    const ROOMS: [&str; 4] = [
        "../assets/rooms/room_a.ron",
        "../assets/rooms/room_b.ron",
        "../assets/rooms/room_c.ron",
        "../assets/rooms/room_d.ron",
    ];

    /// An app with the modules of the game that read a room file.
    fn game_app() -> App {
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
        app
    }

    /// The editor writes the room files of the game with all that they hold: the
    /// room, its objects, the events (also the commands of the audio module and of
    /// this game), and the comments. A load and a save give the same file. The
    /// loader finds nothing wrong in them.
    #[test]
    fn the_editor_writes_each_room_file_with_the_same_content() {
        let app = game_app();
        let registry = app.world().resource::<events::CommandRegistry>();
        let types = app.world().resource::<scene::ComponentTypes>();
        // The events and their pages: those of the rooms before the events were
        // components (stage 51).
        let (mut count, mut pages) = (0, 0);
        for path in ROOMS {
            let path = PathBuf::from(path);
            let validators = app.world().resource::<scene::RoomValidators>();
            scene::read_room(&path, validators).unwrap();
            let document = maki_editor::Document::read(&path, registry).unwrap();
            let text = document.text();
            // The formatter of the room files: the text of the editor in the file.
            if std::env::var_os("MAKI_WRITE_ROOMS").is_some() {
                std::fs::write(&path, &text).unwrap();
            }
            // Git on Windows can give the file CRLF line ends.
            let file = std::fs::read_to_string(&path)
                .unwrap()
                .replace("\r\n", "\n");
            assert_eq!(file, text, "{path:?}");
            assert_eq!(scene::parse_room(&text).unwrap(), document.room);
            let events = registry.parse_events(&text).unwrap();
            assert_eq!(events, document.events(registry));
            assert_eq!(maki_editor::read_notes(&text), document.notes);
            assert!(!document.notes.room.is_empty() && !document.notes.items.is_empty());
            count += events.len();
            pages += events.iter().map(|event| event.pages.len()).sum::<usize>();
            // The inspector reads the room, each object, and each component as a tree,
            // and reads a changed tree back. With no change: the same room, and the
            // same file.
            let objects = (0..document.room.objects.len()).map(maki_editor::Item::Object);
            let items: Vec<_> = std::iter::once(maki_editor::Item::Room)
                .chain(objects)
                .collect();
            let mut again = document.clone();
            for item in &items {
                let (_, tree) = again.tree(item).unwrap();
                again.set_tree(item, tree).unwrap();
            }
            for (index, object) in document.room.objects.iter().enumerate() {
                // The pages of an event have a panel of their own, not a tree.
                for name in object.components.keys().filter(|name| *name != "event") {
                    let tree = again.component_tree(index, name, types).unwrap().unwrap();
                    again.set_component_tree(index, name, &tree, types).unwrap();
                }
            }
            assert_eq!(again.room, document.room, "{path:?}");
            assert_eq!(again.text(), text, "{path:?}");
        }
        // Room D: the button (wires only) and the pad.
        assert_eq!((count, pages), (10, 11));
    }

    /// Room D: the button opens the door by a wire, with no code.
    #[test]
    fn the_button_of_room_d_opens_the_door_by_a_wire() {
        use std::time::Duration;

        use maki_engine::core::{Interpolated, Startup, Transform};
        use maki_engine::platform::{Key, RawInput};
        use maki_engine::scene::{Mover, MoverState, Player, RoomRequest, RoomTarget};

        let mut app = game_app();
        app.add_systems(
            Startup,
            |mut commands: bevy_ecs::system::Commands,
             mut request: bevy_ecs::system::ResMut<RoomRequest>| {
                commands.spawn((Player, Interpolated::at(Transform::IDENTITY)));
                request.0 = Some(RoomTarget {
                    path: "../assets/rooms/room_d.ron".into(),
                    spawn: Some("start".into()),
                    text: None,
                });
            },
        );
        app.run(|mut app| {
            let step = Duration::from_millis(20);
            let door = |app: &App| {
                let mut doors = app.world().try_query::<&Mover>().unwrap();
                doors.single(app.world()).unwrap().state
            };
            for _ in 0..3 {
                app.advance(step);
            }
            assert_eq!(door(&app), MoverState::AtA);
            // The player at the start faces the button and presses interact.
            app.write_message(RawInput::KeyDown(Key::Z))
                .write_message(RawInput::KeyUp(Key::Z));
            for _ in 0..200 {
                app.advance(step);
            }
            assert_eq!(door(&app), MoverState::AtB);
        });
    }

    /// A component of this game says what its fields are with the derive of the
    /// engine. The script crate names `maki-engine` only.
    #[test]
    fn a_type_of_the_script_crate_describes_its_fields() {
        use maki_engine::core::Fields;
        use maki_engine::core::fields::{Kind, read};

        /// A lamp that swings.
        #[derive(Debug, PartialEq, Fields, serde::Serialize, serde::Deserialize)]
        struct Swing {
            /// The largest angle, in degrees.
            #[field(range = 0.0..=90.0, step = 0.5)]
            angle: f32,
            #[field(label = "seconds for one swing", default = 2.0)]
            seconds: f32,
        }

        let Kind::Group(table) = Swing::kind() else {
            panic!("a struct is a group");
        };
        assert_eq!(table[0].help, "The largest angle, in degrees.");
        assert_eq!(table[1].label, "seconds for one swing");
        let swing = Swing {
            angle: 12.5,
            seconds: 2.0,
        };
        assert_eq!(read::<Swing>(swing.tree()).unwrap(), swing);
    }
}
