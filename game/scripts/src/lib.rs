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

    /// The rooms of the game: from the directory of this package, where a test runs.
    const ROOMS: [&str; 3] = [
        "../assets/rooms/room_a.ron",
        "../assets/rooms/room_b.ron",
        "../assets/rooms/room_c.ron",
    ];

    /// The migration of stage 48: the items of the old lists of each room file go
    /// into its list of objects. Run by hand: `cargo test -p maki-game-scripts -- --ignored items_to_objects`.
    // ponytail: goes with the old lists (stage 48, #5).
    #[test]
    #[ignore = "changes the room files of the game"]
    fn items_to_objects() {
        let app = game_app();
        let registry = app.world().resource::<events::CommandRegistry>();
        for path in ROOMS {
            let mut document = maki_editor::Document::read(path.as_ref(), registry).unwrap();
            document.items_to_objects();
            document.save().unwrap();
        }
    }

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
        let mut count = 0;
        for path in ROOMS {
            let path = PathBuf::from(path);
            let validators = app.world().resource::<scene::RoomValidators>();
            scene::read_room(&path, validators).unwrap();
            let document = maki_editor::Document::read(&path, registry).unwrap();
            let text = document.text();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), text, "{path:?}");
            assert_eq!(scene::parse_room(&text).unwrap(), document.room);
            assert_eq!(registry.parse_events(&text).unwrap(), document.events);
            assert_eq!(maki_editor::read_notes(&text), document.notes);
            assert!(!document.notes.room.is_empty() && !document.notes.items.is_empty());
            count += document.events.len();
            // The props and the lights are objects (stage 48).
            let room = &document.room;
            assert!(room.props.is_empty() && room.lights.is_empty(), "{path:?}");

            // The inspector reads each item and each component as a tree, and reads
            // a changed tree back. With no change: the same room, and the same file.
            let lists = [
                ("doorway", room.doorways.len()),
                ("door", room.doors.len()),
                ("object", room.objects.len()),
                ("mover", room.movers.len()),
                ("light", room.lights.len()),
                ("prefab", room.prefabs.len()),
            ];
            let mut items = vec![maki_editor::Item::Room];
            for (list, length) in lists {
                items.extend((0..length).map(|index| match list {
                    "doorway" => maki_editor::Item::Doorway(index),
                    "door" => maki_editor::Item::Door(index),
                    "object" => maki_editor::Item::Object(index),
                    "mover" => maki_editor::Item::Mover(index),
                    "light" => maki_editor::Item::Light(index),
                    _ => maki_editor::Item::Prefab(index),
                }));
            }
            let mut again = document.clone();
            for item in &items {
                let (_, tree) = again.tree(item).unwrap();
                again.set_tree(item, tree).unwrap();
            }
            for (index, object) in document.room.objects.iter().enumerate() {
                for name in object.components.keys() {
                    let tree = again.component_tree(index, name, types).unwrap().unwrap();
                    again.set_component_tree(index, name, &tree, types).unwrap();
                }
            }
            assert_eq!(again.room, document.room, "{path:?}");
            assert_eq!(again.text(), text, "{path:?}");
        }
        assert_eq!(count, 7);
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
