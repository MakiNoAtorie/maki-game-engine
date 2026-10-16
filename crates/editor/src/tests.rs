//! The editor with no window: a room is opened, changed, saved, played, and stopped.

use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy_ecs::query::With;
use glam::Vec3;
use maki_core::{App, ConfigFile, Interpolated, PlayState, Time};
use maki_events::{EventsModule, GameState};
use maki_gameplay::{CameraModule, ControlsModule, GameModule, PlayerModule, RoomsModule};
use maki_platform::{InputMode, PlatformModule, QuitRequested};
use maki_scene::{
    ActiveCamera, CurrentRoom, Light, MeshDef, MeshInstance, Player, RoomItem, SceneModule,
    parse_room,
};
use maki_ui::UiModule;

use super::*;

/// Room A: a lamp, a cube, and an event that runs when the room loads. It sets a
/// flag, turns the lamp off, and goes to room B.
const ROOM_A: &str = r#"// The test room.
(
    width: 6.0,
    depth: 6.0,
    height: 3.0,
    next_id: 5,
    objects: [
        // The cube.
        (id: 1, name: "cube", pose: (position: (1.0, 0.5, 0.0)), components: {"prop": (mesh: Cube(1.0))}),
        (
            id: 2,
            name: "lamp",
            pose: (position: (0.0, 2.0, 0.0)),
            components: {"light": (radius: 0.1, color: (1.0, 0.9, 0.8), intensity: 5.0)},
        ),
        (id: 3, name: "start", pose: (position: (0.0, 0.0, 2.0)), components: {"spawn": ()}),
        // Runs at the start.
        (
            id: 4,
            name: "leave",
            components: {
                "event": (
                    pages: [
                        (
                            trigger: Autorun,
                            commands: [SetFlag("left", true), LightOff("lamp"), Transfer("b.ron", "in")],
                        ),
                    ],
                ),
            },
        ),
    ],
)
"#;
/// The object of the event of room A.
const LEAVE: usize = 3;
const ROOM_B: &str =
    r#"(width: 4.0, depth: 4.0, height: 3.0, objects: [(name: "in", components: {"spawn": ()})])"#;

/// A directory with the two rooms, and an app with the modules of a game and the
/// editor.
fn editor_app(name: &str) -> (App, PathBuf) {
    session_app(name, None)
}

