//! The editor with no window: a room is opened, changed, saved, played, and stopped.

use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy_ecs::query::With;
use glam::Vec3;
use maki_core::{App, ConfigFile, Interpolated, Time};
use maki_events::{EventsModule, GameState};
use maki_gameplay::{CameraModule, ControlsModule, PlayerModule, RoomsModule};
use maki_platform::{InputMode, PlatformModule, QuitRequested};
use maki_scene::{
    ActiveCamera, CurrentRoom, Light, MeshDef, MeshInstance, Player, RoomItem, SceneModule,
    parse_room,
};
use maki_ui::UiModule;

use super::*;

#[test]
fn the_command_line_gives_the_editor_options() {
    let parse = |args: &[&str]| parse_args(args.iter().map(OsString::from));
    let (editor, rest) = parse(&["my.toml", "--editor", "--editor-resave", "a.ron"]).unwrap();
    assert!(editor.open);
    assert_eq!(editor.resave, [PathBuf::from("a.ron")]);
    assert_eq!(rest, [OsString::from("my.toml")]);
    assert_eq!(parse(&["--bench"]).unwrap().0, EditorArgs::default());
    assert!(parse(&["--editor-resave"]).is_err());
}

/// Room A: a lamp, a cube, and an event that runs when the room loads. It sets a
/// flag, turns the lamp off, and goes to room B.
const ROOM_A: &str = r#"// The test room.
(
    width: 6.0,
    depth: 6.0,
    height: 3.0,
    props: [
        // The cube.
        (name: "cube", mesh: Cube(1.0), pose: (position: (1.0, 0.5, 0.0))),
    ],
    lights: [
        (name: "lamp", position: (0.0, 2.0, 0.0), radius: 0.1, color: (1.0, 0.9, 0.8), intensity: 5.0),
    ],
    spawns: {
        "start": (0.0, 0.0, 2.0),
    },
    events: [
        // Runs at the start.
        (
            name: "leave",
            pages: [
                (
                    trigger: Autorun,
                    commands: [SetFlag("left", true), LightOff("lamp"), Transfer("b.ron", "in")],
                ),
            ],
        ),
    ],
)
"#;
const ROOM_B: &str = r#"(width: 4.0, depth: 4.0, height: 3.0, spawns: {"in": (0.0, 0.0, 0.0)})"#;