/// The same, with the session of an app before it.
fn session_app(name: &str, session: Option<Session>) -> (App, PathBuf) {
    let directory = std::env::temp_dir().join(format!("maki-editor-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("a.ron"), ROOM_A).unwrap();
    std::fs::write(directory.join("b.ron"), ROOM_B).unwrap();
    // No sound device in a test.
    let mut app = App::new(ConfigFile::from_text(
        "[audio]
output = false",
    ))
    .unwrap();
    app.add_module(PlatformModule)
        .add_module(maki_assets::AssetsModule)
        .add_module(SceneModule)
        .add_module(EventsModule)
        .add_module(maki_audio::AudioModule)
        .add_module(ControlsModule)
        .add_module(PlayerModule::default())
        .add_module(GameModule)
        .add_module(CameraModule)
        .add_module(RoomsModule {
            start_room: directory.join("a.ron"),
            start_spawn: "start".into(),
        })
        .add_module(UiModule)
        .add_module(EditorModule {
            rooms: directory.clone(),
            project: "test".to_owned(),
            session,
        });
    (app, directory)
}

fn frames(app: &mut App, count: usize) {
    for _ in 0..count {
        app.advance(Duration::from_millis(20));
    }
}

fn request(app: &mut App, command: Command) {
    app.resource_mut::<Editor>().request(command);
    frames(app, 2);
}

/// The lamp of the room, and how many entities show the player.
fn lamp(app: &App) -> (Light, Vec3, usize) {
    let world = app.world();
    let mut lights = world
        .try_query::<(&Light, &Interpolated, &RoomItem)>()
        .unwrap();
    let (light, pose, _) = lights.single(world).unwrap();
    let mut players = world
        .try_query_filtered::<&MeshInstance, With<Player>>()
        .unwrap();
    (
        *light,
        pose.current.translation,
        players.iter(world).count(),
    )
}

#[test]
fn a_room_is_opened_changed_and_saved_and_the_file_reads_as_the_document() {
    let (app, directory) = editor_app("save");
    app.run(|mut app| {
        frames(&mut app, 2);
        let world = app.world();
        let editor = world.resource::<Editor>();
        assert_eq!(editor.document.path, directory.join("a.ron"));
        assert!(!editor.document.dirty);
        // With no change, the text of the document is the text of the file: the
        // events and the comments are there.
        assert_eq!(editor.document.text(), ROOM_A);
        // The editor has the keys and the picture, the clock stands, no player shows.
        assert_eq!(*world.resource::<InputMode>(), MODE);
        assert!(world.resource::<Time>().paused);
        let mut active = world
            .try_query_filtered::<&EditorCamera, With<ActiveCamera>>()
            .unwrap();
        assert_eq!(active.iter(world).count(), 1);
        assert_eq!(lamp(&app).2, 0);
        // The event of the room does not run: the lamp is on.
        assert!(lamp(&app).0.on);
        let loads = world.resource::<CurrentRoom>().loads;

        // The property panel shows each item for some frames: a look is no change.
        for item in [
            Item::Object(1),
            Item::Object(0),
            Item::Object(2),
            Item::Object(LEAVE),
        ] {
            app.resource_mut::<Editor>().selection = vec![item];
            frames(&mut app, 3);
        }
        let editor = app.world().resource::<Editor>();
        assert_eq!(editor.document.text(), ROOM_A);
        assert_eq!(editor.history_steps(), (0, 0));

        // A light and the pose of a prop: the entities change, the room is not loaded.
        let mut editor = app.resource_mut::<Editor>();
        let mut light = editor.document.light(&Item::Object(1)).unwrap();
        (light.intensity, light.position) = (9.0, [1.0, 2.5, 0.0]);
        editor.document.set_light(&Item::Object(1), light);
        editor.document.room.objects[0].pose.position = [-1.0, 0.5, 0.0];
        editor.document.dirty = true;
        frames(&mut app, 2);
        let (light, at, _) = lamp(&app);
        assert_eq!((light.intensity, at), (9.0, Vec3::new(1.0, 2.5, 0.0)));
        assert_eq!(app.world().resource::<CurrentRoom>().loads, loads);
        assert_eq!(
            app.world().resource::<CurrentRoom>().def,
            app.world().resource::<Editor>().document.room
        );

        // Another mesh: the room is loaded again, from the document.
        {
            let cube = maki_scene::Prop {
                mesh: MeshDef::Cube(0.5),
                ..maki_scene::Prop::default()
            };
            let text = maki_scene::ComponentText::of(&cube);
            let mut editor = app.resource_mut::<Editor>();
            editor.document.room.objects[0]
                .components
                .insert("prop".to_owned(), text);
        }
        frames(&mut app, 2);
        assert_eq!(app.world().resource::<CurrentRoom>().loads, loads + 1);
        // The file did not change up to here.
        assert_eq!(
            std::fs::read_to_string(directory.join("a.ron")).unwrap(),
            ROOM_A
        );

        request(&mut app, Command::Save);
        let editor = app.world().resource::<Editor>();
        assert!(!editor.document.dirty, "{}", editor.status);
        let text = std::fs::read_to_string(directory.join("a.ron")).unwrap();
        assert_eq!(parse_room(&text).unwrap(), editor.document.room);
        assert!(text.contains("// The cube.") && text.contains("// Runs at the start."));
        assert!(text.contains(r#"Transfer("b.ron", "in")"#), "{text}");

        // A new room with the changes not saved: the editor asks, and "cancel"
        // keeps the document.
        app.resource_mut::<Editor>().document.dirty = true;
        request(&mut app, Command::New);
        assert_eq!(
            app.world().resource::<Editor>().pending(),
            Some(&Command::New)
        );
        request(&mut app, Command::Answer(Answer::Cancel));
        let editor = app.world().resource::<Editor>();
        assert_eq!(editor.document.path, directory.join("a.ron"));
        assert_eq!(editor.pending(), None);
        request(&mut app, Command::New);
        request(&mut app, Command::Answer(Answer::DoNotSave));
        let editor = app.world().resource::<Editor>();
        assert_eq!(editor.document.path, directory.join("untitled.ron"));
        // An empty room with its lamp, and the world shows it.
        let world = app.world();
        assert_eq!(world.try_query::<&Light>().unwrap().iter(world).count(), 1);
        assert_eq!(world.resource::<CurrentRoom>().def.objects.len(), 2);

        request(&mut app, Command::SaveAs(directory.join("c.ron")));
        request(&mut app, Command::Open(directory.join("c.ron")));
        let editor = app.world().resource::<Editor>();
        assert!(
            Path::new(&editor.document.path).ends_with("c.ron"),
            "{}",
            editor.status
        );
        assert_eq!(editor.document.room.width, 8.0);
    });
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn play_runs_the_game_and_stop_brings_back_the_room_and_the_game_state() {
    let (app, directory) = editor_app("play");
    app.run(|mut app| {
        frames(&mut app, 2);
        app.resource_mut::<GameState>().set_variable("tries", 3);
        let before = app.world().resource::<GameState>().clone();

        request(&mut app, Command::Play);
        assert_eq!(*app.world().resource::<InputMode>(), InputMode::GAME);
        assert!(!app.world().resource::<Time>().paused);
        // The event runs: the flag, and the game goes to room B.
        frames(&mut app, 20);
        let world = app.world();
        assert!(world.resource::<GameState>().flag("left"));
        assert_eq!(
            world.resource::<CurrentRoom>().path,
            directory.join("b.ron")
        );
        let mut players = world
            .try_query_filtered::<&MeshInstance, With<Player>>()
            .unwrap();
        assert_eq!(players.iter(world).count(), 1);

        // The quit action of the game, or the close button: a stop, not the end.
        // A pause of the game does not stay after the stop.
        app.resource_mut::<PlayState>().change(PlayState::Pause);
        app.resource_mut::<QuitRequested>().0 = true;
        frames(&mut app, 2);
        let world = app.world();
        assert_eq!(*world.resource::<PlayState>(), PlayState::Play);
        assert!(!world.resource::<QuitRequested>().0);
        let editor = world.resource::<Editor>();
        assert!(!editor.playing());
        assert_eq!(*world.resource::<GameState>(), before);
        assert_eq!(
            world.resource::<CurrentRoom>().path,
            directory.join("a.ron")
        );
        assert_eq!(editor.document.text(), ROOM_A);
        // The lamp is on again, and the event does not run in the editor.
        frames(&mut app, 20);
        let (light, _, players) = lamp(&app);
        assert!(light.on);
        assert_eq!(players, 0);
        assert!(!app.world().resource::<GameState>().flag("left"));

        // With no game, the quit request ends the app.
        app.resource_mut::<QuitRequested>().0 = true;
        frames(&mut app, 2);
        assert!(app.world().resource::<QuitRequested>().0);
        assert!(!app.world().resource::<maki_platform::QuitHold>().0);
    });
    std::fs::remove_dir_all(&directory).unwrap();
}

/// The pixel of a point of the room in the window, for the view of the editor.
fn window_pixel(app: &App, point: Vec3) -> [f32; 2] {
    let world = app.world();
    let mut cameras = world
        .try_query_filtered::<&maki_scene::Camera, With<EditorCamera>>()
        .unwrap();
    let picture = world.resource::<maki_platform::Picture>();
    let view = View::new(*cameras.single(world).unwrap(), picture.size());
    let [x, y] = view.project(point).unwrap();
    // The picture starts at the right of the left panel, under the top rows.
    [x + picture.left as f32, y + picture.top as f32]
}

/// A press and a release of the left button at a point of the room.
fn click(app: &mut App, point: Vec3) {
    use maki_platform::{Key, RawInput};
    let at = window_pixel(app, point);
    app.write_message(RawInput::PointerMoved(at));
    frames(app, 1);
    app.write_message(RawInput::KeyDown(Key::MouseLeft));
    frames(app, 1);
    app.write_message(RawInput::KeyUp(Key::MouseLeft));
    frames(app, 1);
}

#[test]
fn a_click_selects_what_the_picture_shows_and_a_drag_of_the_gizmo_is_one_undo_step() {
    use maki_platform::{Key, RawInput};

    let (app, directory) = editor_app("pick");
    app.run(|mut app| {
        frames(&mut app, 3);
        let selection = |app: &App| app.world().resource::<Editor>().selection.clone();
        let picture = *app.world().resource::<maki_platform::Picture>();
        assert!(picture.left > 0 && picture.top > 0);

        // The cube, then the spawn point (a mark, no mesh), then the floor.
        click(&mut app, Vec3::new(1.0, 1.0, 0.0));
        assert_eq!(selection(&app), [Item::Object(0)]);
        click(&mut app, Vec3::new(0.0, 0.1, 2.0));
        assert_eq!(selection(&app), [Item::Object(2)]);
        click(&mut app, Vec3::new(-2.0, 0.0, -2.0));
        assert_eq!(selection(&app), []);
        // With shift, a click adds; the lamp is a sphere of light.
        click(&mut app, Vec3::new(1.0, 1.0, 0.0));
        app.write_message(RawInput::KeyDown(Key::Shift));
        click(&mut app, Vec3::new(0.0, 2.0, 0.0));
        app.write_message(RawInput::KeyUp(Key::Shift));
        assert_eq!(selection(&app), [Item::Object(0), Item::Object(1)]);

        // The cube alone, and a drag of the square of its gizmo over the floor: 30
        // frames with the button down.
        click(&mut app, Vec3::new(-2.0, 0.0, -2.0));
        click(&mut app, Vec3::new(1.0, 1.0, 0.0));
        let loads = app.world().resource::<CurrentRoom>().loads;
        let (from, to) = (Vec3::new(1.0, 0.5, 0.0), Vec3::new(-1.3, 0.5, 1.6));
        app.write_message(RawInput::PointerMoved(window_pixel(&app, from)));
        frames(&mut app, 1);
        app.write_message(RawInput::KeyDown(Key::MouseLeft));
        frames(&mut app, 1);
        for step in 1..=30 {
            let at = window_pixel(&app, from.lerp(to, step as f32 / 30.0));
            app.write_message(RawInput::PointerMoved(at));
            frames(&mut app, 1);
        }
        app.write_message(RawInput::KeyUp(Key::MouseLeft));
        frames(&mut app, 2);
        // On the grid of 0.25 m, and the room was not loaded again.
        let cube = |app: &App| {
            app.world().resource::<Editor>().document.room.objects[0]
                .pose
                .position
        };
        assert_eq!(cube(&app), [-1.25, 0.5, 1.5]);
        assert_eq!(app.world().resource::<CurrentRoom>().loads, loads);
        assert_eq!(app.world().resource::<Editor>().history_steps(), (1, 0));

        // Undo and redo are chords. Ctrl+Z is not the plain Z, Ctrl+S is not a
        // tool key, and S alone saves nothing.
        let chord = |app: &mut App, key| {
            app.write_message(RawInput::KeyDown(Key::Control))
                .write_message(RawInput::KeyDown(key));
            frames(app, 1);
            app.write_message(RawInput::KeyUp(Key::Control))
                .write_message(RawInput::KeyUp(key));
            frames(app, 2);
        };
        chord(&mut app, Key::Z);
        assert_eq!(cube(&app), [1.0, 0.5, 0.0]);
        chord(&mut app, Key::Y);
        assert_eq!(cube(&app), [-1.25, 0.5, 1.5]);
        assert!(app.world().resource::<Editor>().document.dirty);
        app.write_message(RawInput::KeyDown(Key::S))
            .write_message(RawInput::KeyUp(Key::S));
        frames(&mut app, 2);
        assert!(app.world().resource::<Editor>().document.dirty);
        // Ctrl+R is no chord: the tool key works with Ctrl down.
        chord(&mut app, Key::R);
        assert_eq!(app.world().resource::<Editor>().tool, Tool::Scale);
        app.resource_mut::<Editor>().tool = Tool::Move;

        // A window of another size: the picture follows after some frames, and a
        // click still selects what the picture shows under the pointer.
        click(&mut app, Vec3::new(-2.0, 0.0, -2.0));
        let before = *app.world().resource::<maki_platform::Picture>();
        app.insert_resource(maki_platform::WindowSize {
            width: 1500,
            height: 1100,
        });
        frames(&mut app, 12);
        let picture = *app.world().resource::<maki_platform::Picture>();
        assert_ne!(
            (picture.width, picture.height),
            (before.width, before.height)
        );
        click(&mut app, Vec3::new(-1.25, 1.0, 1.5));
        assert_eq!(selection(&app), [Item::Object(0)]);
        click(&mut app, Vec3::new(0.0, 2.0, 0.0));
        assert_eq!(selection(&app), [Item::Object(1)]);

        // A press and a move in the same frame: the click is at the press place.
        click(&mut app, Vec3::new(-2.0, 0.0, -2.0));
        assert_eq!(selection(&app), []);
        let (cube_pixel, floor_pixel) = (
            window_pixel(&app, Vec3::new(-1.25, 1.0, 1.5)),
            window_pixel(&app, Vec3::new(-2.0, 0.0, -2.0)),
        );
        app.write_message(RawInput::PointerMoved(cube_pixel))
            .write_message(RawInput::KeyDown(Key::MouseLeft))
            .write_message(RawInput::KeyUp(Key::MouseLeft))
            .write_message(RawInput::PointerMoved(floor_pixel));
        frames(&mut app, 2);
        assert_eq!(selection(&app), [Item::Object(0)]);

        // A copy, then the copy is removed: the selection follows.
        request(&mut app, Command::Duplicate);
        assert_eq!(selection(&app), [Item::Object(4)]);
        request(&mut app, Command::Delete);
        assert_eq!(selection(&app), []);
        assert_eq!(
            app.world().resource::<Editor>().document.room.objects.len(),
            4
        );
        // The comment of the cube is still the comment of the cube.
        assert!(
            app.world()
                .resource::<Editor>()
                .document
                .text()
                .contains("// The cube.\n        (id: 1, name: \"cube\"")
        );
    });
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_mover_that_the_editor_adds_moves_in_the_game_when_an_event_opens_it() {
    use maki_events::{Builtin, EventPages, Page, Trigger};

    let (app, directory) = editor_app("mover");
    app.run(|mut app| {
        frames(&mut app, 2);
        request(&mut app, Command::New);
        request(&mut app, Command::Add(Kind::Mover));
        let mut editor = app.resource_mut::<Editor>();
        // The lamp and the spawn point of the new room are objects 0 and 1.
        assert_eq!(editor.selection, [Item::Object(2)]);
        let registry = editor.registry.clone();
        let command = |command: Builtin| registry.command(command).unwrap();
        let mut mover = editor.document.mover(&Item::Object(2)).unwrap();
        mover.seconds = 0.2;
        editor.document.room.objects[2].set_mover(mover);
        let lift = maki_scene::ObjectDef {
            name: Some("lift".to_owned()),
            ..Default::default()
        };
        editor.document.room.objects.push(lift);
        let pages = vec![Page {
            conditions: Vec::new(),
            trigger: Trigger::Autorun,
            commands: vec![
                command(Builtin::OpenDoor("mover".into())),
                command(Builtin::SetFlag("up".to_owned(), true)),
            ],
        }];
        let event = EventPages {
            pages,
            wires: Vec::new(),
        };
        editor.document.set_event(3, &event);
        let height = |app: &App| {
            let world = app.world();
            let mut movers = world
                .try_query::<(&maki_scene::Mover, &Interpolated)>()
                .unwrap();
            movers.single(world).unwrap().1.current.translation.y
        };
        frames(&mut app, 2);
        assert_eq!(height(&app), 0.5);

        request(&mut app, Command::Play);
        request(&mut app, Command::Answer(Answer::DoNotSave));
        frames(&mut app, 30);
        // At its pose `b`, one meter up; the event went on after the move.
        assert_eq!(height(&app), 1.5);
        assert!(app.world().resource::<GameState>().flag("up"));
        request(&mut app, Command::Stop);
        assert_eq!(height(&app), 0.5);
    });
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_test_of_a_page_plays_the_room_in_step_mode_and_stop_ends_it() {
    use maki_events::EventRunner;

    let (app, directory) = editor_app("test-run");
    app.run(|mut app| {
        frames(&mut app, 2);
        request(&mut app, Command::Test(maki_scene::ObjectId(4), 0));
        frames(&mut app, 10);
        // The game runs, and each event waits before its first command.
        let world = app.world();
        assert!(world.resource::<Editor>().playing());
        assert!(world.resource::<EventRunner>().step_mode());
        let waits = |app: &App| {
            let running = app.world().resource::<EventRunner>().running();
            running
                .iter()
                .filter_map(|event| event.current())
                .map(|c| format!("{c:?}"))
                .collect::<Vec<_>>()
        };
        // The page that the test started. It starts before the first step of the
        // room, so its own `Autorun` waits: an event blocks the player.
        assert_eq!(waits(&app), [r#"SetFlag("left", true)"#]);
        assert!(!app.world().resource::<GameState>().flag("left"));

        app.resource_mut::<EventRunner>().step_command();
        frames(&mut app, 3);
        assert!(app.world().resource::<GameState>().flag("left"));
        assert_eq!(waits(&app)[0], r#"LightOff("lamp")"#);
        assert!(lamp(&app).0.on);

        // Stop: no step mode, no flag, and a play after it runs with no steps.
        request(&mut app, Command::Stop);
        assert!(!app.world().resource::<EventRunner>().step_mode());
        assert!(!app.world().resource::<GameState>().flag("left"));
        request(&mut app, Command::Play);
        frames(&mut app, 20);
        assert_eq!(
            app.world().resource::<CurrentRoom>().path,
            directory.join("b.ron")
        );
    });
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_module_gives_the_widgets_for_the_arguments_of_its_command() {
    use maki_events::CommandRegistry;
    use maki_ui::egui;

    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    enum Sound {
        Play(String, f32),
    }

    let (mut app, directory) = editor_app("arguments");
    let registry = app.world().resource::<CommandRegistry>().clone();
    registry.add(|_: &Sound, _, _| true);
    // The widgets of the module: here they only make the sound louder.
    add_argument_editor(
        &mut app,
        |_ui, Sound::Play(_, volume): &mut Sound, _room| {
            *volume += 1.0;
            true
        },
    );
    let editors = app.world().resource::<ArgumentEditors>().clone();
    let sound = registry.parse_command(r#"Play("door", 0.5)"#).unwrap();
    let wait = registry.parse_command("Wait(1.0)").unwrap();
    let mut output = egui::Context::default().run_ui(egui::RawInput::default(), |ui| {
        let mut edit = |command| {
            editors
                .0
                .iter()
                .find_map(|edit| edit(ui, command, &registry, &maki_scene::RoomDef::default()))
        };
        // A command of another enum: not this editor. Its own: the changed command.
        assert_eq!(edit(&wait), None);
        let louder = registry.command(Sound::Play("door".to_owned(), 1.5));
        assert_eq!(edit(&sound), Some(louder));
    });
    // egui panics when texture changes are dropped unread.
    output.textures_delta.clear();
    std::fs::remove_dir_all(&directory).unwrap();
}

/// The traps of the UX pass: which list a command goes to, a file that "Save as"
/// would write over, play with changes, a drag that is cancelled, and the selection
/// of an undo.
#[test]
fn a_command_goes_to_the_list_of_the_last_click_and_no_work_is_lost_with_no_question() {
    use maki_platform::{Key, RawInput};

    use crate::{Row, RowKind};

    let (app, directory) = editor_app("traps");
    app.run(|mut app| {
        frames(&mut app, 3);
        fn editor(app: &App) -> &Editor {
            app.world().resource::<Editor>()
        }
        let cube = |app: &App| editor(app).document.room.objects[0].pose.position;
        // The first page of the one event of the room.
        fn page(app: &App) -> Vec<maki_events::Command> {
            let editor = editor(app);
            let events = editor.document.events(&editor.registry);
            events[0].pages[0].commands.clone()
        }
        let commands = |app: &App| page(app).len();
        let row = |index| Row {
            list: Vec::new(),
            index,
            kind: RowKind::Command,
        };

        // Esc in a drag: the values of its start, and no step of the history.
        click(&mut app, Vec3::new(1.0, 1.0, 0.0));
        assert_eq!(editor(&app).selection, [Item::Object(0)]);
        let steps = editor(&app).history_steps();
        let (from, to) = (Vec3::new(1.0, 0.5, 0.0), Vec3::new(-1.0, 0.5, 1.0));
        app.write_message(RawInput::PointerMoved(window_pixel(&app, from)));
        frames(&mut app, 1);
        app.write_message(RawInput::KeyDown(Key::MouseLeft));
        frames(&mut app, 1);
        app.write_message(RawInput::PointerMoved(window_pixel(&app, to)));
        frames(&mut app, 2);
        assert_ne!(cube(&app), [1.0, 0.5, 0.0]);
        request(&mut app, Command::Cancel);
        assert_eq!(cube(&app), [1.0, 0.5, 0.0]);
        assert!(editor(&app).dragged().is_none());
        app.write_message(RawInput::KeyUp(Key::MouseLeft));
        frames(&mut app, 2);
        assert_eq!(editor(&app).history_steps(), steps);
        assert_eq!(cube(&app), [1.0, 0.5, 0.0]);
        // Esc with no drag: nothing is selected.
        request(&mut app, Command::Cancel);
        assert_eq!(editor(&app).selection, []);

        // An event, and a click on its second command row: Delete removes the row,
        // not the event.
        {
            let mut editor = app.resource_mut::<Editor>();
            editor.select(Item::Object(LEAVE), false);
            // The panel shows the first page of the event, then a click on a row.
            editor.event_ui.go_to(LEAVE, 0, None);
            editor.event_ui.select_rows(row(1), None);
        }
        request(&mut app, Command::Delete);
        let objects = editor(&app).document.room.objects.len();
        assert_eq!((objects, commands(&app)), (4, 2));
        // A range of two rows (Shift and a click): copy, paste after them, and a
        // copy of the pasted rows.
        app.resource_mut::<Editor>()
            .event_ui
            .select_rows(row(1), Some(0));
        request(&mut app, Command::Copy);
        request(&mut app, Command::Paste);
        assert_eq!(commands(&app), 4);
        assert_eq!(editor(&app).event_ui.selected(), Some((Vec::new(), 2..=3)));
        request(&mut app, Command::Duplicate);
        assert_eq!(commands(&app), 6);
        request(&mut app, Command::Delete);
        assert_eq!(commands(&app), 4);
        let list = &page(&app);
        assert_eq!((&list[0], &list[1]), (&list[2], &list[3]));
        // Undo is of the document: the rows that were deleted are back.
        request(&mut app, Command::Undo);
        assert_eq!(commands(&app), 6);

        // A click on an item: the same key deletes the item, and the rows stay. The
        // undo of it selects the item again.
        app.resource_mut::<Editor>().select(Item::Object(0), false);
        // The key comes in a later frame than the click.
        frames(&mut app, 1);
        request(&mut app, Command::Delete);
        assert_eq!(editor(&app).document.room.objects.len(), 3);
        assert_eq!((commands(&app), editor(&app).selection.len()), (6, 0));
        request(&mut app, Command::Undo);
        assert_eq!(editor(&app).selection, [Item::Object(0)]);
        assert_eq!(editor(&app).document.room.objects.len(), 4);

        // Play with changes: no question. The game goes to room B (the event of
        // room A), and a load of room A gets the room of the editor, not the file.
        app.resource_mut::<Editor>().document.room.width = 7.0;
        assert!(editor(&app).document.dirty);
        request(&mut app, Command::Play);
        assert!(editor(&app).playing() && editor(&app).pending().is_none());
        frames(&mut app, 20);
        assert_eq!(
            app.world().resource::<CurrentRoom>().path,
            directory.join("b.ron")
        );
        app.resource_mut::<maki_scene::RoomRequest>().0 = Some(maki_scene::RoomTarget {
            path: directory.join("a.ron"),
            spawn: Some("start".into()),
            text: None,
        });
        frames(&mut app, 1);
        let room = app.world().resource::<CurrentRoom>();
        assert_eq!(
            (room.path.clone(), room.def.width),
            (directory.join("a.ron"), 7.0)
        );
        request(&mut app, Command::Stop);
        assert!(editor(&app).document.dirty);
        assert_eq!(editor(&app).document.room.width, 7.0);
        // After the stop, the file is the file again.
        assert_eq!(app.world().resource::<maki_scene::RoomOverride>().0, None);

        // "Save as" with the name of room B: a question, and the file stays until
        // the answer is "Replace".
        let room_b = directory.join("b.ron");
        request(&mut app, Command::SaveAs(room_b.clone()));
        assert_eq!(
            editor(&app).pending(),
            Some(&Command::SaveAs(room_b.clone()))
        );
        assert_eq!(std::fs::read_to_string(&room_b).unwrap(), ROOM_B);
        request(&mut app, Command::Answer(Answer::Cancel));
        assert_eq!(editor(&app).document.path, directory.join("a.ron"));
        assert_eq!(std::fs::read_to_string(&room_b).unwrap(), ROOM_B);
        request(&mut app, Command::SaveAs(room_b.clone()));
        request(&mut app, Command::Answer(Answer::Replace));
        assert_eq!(editor(&app).document.path, room_b);
        assert!(
            std::fs::read_to_string(&room_b)
                .unwrap()
                .contains("width: 7.0")
        );
        assert!(!editor(&app).document.dirty);
    });
    std::fs::remove_dir_all(&directory).unwrap();
}

/// The audio module in the editor: the sound of a light is a part of the room, and
/// the audio commands are made and changed with no text field.
#[test]
fn a_sound_command_is_added_with_arguments_and_a_rename_of_its_light_follows() {
    use maki_audio::{AudioCommand, AudioModule};
    use maki_events::CommandRegistry;
    use maki_ui::egui;

    // No sound device in a test.
    let mut app = App::new(ConfigFile::from_text("[audio]\noutput = false")).unwrap();
    app.add_module(PlatformModule)
        .add_module(SceneModule)
        .add_module(EventsModule)
        .add_module(AudioModule);
    crate::audio::add_argument_editors(&mut app);
    let registry = app.world().resource::<CommandRegistry>().clone();
    let editors = app.world().resource::<ArgumentEditors>().clone();
    let text = r#"(width: 6.0, depth: 6.0, height: 3.0,
        objects: [
            (name: "dark", pose: (position: (2.0, 2.0, 0.0)), components: {"light": (radius: 0.1, color: (1.0, 1.0, 1.0), intensity: 5.0)}),
            (name: "lamp", pose: (position: (0.0, 2.0, 0.0)), components: {"light": (radius: 0.1, color: (1.0, 1.0, 1.0), intensity: 5.0, sound: (file: "hum.wav", volume: 0.5))}),
            (name: "power", components: {"event": (pages: [(trigger: Interact, commands: [
                SoundOff("lamp"), If([Flag("loud")], [SoundOn("lamp")], [PlaySound("click.wav", 1.0)]),
            ])])}),
        ])"#;
    let mut document = Document::parse(Path::new("room.ron"), text, &registry).unwrap();
    let room = document.room.clone();

    // The "Add" list: a new command names the light that has a sound.
    let new = |name: &str| registry.new_command(name, &room);
    let lamp = || "lamp".to_owned();
    assert_eq!(
        new("SoundOn"),
        registry.command(AudioCommand::SoundOn(lamp().into()))
    );
    assert_eq!(
        new("PlaySound"),
        registry.parse_command(r#"PlaySound("", 1.0)"#).ok()
    );
    assert_eq!(new("Wait"), None);
    // The arguments of each audio command have widgets. With no input, no change.
    let mut output = egui::Context::default().run_ui(egui::RawInput::default(), |ui| {
        for name in ["PlaySound", "PlayMusic", "StopMusic", "SoundOn", "SoundOff"] {
            let command = new(name).unwrap();
            let mut edits = editors.0.iter();
            let edited = edits.find_map(|edit| edit(ui, &command, &registry, &room));
            assert_eq!(edited, Some(None), "{name}");
        }
    });
    // egui panics when texture changes are dropped unread.
    output.textures_delta.clear();

    // What the loader finds, on the rows: the file of the click is not there.
    let found = problems(&document, &[], &registry);
    let texts: Vec<&str> = found.iter().map(|problem| problem.text.as_str()).collect();
    assert_eq!(texts, ["no sound file `click.wav`"]);
    assert!(found[0].warning);

    // A rename of the light: the two sound commands follow, also the one in a list.
    document.room.objects[1].name = Some("bulb".to_owned());
    let status = document.renamed(&Item::Object(1), Some(lamp()), &registry);
    assert!(status.contains("2 event commands"), "{status}");
    let text = document.text();
    assert!(text.contains(r#"SoundOff("bulb")"#) && text.contains(r#"SoundOn("bulb")"#));
    assert!(
        problems(&document, &[], &registry)
            .iter()
            .all(|problem| problem.warning)
    );
    // The saved text is the same room, with the sound of the light.
    assert_eq!(parse_room(&text).unwrap(), document.room);
    assert!(
        text.contains(r#"sound:(file:"hum.wav",volume:0.5)"#)
            || text.contains(r#"sound: (file: "hum.wav", volume: 0.5)"#),
        "{text}"
    );
}

/// A WAV file of half a second of silence: one channel, 16 bits, 8000 Hz.
fn write_wav(path: &Path) {
    let samples = 4000_u32;
    let mut bytes = Vec::new();
    let parts: [&[u8]; 10] = [
        b"RIFF",
        &(36 + samples * 2).to_le_bytes(),
        b"WAVEfmt ",
        &16_u32.to_le_bytes(),
        // PCM, one channel.
        &[1, 0, 1, 0],
        &8000_u32.to_le_bytes(),
        &16000_u32.to_le_bytes(),
        // Bytes and bits in a sample.
        &[2, 0, 16, 0],
        b"data",
        &(samples * 2).to_le_bytes(),
    ];
    for part in parts {
        bytes.extend(part);
    }
    bytes.resize(bytes.len() + samples as usize * 2, 0);
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn stop_ends_all_sound_and_a_room_that_the_editor_shows_is_silent() {
    use maki_audio::Audio;
    use maki_platform::{Key, RawInput};

    let (app, directory) = editor_app("sound");
    write_wav(&directory.join("hum.wav"));
    // A lamp that hums, and an event that starts music and a sound.
    let room = r#"(width: 6.0, depth: 6.0, height: 3.0,
        objects: [
            (name: "lamp", pose: (position: (0.0, 2.0, 0.0)), components: {"light": (radius: 0.1, color: (1.0, 1.0, 1.0), intensity: 5.0, sound: (file: "hum.wav"))}),
            (name: "start", pose: (position: (0.0, 0.0, 2.0)), components: {"spawn": ()}),
            (name: "music", components: {"event": (pages: [(trigger: Autorun, commands: [
                PlayMusic("hum.wav", 1.0), PlaySound("hum.wav", 1.0)])])}),
        ])"#;
    std::fs::write(directory.join("c.ron"), room).unwrap();
    // Music, the sounds that play one time, the sounds at a place.
    let playing = |app: &App| {
        let audio = app.world().resource::<Audio>();
        let (effects, sources) = (audio.effects().len(), audio.sources().len());
        (audio.music().is_some(), effects, sources)
    };
    app.run(|mut app| {
        frames(&mut app, 2);
        request(&mut app, Command::Open(directory.join("c.ron")));
        frames(&mut app, 5);
        assert_eq!(playing(&app), (false, 0, 0));

        request(&mut app, Command::Play);
        frames(&mut app, 5);
        assert_eq!(playing(&app), (true, 1, 1));

        // The play key, held for some frames as a hand holds it: a stop, and no
        // new game after it.
        app.write_message(RawInput::KeyDown(Key::F5));
        frames(&mut app, 6);
        app.write_message(RawInput::KeyUp(Key::F5));
        frames(&mut app, 2);
        assert!(!app.world().resource::<Editor>().playing());
        assert_eq!(playing(&app), (false, 0, 0));
        // The room is shown again, with its lamp.
        frames(&mut app, 5);
        assert_eq!(lamp(&app).2, 0);
        assert_eq!(playing(&app), (false, 0, 0));
    });
    std::fs::remove_dir_all(&directory).unwrap();
}

/// A glTF file with one triangle: the corners (0, 0, 0), (1, 0, 0), (0, 1, 0).
const TRIANGLE: &str = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0}],
"meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],
"buffers":[{"uri":"data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAgD8AAAAA","byteLength":36}],
"bufferViews":[{"buffer":0,"byteLength":36}],
"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}]}"#;

#[test]
fn a_click_on_a_model_from_a_file_selects_its_prop() {
    let (app, directory) = editor_app("model");
    std::fs::write(directory.join("triangle.gltf"), TRIANGLE).unwrap();
    let room = r#"(width: 6.0, depth: 6.0, height: 3.0, objects: [
        (pose: (position: (2.0, 0.5, 0.0)), components: {"prop": (mesh: Cube(1.0))}),
        (pose: (position: (-1.0, 0.0, 0.0)), components: {"prop": (mesh: File("triangle.gltf"))}),
    ])"#;
    std::fs::write(directory.join("m.ron"), room).unwrap();
    app.run(|mut app| {
        frames(&mut app, 3);
        request(&mut app, Command::Open(directory.join("m.ron")));
        // The file is read on a thread.
        for _ in 0..500 {
            frames(&mut app, 1);
            if app.world().resource::<maki_assets::ModelLoads>().reading() == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        frames(&mut app, 2);
        // A point of the triangle, then the floor.
        click(&mut app, Vec3::new(-0.75, 0.25, 0.0));
        let selection = |app: &App| app.world().resource::<Editor>().selection.clone();
        assert_eq!(selection(&app), [Item::Object(1)]);
        click(&mut app, Vec3::new(-2.0, 0.0, 2.0));
        assert_eq!(selection(&app), []);
    });
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn items_go_to_another_room_and_a_prefab_is_made_unpacked_and_changed() {
    use maki_scene::{ObjectId, PoseDef, RoomItem, read_prefab};

    let (app, directory) = editor_app("prefab");
    app.run(|mut app| {
        frames(&mut app, 3);
        let select = |app: &mut App, items: &[Item]| {
            let mut editor = app.resource_mut::<Editor>();
            editor.selection = items.to_vec();
            // Not the command rows of an event.
            editor.event_ui.focused = false;
        };
        let editor = |app: &App| app.world().resource::<Editor>().clone_parts();
        // The cube and the lamp of room A go into room B by the buffer. The lamp of
        // room B is a second lamp: the name gets a number.
        request(&mut app, Command::Add(Kind::Light));
        select(&mut app, &[Item::Object(0), Item::Object(1)]);
        request(&mut app, Command::Copy);
        request(&mut app, Command::Save);
        request(&mut app, Command::Open(directory.join("b.ron")));
        request(&mut app, Command::Add(Kind::Light));
        app.resource_mut::<Editor>().document.room.objects[1].name = Some("lamp".to_owned());
        request(&mut app, Command::Paste);
        let (room, selection) = editor(&app);
        assert_eq!(selection, [Item::Object(2), Item::Object(3)]);
        assert_eq!(room.objects[2].pose.position, [1.0, 0.5, 0.0]);
        assert_eq!(room.objects[3].name.as_deref(), Some("lamp_2"));

        // A second cube (and lamp), 1 m beside the first, and the two cubes as a
        // prefab: its origin is the floor point of the first.
        request(&mut app, Command::Duplicate);
        app.resource_mut::<Editor>().document.room.objects[4]
            .pose
            .position = [2.0, 0.5, 0.0];
        select(&mut app, &[Item::Object(2), Item::Object(4)]);
        request(&mut app, Command::MakePrefab("pair".to_owned()));
        let (room, selection) = editor(&app);
        // The prefab is an object behind the spawn point and the three lamps.
        assert_eq!(selection, [Item::Object(4)]);
        assert!(room.objects.iter().all(|object| object.prop().is_none()));
        assert_eq!(room.objects.len(), 5);
        assert_eq!(room.objects[4].pose.position, [1.0, 0.0, 0.0]);
        let file = directory.join("prefabs/pair.prefab");
        let places = |file: &Path| -> Vec<[f32; 3]> {
            let objects = read_prefab(file).unwrap().objects;
            objects.iter().map(|object| object.pose.position).collect()
        };
        assert_eq!(places(&file), [[0.0, 0.5, 0.0], [1.0, 0.5, 0.0]]);
        // The objects of the file have ids of the file.
        let ids = read_prefab(&file)
            .unwrap()
            .objects
            .iter()
            .map(|o| o.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, [Some(ObjectId(1)), Some(ObjectId(2))]);
        // The world shows the two cubes, and a click on one selects the prefab.
        frames(&mut app, 2);
        let world = app.world();
        let mut shown = world.try_query::<&RoomItem>().unwrap();
        let of_prefab = |item: &&RoomItem| **item == RoomItem(4);
        assert_eq!(shown.iter(world).filter(of_prefab).count(), 2);
        select(&mut app, &[]);
        click(&mut app, Vec3::new(1.0, 1.0, 0.0));
        assert_eq!(editor(&app).1, [Item::Object(4)]);

        // The prefab is turned and moved, and unpacked: the cubes are props again,
        // at the places that the prefab showed them at.
        app.resource_mut::<Editor>().document.room.objects[4].pose = PoseDef {
            position: [0.0, 0.0, -1.0],
            rotation: [0.0, 90.0, 0.0],
            ..PoseDef::default()
        };
        request(&mut app, Command::UnpackPrefab);
        let (room, selection) = editor(&app);
        assert_eq!(selection, [Item::Object(4), Item::Object(5)]);
        assert!(room.objects.iter().all(|object| object.prefab().is_none()));
        // A quarter turn around Y moves +X to -Z.
        assert_eq!(room.objects[5].pose.position, [0.0, 0.5, -2.0]);
        assert_eq!(room.objects[5].pose.rotation, [0.0, 90.0, 0.0]);

        // One cube goes up, and the prefab is made again with its name: the file has
        // the change in the space of the prefab, and the prefab has its pose again.
        app.resource_mut::<Editor>().document.room.objects[5]
            .pose
            .position[1] = 1.5;
        assert_eq!(app.world().resource::<Editor>().prefab_name, "pair");
        request(&mut app, Command::MakePrefab("pair".to_owned()));
        assert_eq!(places(&file), [[0.0, 0.5, 0.0], [1.0, 1.5, 0.0]]);
        let (room, _) = editor(&app);
        assert_eq!(room.objects[4].pose.rotation, [0.0, 90.0, 0.0]);
        assert_eq!(room.objects[4].pose.position, [0.0, 0.0, -1.0]);

        // The room is saved with its prefab and reads as the document.
        request(&mut app, Command::Save);
        let text = std::fs::read_to_string(directory.join("b.ron")).unwrap();
        assert!(text.contains(r#"file: "prefabs/pair.prefab""#), "{text}");
        assert_eq!(maki_scene::parse_room(&text).unwrap(), room);
    });
    std::fs::remove_dir_all(&directory).unwrap();
}

/// A command of a menu or the command search reads `Panels`, which `run_ui` holds out
/// of the world during the frame.
#[test]
fn a_command_that_reads_the_panels_runs_from_the_palette_and_from_a_menu() {
    use bevy_ecs::resource::Resource;
    #[derive(Resource, Default)]
    struct Seen(u32);
    /// 1: start from the palette, 2: from a menu. Taken by `drive`.
    #[derive(Resource, Default)]
    struct Press(u32);
    // As a click in the egui frame: `Panels` is out of the world.
    fn drive(world: &mut World) {
        let press = std::mem::take(&mut world.resource_mut::<Press>().0);
        world.resource_scope(|world, mut panels: Mut<Panels>| match press {
            1 => {
                let row = panels.search(world, "reads panels").remove(0);
                panels.run_row(world, &row);
            }
            2 => maki_ui::queue_command(world, "test.reads_panels"),
            _ => {}
        });
    }
    let (mut app, directory) = editor_app("panel_commands");
    app.init_resource::<Seen>().init_resource::<Press>();
    app.add_systems(Update, drive.ambiguous_with_all());
    let read = |world: &mut World| {
        // Panics if `Panels` is not in the world.
        assert!(world.resource::<Panels>().is_open(panels::OUTLINER));
        world.resource_mut::<Seen>().0 += 1;
    };
    maki_ui::add_command(
        &mut app,
        maki_ui::ToolCommand {
            menu: "File",
            ..maki_ui::ToolCommand::new("test.reads_panels", "Reads panels", read)
        },
    );
    app.run(|mut app| {
        frames(&mut app, 2);
        for press in [1, 2] {
            app.resource_mut::<Press>().0 = press;
            frames(&mut app, 2);
            assert_eq!(app.world().resource::<Seen>().0, press);
        }
    });
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn the_shell_has_the_commands_and_the_panels_of_the_editor() {
    let (app, directory) = editor_app("shell");
    app.run(|mut app| {
        frames(&mut app, 2);
        let world = app.world();
        let panels = world.resource::<Panels>();
        // The panels are tabs of their docks, and the layouts have their names.
        for panel in [panels::OUTLINER, panels::INSPECTOR, panels::EVENT] {
            assert_eq!(panels.tabs_with(panel), [panel]);
        }
        let (layouts, current) = panels.layouts();
        assert_eq!(layouts, ["Build", "Logic", "Look", "Animate", "Debug"]);
        assert_eq!(current, "Build");
        // The command search has a command with its menu and its key, and knows
        // that a command cannot run now.
        let row = |query: &str| panels.search(world, query).remove(0);
        let save = row("save");
        assert_eq!(
            (save.text.as_str(), save.key.as_str()),
            ("File: Save", "Ctrl+S")
        );
        assert!(save.can_run && !row("undo").can_run);
        assert_eq!(row("add cuboid").text, "Add: Cuboid");

        // The key of "Rename": the row of the lamp in the outliner is a text field
        // in the next frame. Its end gives the name, and the event follows.
        app.resource_mut::<Editor>().select(Item::Object(1), false);
        app.write_message(maki_platform::RawInput::KeyDown(Key::F2));
        app.advance(Duration::from_millis(20));
        app.write_message(maki_platform::RawInput::KeyUp(Key::F2));
        app.resource_mut::<Editor>()
            .rename(&Item::Object(1), "bulb".to_owned());
        frames(&mut app, 2);
        let editor = app.world().resource::<Editor>();
        assert_eq!(
            editor.document.room.objects[1].name.as_deref(),
            Some("bulb")
        );
        assert!(editor.document.text().contains(r#"LightOff("bulb")"#));
        assert!(editor.document.dirty);

        // The layout "Look" closes the event panel; an event that is selected
        // shows it again.
        app.resource_mut::<Panels>().apply_layout("Look");
        frames(&mut app, 2);
        assert!(!app.world().resource::<Panels>().is_open(panels::EVENT));
        app.resource_mut::<Editor>()
            .select(Item::Object(LEAVE), false);
        frames(&mut app, 2);
        assert!(app.world().resource::<Panels>().is_open(panels::EVENT));

        // A command that fails is a notice, not a window that must be answered.
        request(&mut app, Command::Answer(Answer::DoNotSave));
        let missing = directory.join("none.ron");
        app.resource_mut::<Editor>().document.dirty = false;
        request(&mut app, Command::Open(missing));
        let notices = app.world().resource::<maki_ui::Notices>();
        let texts: Vec<&str> = notices.texts().collect();
        assert!(
            texts.len() == 1 && texts[0].contains("none.ron"),
            "{texts:?}"
        );
        assert!(app.world().resource::<Editor>().pending().is_none());
    });
}

#[test]
fn the_document_of_a_session_goes_to_the_next_editor_app() {
    // The first app: a change that is not saved, another spawn point for "Play",
    // and another view.
    let (app, directory) = editor_app("session");
    let (session, room) = app.run(|mut app| {
        frames(&mut app, 2);
        let mut editor = app.resource_mut::<Editor>();
        let mut light = editor.document.light(&Item::Object(1)).unwrap();
        light.intensity = 2.5;
        editor.document.set_light(&Item::Object(1), light);
        editor.document.dirty = true;
        editor.play_from = Some("other".to_owned());
        editor.select(Item::Object(LEAVE), false);
        frames(&mut app, 2);
        let world = app.world();
        let mut cameras = world.try_query::<&EditorCamera>().unwrap();
        assert!(cameras.iter(world).next().is_some());
        let room = world.resource::<Editor>().document.room.clone();
        (session(world).unwrap(), room)
    });
    assert!(session.dirty && session.view.is_some());
    assert_eq!(session.path, directory.join("a.ron"));

    // The next app starts with that document, not with the file: the change is
    // there, and it is still not saved.
    let (app, _) = session_app("session", Some(session.clone()));
    app.run(|mut app| {
        frames(&mut app, 2);
        let world = app.world();
        let editor = world.resource::<Editor>();
        assert_eq!(editor.document.room, room);
        assert_eq!(
            editor.document.light(&Item::Object(1)).unwrap().intensity,
            2.5
        );
        assert!(editor.document.dirty);
        assert_eq!(editor.document.path, directory.join("a.ron"));
        assert_eq!(editor.play_from.as_deref(), Some("other"));
        assert_eq!(editor.selection, [Item::Object(LEAVE)]);
        // The same session comes out of it.
        assert_eq!(super::session(world).unwrap(), session);
    });
}

/// The rows of each item kind come from the table of its fields, and what the
/// inspector reads back is the item.
#[test]
fn the_rows_of_an_item_come_from_the_table_of_its_fields() {
    use maki_assets::{Doorway, Material, Wall};
    use maki_core::FieldTypes;
    use maki_core::fields::{Kind, Value};
    use maki_scene::{
        ComponentTypes, DoorLeaf, LookDef, MoverDef, ObjectDef, PoseDef, PrefabUse, PropDef,
    };
    use maki_ui::egui;

    let mut app = App::new(ConfigFile::default()).unwrap();
    app.add_module(SceneModule);
    let types = app.world().resource::<FieldTypes>();
    let components = app.world().resource::<ComponentTypes>();

    // A room with an object of each kind, and with numbers that are not exact in
    // binary: the lamp and the spawn point of a new room (0, 1), a door (2), a
    // crate (3), a mover (4), a prefab (5).
    let mut document = Document::new_room(PathBuf::from("room.ron"));
    let room = &mut document.room;
    let mut door = ObjectDef {
        name: Some("door".to_owned()),
        ..ObjectDef::default()
    };
    let doorway = Doorway {
        wall: Wall::NegX,
        offset: 0.3,
        width: 1.0,
        height: 2.0,
        depth: 1.0,
    };
    door.set_component("doorway", &doorway);
    let leaf = DoorLeaf {
        seconds: 1.2,
        color: [0.35, 0.2, 0.1],
        open: false,
    };
    door.set_component("door", &leaf);
    room.objects.push(door);
    let look = LookDef {
        color: Some([0.15, 0.3, 0.45]),
        material: Some(Material::Metal { roughness: 0.3 }),
        ..LookDef::default()
    };
    let crate_prop = PropDef {
        name: Some("crate".to_owned()),
        look: look.clone(),
        ..PropDef::default()
    };
    room.objects.push(crate_prop.into_object());
    let mover = MoverDef {
        name: None,
        mesh: MeshDef::Cuboid([-0.5, 0.0, -0.5], [0.5, 1.0, 0.5]),
        look,
        a: PoseDef::default(),
        b: PoseDef {
            position: [0.0, 1.1, 0.0],
            ..PoseDef::default()
        },
        seconds: 2.0,
    };
    room.objects.push(mover.into_object());
    let prefab = PrefabUse {
        name: None,
        file: "table.prefab".into(),
        pose: PoseDef::default(),
        changes: Default::default(),
    };
    room.objects.push(prefab.into_object());
    let mut items = vec![Item::Room];
    items.extend((0..document.room.objects.len()).map(Item::Object));

    // A frame with no input: the rows of each item and of each component are drawn,
    // and no value changes.
    let before = document.clone();
    let lists = |_: &str| Vec::new();
    let mut output = egui::Context::default().run_ui(egui::RawInput::default(), |ui| {
        for item in &items {
            let (name, mut tree) = document.tree(item).unwrap();
            let shown = tree.clone();
            let kind = &types.get(name).unwrap().kind;
            assert!(
                !crate::fields::show(ui, kind, &mut tree, &lists),
                "{item:?}"
            );
            assert_eq!(tree, shown, "{item:?}");
            document.set_tree(item, tree).unwrap();
        }
        for (index, object) in before.room.objects.iter().enumerate() {
            for name in object.components.keys() {
                let tree = document.component_tree(index, name, components);
                let mut tree = tree.unwrap().unwrap();
                let shown = tree.clone();
                let kind = &types.get(name).unwrap().kind;
                assert!(!crate::fields::show(ui, kind, &mut tree, &lists), "{name}");
                assert_eq!(tree, shown, "{name}");
                document
                    .set_component_tree(index, name, &tree, components)
                    .unwrap();
            }
        }
    });
    // egui panics when texture changes are dropped unread.
    output.textures_delta.clear();
    // The tree of each item, read back: the same document, and the same file.
    assert_eq!(document, before);
    assert_eq!(document.text(), before.text());

    // A changed tree changes its object, and no other: the mesh of the crate, by the
    // variant that the list of the mesh gives for "Sphere", and its place.
    let mut tree = document
        .component_tree(3, "prop", components)
        .unwrap()
        .unwrap();
    let Kind::Group(table) = &types.get("prop").unwrap().kind else {
        panic!("a prop is a struct");
    };
    let mesh = table.iter().find(|field| field.name == "mesh").unwrap();
    let Kind::Choice(variants) = &mesh.kind else {
        panic!("a mesh is an enum");
    };
    tree["mesh"] = variants[2].zero();
    document
        .set_component_tree(3, "prop", &tree, components)
        .unwrap();
    let (_, mut rows) = document.tree(&Item::Object(3)).unwrap();
    rows["pose"]["position"] = Value::from(vec![1.0, 2.0, 3.0]);
    document.set_tree(&Item::Object(3), rows).unwrap();
    let prop = document.room.objects[3].prop().unwrap().unwrap();
    assert_eq!(prop.pose.position, [1.0, 2.0, 3.0]);
    let sphere = MeshDef::Sphere {
        radius: 0.5,
        segments: 32,
        rings: 16,
    };
    assert_eq!((&prop.mesh, prop.name.as_deref()), (&sphere, Some("crate")));
    assert_eq!(document.room.objects[2], before.room.objects[2]);

    // A tree that is not one of the item, an item that is not there, an item with
    // rows of its own: an error or `None`, and the document stays.
    let was = document.clone();
    assert!(document.set_tree(&Item::Object(0), Value::Null).is_err());
    assert!(document.set_tree(&Item::Object(0), tree.clone()).is_err());
    assert!(document.set_tree(&Item::Object(9), tree).is_err());
    assert!(document.tree(&Item::Object(9)).is_none());
    let null = Value::Null;
    assert!(
        document
            .set_component_tree(3, "prop", &null, components)
            .is_err()
    );
    assert!(document.component_tree(3, "swing", components).is_none());
    assert_eq!(document, was);
}

/// An object shows its own rows and a group for each component. A component with
/// no type keeps its text; one with a type goes to its tree and back.
#[test]
fn an_object_shows_each_component_and_keeps_one_with_no_type() {
    use maki_scene::ComponentTypes;

    let mut app = App::new(ConfigFile::default()).unwrap();
    app.add_module(SceneModule);
    let types = app.world().resource::<ComponentTypes>();
    let text = r#"(width: 4.0, depth: 4.0, height: 3.0, objects: [
        // A note of the object.
        (name: "ball", pose: (position: (1.0, 0.5, 0.0)), components: {
            "prop": (mesh: Sphere(radius: 0.15, segments: 8, rings: 4), look: (color: (0.15, 0.3, 0.45))),
            "swing": (angle: 12.5, axis: Y),
        }),
    ])"#;
    let registry = maki_events::CommandRegistry::default();
    let mut document = Document::parse(Path::new("room.ron"), text, &registry).unwrap();
    let before = document.clone();

    // The rows of the object, and the tree of each component read back: no change.
    let (name, tree) = document.tree(&Item::Object(0)).unwrap();
    assert_eq!(name, "object");
    assert!(tree.get("components").is_none());
    document.set_tree(&Item::Object(0), tree).unwrap();
    let prop = document.component_tree(0, "prop", types).unwrap().unwrap();
    document
        .set_component_tree(0, "prop", &prop, types)
        .unwrap();
    assert_eq!(document, before);
    // No type reads "swing": an error, and the text stays.
    assert!(document.component_tree(0, "swing", types).unwrap().is_err());
    assert!(
        document
            .set_component_tree(0, "swing", &prop, types)
            .is_err()
    );
    assert!(document.component_tree(0, "none", types).is_none());

    // A change of the pose and of a field of the prop.
    let (_, mut tree) = document.tree(&Item::Object(0)).unwrap();
    tree["pose"]["position"] = maki_core::fields::Value::from(vec![2.0, 0.5, 0.0]);
    document.set_tree(&Item::Object(0), tree).unwrap();
    let mut prop = prop;
    prop["collider"] = false.into();
    document
        .set_component_tree(0, "prop", &prop, types)
        .unwrap();

    // Saved and read again: the note, the text of "swing", the changes.
    let saved = document.text();
    assert_eq!(
        saved,
        r#"(
    width: 4.0,
    depth: 4.0,
    height: 3.0,
    next_id: 2,
    objects: [
        // A note of the object.
        (
            id: 1,
            name: "ball",
            pose: (position: (2.0, 0.5, 0.0)),
            components: {
                "prop": (
                    mesh: Sphere(radius: 0.15, segments: 8, rings: 4),
                    look: (color: (0.15, 0.3, 0.45)),
                    collider: false,
                ),
                "swing": (angle: 12.5, axis: Y),
            },
        ),
    ],
)
"#
    );
    let again = Document::parse(Path::new("room.ron"), &saved, &registry).unwrap();
    assert_eq!(again.text(), saved);
}

/// An instance of a prefab shows the objects of its file with its changes, in the
/// world and in the inspector; a look in the inspector changes nothing.
#[test]
fn an_instance_shows_its_prefab_with_its_changes() {
    use maki_scene::{PrefabUse, ron_options};

    let (app, directory) = editor_app("instance");
    std::fs::create_dir_all(directory.join("prefabs")).unwrap();
    let lamp = r#"(objects: [(name: "lamp", pose: (position: (0.0, 2.0, 0.0)),
        components: {"light": (radius: 0.1, color: (1.0, 1.0, 1.0), intensity: 8.0)})])"#;
    std::fs::write(directory.join("prefabs/lamp.prefab"), lamp).unwrap();
    app.run(|mut app| {
        frames(&mut app, 2);
        // A change of the intensity only; the radius follows the file.
        let changes = ron_options()
            .from_str(r#"{"lamp/light": {"intensity": 3.0}}"#)
            .unwrap();
        let used = PrefabUse {
            name: Some("lamp 2".to_owned()),
            file: "prefabs/lamp.prefab".into(),
            pose: Default::default(),
            changes,
        };
        let mut editor = app.resource_mut::<Editor>();
        editor.document.room.objects.push(used.into_object());
        editor.document.dirty = true;
        let item = Item::Object(editor.document.room.objects.len() - 1);
        editor.selection = vec![item];
        frames(&mut app, 3);
        let before = app.world().resource::<Editor>().document.clone();
        frames(&mut app, 3);
        assert_eq!(app.world().resource::<Editor>().document, before);

        let world = app.world();
        let mut lights = world.try_query::<(&Light, &RoomItem)>().unwrap();
        let of_instance: Vec<f32> = lights
            .iter(world)
            .filter(|(_, at)| at.0 == before.room.objects.len() - 1)
            .map(|(light, _)| light.intensity)
            .collect();
        assert_eq!(of_instance, [3.0]);

        // A change of the instance is one step of undo.
        let index = before.room.objects.len() - 1;
        let mut editor = app.resource_mut::<Editor>();
        let mut used = editor.document.room.objects[index]
            .prefab()
            .unwrap()
            .unwrap();
        used.changes.clear();
        let prefab = maki_scene::PrefabFile {
            file: used.file,
            changes: used.changes,
        };
        editor.document.room.objects[index].set_component("prefab", &prefab);
        editor.document.dirty = true;
        frames(&mut app, 2);
        request(&mut app, Command::Undo);
        assert_eq!(app.world().resource::<Editor>().document.room, before.room);
    });
    std::fs::remove_dir_all(&directory).unwrap();
}

/// The rows "Set a field" and "Call a script function" are rows as each other: a
/// change is one step of the history, and a wrong name is a problem of the row.
#[test]
fn a_general_command_row_has_undo_and_shows_its_problems() {
    use maki_events::{Builtin, TriggerArea};

    let (app, directory) = editor_app("general");
    app.run(|mut app| {
        frames(&mut app, 2);
        let mut editor = app.resource_mut::<Editor>();
        let registry = editor.registry.clone();
        let before = editor.document.event(LEAVE, &registry).unwrap().unwrap();
        // The object of the event gets an area, and its page a row of each.
        let area = TriggerArea::default();
        editor.document.room.objects[LEAVE].set_component("area", &area);
        let mut event = before.clone();
        let width = maki_core::fields::number(2.0, false);
        let rows = [
            Builtin::SetField("leave".into(), "area".into(), "width".into(), width.clone()),
            Builtin::SetField("leave".into(), "swing".into(), "angle".into(), width),
            Builtin::CallScript("nothing".into()),
        ];
        event.pages[0].commands = rows.map(|row| registry.command(row).unwrap()).into();
        editor.document.set_event(LEAVE, &event);
        editor.document.dirty = true;
        frames(&mut app, 2);

        let mut editor = app.resource_mut::<Editor>();
        let problems = editor.problems();
        let texts: Vec<(usize, &str)> = problems
            .iter()
            .map(|problem| (problem.object, problem.text.as_str()))
            .collect();
        assert_eq!(
            texts,
            [
                (LEAVE, "the object `leave` has no component `swing`"),
                (LEAVE, "no module added the script function `nothing`"),
            ]
        );
        // Undo: the page of before, with no area.
        request(&mut app, Command::Undo);
        let editor = app.world().resource::<Editor>();
        let event = editor.document.event(LEAVE, &editor.registry).unwrap();
        assert_eq!(event.unwrap(), before);
        assert!(
            !editor.document.room.objects[LEAVE]
                .components
                .contains_key("area")
        );
    });
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn f5_is_the_play_key_of_the_editor_and_the_reload_key_of_the_game() {
    let (app, directory) = editor_app("f5keys");
    let keys = |app: &App| {
        let bindings = &app.world().resource::<maki_platform::Bindings>().0;
        let on_f5 = |action| {
            bindings
                .iter()
                .any(|b| b.action == action && b.chord == Key::F5.into())
        };
        (on_f5(PLAY), on_f5(maki_gameplay::RELOAD_ROOM))
    };
    // The game: F5 reloads the room.
    assert_eq!(keys(&app), (true, true));
    app.run(|mut app| {
        frames(&mut app, 2);
        // The editor: F5 plays and stops only.
        assert_eq!(keys(&app), (true, false));
        // The key as a hand presses it: a game starts, and no frame asks for a reload.
        use maki_platform::RawInput;
        app.write_message(RawInput::KeyDown(Key::F5));
        for _ in 0..4 {
            frames(&mut app, 1);
            let actions = app.world().resource::<maki_platform::ActionState>();
            assert!(!actions.pressed(maki_gameplay::RELOAD_ROOM));
        }
        app.write_message(RawInput::KeyUp(Key::F5));
        frames(&mut app, 2);
        assert!(app.world().resource::<Editor>().playing());
    });
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn add_an_event_to_an_object_with_none_and_undo() {
    let (app, directory) = editor_app("addevent");
    app.run(|mut app| {
        frames(&mut app, 2);
        let editor = app.world().resource::<Editor>();
        let index = (0..editor.document.room.objects.len())
            .find(|i| editor.document.event(*i, &editor.registry).is_none())
            .unwrap();
        let mut editor = app.resource_mut::<Editor>();
        editor.document.add_event(index);
        editor.document.dirty = true;
        frames(&mut app, 2);
        let editor = app.world().resource::<Editor>();
        let event = editor
            .document
            .event(index, &editor.registry)
            .unwrap()
            .unwrap();
        assert_eq!(event.pages.len(), 1);
        request(&mut app, Command::Undo);
        let editor = app.world().resource::<Editor>();
        assert!(editor.document.event(index, &editor.registry).is_none());
    });
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn two_wires_on_one_output_have_two_tab_labels() {
    let wire = |output: &str| maki_events::Wire {
        output: output.to_owned(),
        commands: Vec::new(),
    };
    let wires = [wire("used"), wire("hit"), wire("used")];
    let label = |i| maki_events::Wire::label(&wires, i);
    assert_eq!(label(0), "used 1");
    assert_eq!(label(1), "hit");
    assert_eq!(label(2), "used 2");
}