/// A directory with the two rooms, and an app with the modules of a game and the
/// editor.
fn editor_app(name: &str) -> (App, PathBuf) {
    let directory = std::env::temp_dir().join(format!("maki-editor-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("a.ron"), ROOM_A).unwrap();
    std::fs::write(directory.join("b.ron"), ROOM_B).unwrap();
    let mut app = App::new(ConfigFile::default()).unwrap();
    app.add_module(PlatformModule)
        .add_module(maki_assets::AssetsModule)
        .add_module(SceneModule)
        .add_module(EventsModule)
        .add_module(ControlsModule)
        .add_module(PlayerModule)
        .add_module(CameraModule)
        .add_module(RoomsModule {
            start_room: directory.join("a.ron"),
            start_spawn: "start".to_owned(),
        })
        .add_module(UiModule)
        .add_module(EditorModule {
            rooms: directory.clone(),
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
            Item::Light(0),
            Item::Prop(0),
            Item::Spawn("start".to_owned()),
            Item::Event(0),
        ] {
            app.resource_mut::<Editor>().selection = vec![item];
            frames(&mut app, 3);
        }
        let editor = app.world().resource::<Editor>();
        assert_eq!(editor.document.text(), ROOM_A);
        assert_eq!(editor.history_steps(), (0, 0));

        // A light and the pose of a prop: the entities change, the room is not loaded.
        let mut editor = app.resource_mut::<Editor>();
        editor.document.room.lights[0].intensity = 9.0;
        editor.document.room.lights[0].position = [1.0, 2.5, 0.0];
        editor.document.room.props[0].pose.position = [-1.0, 0.5, 0.0];
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
        app.resource_mut::<Editor>().document.room.props[0].mesh = MeshDef::Cube(0.5);
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
        assert!(world.resource::<CurrentRoom>().def.props.is_empty());

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
        app.resource_mut::<QuitRequested>().0 = true;
        frames(&mut app, 2);
        let world = app.world();
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
    let window = world.resource::<maki_platform::WindowSettings>();
    let size = [window.width as f32, window.height as f32];
    let view = View::new(*cameras.single(world).unwrap(), size);
    let [x, y] = view.project(point).unwrap();
    // The picture starts at the right of the left panel.
    [
        x + world.resource::<maki_platform::WindowMargin>().left as f32,
        y,
    ]
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
        assert!(app.world().resource::<maki_platform::WindowMargin>().left > 0);

        // The cube, then the spawn point (a mark, no mesh), then the floor.
        click(&mut app, Vec3::new(1.0, 1.0, 0.0));
        assert_eq!(selection(&app), [Item::Prop(0)]);
        click(&mut app, Vec3::new(0.0, 0.1, 2.0));
        assert_eq!(selection(&app), [Item::Spawn("start".to_owned())]);
        click(&mut app, Vec3::new(-2.0, 0.0, -2.0));
        assert_eq!(selection(&app), []);
        // With shift, a click adds; the lamp is a sphere of light.
        click(&mut app, Vec3::new(1.0, 1.0, 0.0));
        app.write_message(RawInput::KeyDown(Key::Shift));
        click(&mut app, Vec3::new(0.0, 2.0, 0.0));
        app.write_message(RawInput::KeyUp(Key::Shift));
        assert_eq!(selection(&app), [Item::Prop(0), Item::Light(0)]);

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
            app.world().resource::<Editor>().document.room.props[0]
                .pose
                .position
        };
        assert_eq!(cube(&app), [-1.25, 0.5, 1.5]);
        assert_eq!(app.world().resource::<CurrentRoom>().loads, loads);
        assert_eq!(app.world().resource::<Editor>().history_steps(), (1, 0));

        request(&mut app, Command::Undo);
        assert_eq!(cube(&app), [1.0, 0.5, 0.0]);
        request(&mut app, Command::Redo);
        assert_eq!(cube(&app), [-1.25, 0.5, 1.5]);

        // A copy, then the copy is removed: the selection follows.
        request(&mut app, Command::Duplicate);
        assert_eq!(selection(&app), [Item::Prop(1)]);
        request(&mut app, Command::Delete);
        assert_eq!(selection(&app), []);
        assert_eq!(
            app.world().resource::<Editor>().document.room.props.len(),
            1
        );
        // The comment of the cube is still the comment of the cube.
        assert!(
            app.world()
                .resource::<Editor>()
                .document
                .text()
                .contains("// The cube.\n        (name: \"cube\"")
        );
    });
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_mover_that_the_editor_adds_moves_in_the_game_when_an_event_opens_it() {
    use maki_events::{Builtin, EventDef, Page, Trigger};

    let (app, directory) = editor_app("mover");
    app.run(|mut app| {
        frames(&mut app, 2);
        request(&mut app, Command::New);
        request(&mut app, Command::Add(Kind::Mover));
        let mut editor = app.resource_mut::<Editor>();
        assert_eq!(editor.selection, [Item::Mover(0)]);
        let registry = editor.registry.clone();
        let command = |command: Builtin| registry.command(command).unwrap();
        editor.document.room.movers[0].seconds = 0.2;
        editor.document.events.push(EventDef {
            name: "lift".to_owned(),
            area: Default::default(),
            pages: vec![Page {
                conditions: Vec::new(),
                trigger: Trigger::Autorun,
                commands: vec![
                    command(Builtin::OpenDoor("mover".to_owned())),
                    command(Builtin::SetFlag("up".to_owned(), true)),
                ],
            }],
        });
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
        request(&mut app, Command::Test("leave".to_owned(), 0));
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
        // The page that the test started, and the same page by its own trigger.
        assert_eq!(
            waits(&app),
            [r#"SetFlag("left", true)"#, r#"SetFlag("left", true)"#]
        );
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
    add_argument_editor(&mut app, |_ui, Sound::Play(_, volume): &mut Sound| {
        *volume += 1.0;
        true
    });
    let editors = app.world().resource::<ArgumentEditors>().clone();
    let sound = registry.parse_command(r#"Play("door", 0.5)"#).unwrap();
    let wait = registry.parse_command("Wait(1.0)").unwrap();
    let mut output = egui::Context::default().run_ui(egui::RawInput::default(), |ui| {
        let mut edit = |command| {
            editors
                .0
                .iter()
                .find_map(|edit| edit(ui, command, &registry))
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
