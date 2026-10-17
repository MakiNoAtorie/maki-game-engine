//! Tests of the interpreter with a fixed step: a headless app, a room file that the
//! test writes, and one fixed step for each frame.

use std::path::PathBuf;
use std::time::Duration;

use bevy_ecs::query::With;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Commands, Query, Res, ResMut};
use bevy_ecs::world::World;
use maki_core::{App, ConfigFile, FixedUpdate, Interpolated, Startup, Time, Transform};
use maki_platform::{Key, PlatformModule, RawInput};
use maki_scene::{
    CurrentRoom, Light, Mover, MoverState, Name, ObjectId, Player, RoomRequest, RoomTarget,
    SceneModule, ScreenFade,
};

use super::*;

mod data;
mod flow;
mod items;
mod menu_items;
mod menus;
mod messages;
mod numbers;
mod objects;
mod repeat;
mod review;
mod state;
mod text;

/// 50 Hz: one fixed step of 20 ms for each frame.
const STEP: Duration = Duration::from_millis(20);

/// A room of 8 x 8 m with a lamp, a door in the back wall, and these events.
fn room(events: &str) -> String {
    let events = short_events(events);
    let objects = [
        r#"(id: 1, name: "door", components: {"door": (seconds: 0.1), "doorway": (wall: NegZ, offset: 0.0, width: 1.0, height: 2.0, depth: 1.0)})"#,
        r#"(id: 2, name: "lamp", pose: (position: (0, 2, 0)), components: {"light": (radius: 0.1, color: (1, 1, 1), intensity: 5.0)})"#,
        r#"(id: 3, name: "start", components: {"spawn": ()})"#,
        r#"(id: 4, name: "east", pose: (position: (2, 0, 0)), components: {"spawn": ()})"#,
        &events,
    ];
    maki_scene::room_text(8.0, 8.0, 3.0, &objects)
}

/// The directory of the room files of one test.
fn directory(test: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("maki-events-{test}-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

/// A headless game in the room `a.ron` of `directory`, with the player at "start".
/// The first step with the events of the room is that of the second frame.
fn game(directory: &std::path::Path) -> App {
    let path = directory.join("a.ron");
    let mut app = App::new(ConfigFile::from_text("[time]\nfixed_hz = 50.0")).unwrap();
    app.add_module(PlatformModule)
        .add_module(SceneModule)
        .add_module(maki_physics::PhysicsModule)
        .add_module(EventsModule)
        .add_systems(
            Startup,
            move |mut commands: Commands, mut request: ResMut<RoomRequest>| {
                // The capsule is what the sensors of the areas see (the walk adds it in a game).
                let body = maki_physics::Kinematic {
                    shape: maki_physics::Shape::Capsule {
                        radius: 0.3,
                        half_height: 0.5,
                    },
                    center: glam::Vec3::Y * 0.8,
                    sensor: false,
                };
                commands.spawn((Player, Interpolated::at(Transform::IDENTITY), body));
                request.0 = Some(RoomTarget {
                    path: path.clone(),
                    spawn: Some(ObjectId(3)),
                    text: None,
                });
            },
        );
    app
}

fn flag(app: &App, name: &str) -> bool {
    app.world().resource::<GameState>().flag(name)
}

fn running(app: &App) -> usize {
    app.world().resource::<EventRunner>().running().len()
}

fn steps(app: &mut App, count: u32) {
    for _ in 0..count {
        app.advance(STEP);
    }
}

/// One frame with a tap of the interact key in it.
fn interact(app: &mut App) {
    app.write_message(RawInput::KeyDown(Key::Z))
        .write_message(RawInput::KeyUp(Key::Z));
    app.advance(STEP);
}

#[test]
fn a_wait_takes_its_seconds_in_steps_and_the_game_goes_on() {
    let directory = directory("wait");
    let events = r#"(name: "timer", pages: [(trigger: Autorun, commands: [
        SetFlag("started", true), Wait(1.0), SetFlag("ended", true)])])"#;
    std::fs::write(directory.join("a.ron"), room(events)).unwrap();
    let app = game(&directory);
    app.run(|mut app| {
        // Frame 1 loads the room, the step of frame 2 starts the event, and the step
        // of frame 3 runs its first commands.
        steps(&mut app, 3);
        assert!(flag(&app, "started") && !flag(&app, "ended"));
        // An autorun event takes the control from the player.
        assert!(app.world().resource::<EventRunner>().blocks_player());
        // 1 s is 50 steps. The step of frame 3 was the first one.
        steps(&mut app, 48);
        assert!(!flag(&app, "ended"));
        steps(&mut app, 1);
        assert!(flag(&app, "ended"));
        assert_eq!(running(&app), 0);
        // The game made a fixed step in each frame: no frame waited for the event.
        assert_eq!(app.world().resource::<Time>().fixed_tick, 52);
        // Autorun: one time, not again.
        steps(&mut app, 5);
        assert_eq!(running(&app), 0);
    });
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_branch_takes_the_side_of_its_flag_and_a_later_page_takes_over() {
    let directory = directory("branch");
    let events = r#"
        (name: "first", pages: [(trigger: Autorun, commands: [
            If([Flag("seen")], [SetVariable("side", 1)], [SetVariable("side", 2)]),
            AddVariable("side", 10),
            SetFlag("seen", true)])]),
        (name: "second", pages: [(conditions: [Flag("seen")], trigger: Autorun, commands: [
            If([Flag("seen"), Variable("side", Equals, 12)],
                [SetVariable("again", 1)], [SetVariable("again", 2)])])]),
        (name: "loop", pages: [
            (trigger: Parallel, commands: [AddVariable("count", 1), Wait(0.04)]),
            (conditions: [Variable("count", AtLeast, 3)], trigger: Parallel, commands: [])])"#;
    std::fs::write(directory.join("a.ron"), room(events)).unwrap();
    let app = game(&directory);
    app.run(|mut app| {
        steps(&mut app, 12);
        let state = app.world().resource::<GameState>();
        // The flag was not set: the second list, then the commands after the branch.
        assert_eq!(state.variable("side"), 12);
        // The page of the second event became active when the flag was set.
        assert_eq!(state.variable("again"), 1);
        // The parallel event started again each time it ended, until its second page
        // (with no commands) took over. It did not take the control from the player.
        assert_eq!(state.variable("count"), 3);
        assert!(!app.world().resource::<EventRunner>().blocks_player());
    });
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn interact_starts_the_event_that_the_player_faces_and_a_message_waits_for_a_press() {
    let directory = directory("interact");
    // The player is at the origin and faces +Z.
    let events = r#"
        (name: "behind", area: (center: (0, -1), size: (1, 1)),
            pages: [(trigger: Interact, commands: [SetFlag("behind", true)])]),
        (name: "far", area: (center: (0, 3), size: (1, 1)),
            pages: [(trigger: Interact, commands: [SetFlag("far", true)])]),
        (name: "sign", area: (center: (0, 1), size: (1, 1)),
            pages: [(trigger: Interact, commands: [Message("Hello."), SetFlag("read", true)])])"#;
    std::fs::write(directory.join("a.ron"), room(events)).unwrap();
    let app = game(&directory);
    let message = |app: &App| app.world().resource::<MessageBox>().0.clone();
    app.run(|mut app| {
        steps(&mut app, 3);
        assert_eq!(running(&app), 0);
        interact(&mut app);
        steps(&mut app, 1);
        // Only the event in front of the player, in reach.
        assert_eq!(message(&app).as_deref(), Some("Hello."));
        assert!(app.world().resource::<EventRunner>().blocks_player());
        steps(&mut app, 30);
        assert_eq!(message(&app).as_deref(), Some("Hello."));
        assert!(!flag(&app, "read"));
        // The press that closes the message starts no event again.
        interact(&mut app);
        assert_eq!(message(&app), None);
        assert!(flag(&app, "read"));
        steps(&mut app, 3);
        assert_eq!(message(&app), None);
        assert!(!flag(&app, "behind") && !flag(&app, "far"));
    });
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn touch_starts_when_the_player_walks_into_the_area() {
    let directory = directory("touch");
    // The first area is around the start of the player, the second one is at x = 2.
    let events = r#"
        (name: "here", area: (center: (0, 0), size: (1, 1)),
            pages: [(trigger: Touch, commands: [AddVariable("here", 1)])]),
        (name: "plate", area: (center: (2, 0), size: (1, 1)),
            pages: [(trigger: Touch, commands: [AddVariable("plate", 1)])])"#;
    std::fs::write(directory.join("a.ron"), room(events)).unwrap();
    let mut app = game(&directory);
    // The player walks 0.25 m along X in each step from step 10 to step 21: over the
    // plate and past it.
    app.add_systems(
        FixedUpdate,
        (|time: Res<Time>, mut players: Query<&mut Interpolated, With<Player>>| {
            if (10..22).contains(&time.fixed_tick) {
                players.single_mut().unwrap().current.translation.x += 0.25;
            }
        })
        .before(run_events),
    );
    app.run(|mut app| {
        steps(&mut app, 30);
        let state = app.world().resource::<GameState>();
        // The player started in the first area: it did not walk into it.
        assert_eq!(state.variable("here"), 0);
        // One time, when the player came in. Not in each step in the area.
        assert_eq!(state.variable("plate"), 1);
    });
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn commands_reach_the_door_the_light_the_fade_and_the_next_room() {
    let directory = directory("world");
    let events = r#"(name: "show", pages: [(trigger: Autorun, commands: [
        OpenDoor(1), SetFlag("open", true),
        LightIntensity(2, 2.0), LightOff(2),
        FadeOut(0.1), SetFlag("dark", true),
        Transfer("b.ron", 4),
        SetFlag("there", true), FadeIn(0.1), CloseDoor(1), LightOn(99)])])"#;
    std::fs::write(directory.join("a.ron"), room(events)).unwrap();
    std::fs::write(directory.join("b.ron"), room("")).unwrap();
    let app = game(&directory);
    let door = |app: &App| {
        let mut movers = app.world().try_query::<(&Name, &Mover)>().unwrap();
        movers.single(app.world()).unwrap().1.state
    };
    let lamp = |app: &App| {
        let mut lights = app.world().try_query::<&Light>().unwrap();
        *lights.single(app.world()).unwrap()
    };
    let fade = |app: &App| app.world().resource::<ScreenFade>().0;
    app.run(|mut app| {
        steps(&mut app, 3);
        // The door takes 0.1 s, some steps. The command waits for it.
        assert!(matches!(door(&app), MoverState::ToB(_)));
        assert!(!flag(&app, "open"));
        steps(&mut app, 8);
        assert_eq!(door(&app), MoverState::AtB);
        assert!(flag(&app, "open"));
        assert_eq!((lamp(&app).intensity, lamp(&app).on), (2.0, false));
        // The fade out has started, and it also takes some steps.
        assert!(fade(&app) > 0.0 && !flag(&app, "there"));

        // The fade ends, then the transfer: the loader runs in the frame of the
        // request, and the event goes on in the next step, in the new room.
        steps(&mut app, 8);
        assert!(flag(&app, "dark") && flag(&app, "there"));
        // The place of the spawn point, for the player module.
        let current = app.world().resource::<CurrentRoom>();
        assert!(current.path.ends_with("b.ron"));
        assert_eq!(current.spawn, Some([2.0, 0.0, 0.0]));
        // The new room has its own door and lamp, as its file says: closed and on.
        assert_eq!(door(&app), MoverState::AtA);
        assert!(lamp(&app).on);
        // The fade in, a door that is closed already, and a light that is not there:
        // the event ends.
        steps(&mut app, 10);
        assert_eq!(fade(&app), 0.0);
        assert_eq!(running(&app), 0);
    });
    std::fs::remove_dir_all(directory).unwrap();
}

/// The room of [`room`] with ids, each object renamed after the commands named it
/// by its id; and a button (id 5) whose wire opens the door by its id.
fn renamed_room(events: &str) -> String {
    let button = r#"objects: [(id: 5, name: "knob", pose: (position: (3, 0, 3)), components: {"area": (width: 0.5, depth: 0.5), "event": (wires: [(output: "used", commands: [OpenDoor(1)])])}),"#;
    room(events)
        .replacen("objects: [", button, 1)
        .replacen(r#"(id: 1, name: "door""#, r#"(id: 1, name: "gate""#, 1)
        .replacen(r#"(id: 2, name: "lamp""#, r#"(id: 2, name: "bulb""#, 1)
        .replacen(r#"(id: 4, name: "east""#, r#"(id: 4, name: "far""#, 1)
}

#[test]
fn a_command_a_wire_and_a_transfer_by_id_reach_their_objects_after_a_rename() {
    let directory = directory("ids");
    let events = r#"(name: "show", pages: [(trigger: Autorun, commands: [
        Signal(5, "used"), LightOff(2), SetField(2, "light", "intensity", 3.0),
        Wait(0.3), Transfer("b.ron", 4), SetFlag("there", true)])])"#;
    std::fs::write(directory.join("a.ron"), renamed_room(events)).unwrap();
    std::fs::write(directory.join("b.ron"), renamed_room("")).unwrap();
    let app = game(&directory);
    app.run(|mut app| {
        steps(&mut app, 12);
        let world = app.world();
        let door = world
            .try_query::<&Mover>()
            .unwrap()
            .single(world)
            .unwrap()
            .state;
        assert_eq!(door, MoverState::AtB);
        let lamp = *world.try_query::<&Light>().unwrap().single(world).unwrap();
        assert_eq!((lamp.on, lamp.intensity), (false, 3.0));
        steps(&mut app, 15);
        assert!(flag(&app, "there"));
        let current = app.world().resource::<CurrentRoom>();
        assert!(current.path.ends_with("b.ron"));
        assert_eq!(current.spawn, Some([2.0, 0.0, 0.0]));
    });
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn the_check_reports_an_id_with_no_object() {
    let mut app = App::new(ConfigFile::default()).unwrap();
    app.add_module(PlatformModule)
        .add_module(SceneModule)
        .add_module(EventsModule);
    app.update();
    let validators = app.world().resource::<maki_scene::RoomValidators>();
    let events = r#"(name: "a", pages: [(trigger: Interact, commands: [
        OpenDoor(1), OpenDoor(9), LightOn(2), LightOff(1), Signal(8, "used"),
        SetField(7, "light", "intensity", 1.0), Transfer("a.ron", 4), Transfer("a.ron", 6)])])"#;
    let path = std::path::Path::new("a.ron");
    let error = maki_scene::check_room(path, &renamed_room(events), validators)
        .unwrap_err()
        .to_string();
    for wanted in [
        "the room has no door or mover `id 9`",
        "the room has no light `id 1`",
        "the room has no object `id 8`",
        "the room has no object `id 7`",
    ] {
        assert!(error.contains(wanted), "{wanted}: {error}");
    }
    // Each other row is right: four errors. The spawn point is a warning.
    assert_eq!(error.lines().count(), 4, "{error}");
}

#[test]
fn a_transfer_to_a_bad_room_does_not_happen_and_the_event_goes_on() {
    let directory = directory("bad");
    let events = r#"(name: "go", pages: [(trigger: Autorun, commands: [
        Transfer("b.ron", 3), SetFlag("after", true)])])"#;
    std::fs::write(directory.join("a.ron"), room(events)).unwrap();
    // A command that does not exist.
    let bad = r#"(name: "x", pages: [(trigger: Autorun, commands: [Explode])])"#;
    std::fs::write(directory.join("b.ron"), room(bad)).unwrap();
    let app = game(&directory);
    app.run(|mut app| {
        steps(&mut app, 6);
        assert!(
            app.world()
                .resource::<CurrentRoom>()
                .path
                .ends_with("a.ron")
        );
        assert!(flag(&app, "after"));
        assert_eq!(running(&app), 0);
    });
    std::fs::remove_dir_all(directory).unwrap();
}

/// The commands of a module that is not this crate.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
enum TestCommand {
    /// Waits this many steps.
    Steps(u32),
    /// Runs the list this many times.
    Twice(Vec<Command>),
    Ring {
        bell: String,
    },
}

#[test]
fn a_module_adds_commands_that_wait_and_that_hold_command_lists() {
    let directory = directory("module");
    let events = r#"(name: "bell", pages: [(trigger: Autorun, commands: [
        Steps(3),
        Twice([AddVariable("count", 1), If([Flag("no")], [], [Ring(bell: "small")])]),
        SetFlag("ended", true)])])"#;
    std::fs::write(directory.join("a.ron"), room(events)).unwrap();
    let app = game(&directory);
    let registry = app.world().resource::<CommandRegistry>().clone();
    registry.add(
        |command: &TestCommand, step: &mut Step, world: &mut World| {
            match command {
                TestCommand::Steps(steps) => {
                    let done = step.memory::<u32>();
                    *done += 1;
                    return *done >= *steps;
                }
                TestCommand::Twice(list) => step.open([list.clone(), list.clone()].concat()),
                TestCommand::Ring { bell } => {
                    world.resource_mut::<GameState>().set_flag(bell, true)
                }
            }
            true
        },
    );
    assert!(registry.names().contains(&"Ring") && registry.names().contains(&"Wait"));
    app.run(|mut app| {
        // The step of frame 3 runs the first command. It waits for three steps.
        steps(&mut app, 4);
        assert_eq!(app.world().resource::<GameState>().variable("count"), 0);
        steps(&mut app, 1);
        assert_eq!(app.world().resource::<GameState>().variable("count"), 2);
        assert!(flag(&app, "small") && flag(&app, "ended"));
    });
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn in_step_mode_each_command_waits_for_its_step_and_a_page_starts_for_a_test() {
    let directory = directory("step");
    // The second page needs a flag that nothing sets, and an interact that no
    // one gives: only a test starts it.
    let events = r#"(id: 4, name: "door", pages: [
        (trigger: Interact, commands: [SetFlag("first", true)]),
        (conditions: [Flag("never")], trigger: Interact, commands: [
            SetFlag("a", true), Wait(0.04), SetFlag("b", true), SetFlag("c", true),
        ]),
    ])"#;
    std::fs::write(directory.join("a.ron"), room(events)).unwrap();
    // A tool starts a page with the world: the pages asked for, and what each start
    // returned.
    #[derive(bevy_ecs::resource::Resource, Default)]
    struct Starts(Vec<(ObjectId, usize)>, Vec<bool>);
    let mut app = game(&directory);
    app.init_resource::<Starts>().add_systems(
        maki_core::Update,
        (|world: &mut World| {
            let asked = std::mem::take(&mut world.resource_mut::<Starts>().0);
            // A new name: the event is of the object, by its id.
            let mut names = world.query::<&mut Name>();
            for mut name in names.iter_mut(world).filter(|name| name.0 == "door") {
                name.0 = "gate".to_owned();
            }
            for (id, page) in asked {
                let started = start_event(world, id, page);
                world.resource_mut::<Starts>().1.push(started);
            }
        })
        .ambiguous_with_all(),
    );
    app.run(|mut app| {
        steps(&mut app, 2);
        let current = |app: &App| {
            let runner = app.world().resource::<EventRunner>();
            runner
                .running()
                .first()
                .and_then(|event| event.current())
                .map(|c| format!("{c:?}"))
        };
        app.resource_mut::<EventRunner>().set_step_mode(true);
        let (door, window) = (ObjectId(4), ObjectId(9));
        app.resource_mut::<Starts>().0 = vec![(door, 7), (window, 0), (door, 1)];
        steps(&mut app, 1);
        assert_eq!(app.world().resource::<Starts>().1, [false, false, true]);
        // No step was asked for: the event waits before its first command.
        steps(&mut app, 5);
        assert_eq!(current(&app).as_deref(), Some(r#"SetFlag("a", true)"#));
        assert!(!flag(&app, "a"));

        // One step: one command. The wait takes its time, then waits for a step.
        app.resource_mut::<EventRunner>().step_command();
        steps(&mut app, 1);
        assert!(flag(&app, "a"));
        assert_eq!(current(&app).as_deref(), Some("Wait(0.04)"));
        app.resource_mut::<EventRunner>().step_command();
        steps(&mut app, 5);
        assert_eq!(current(&app).as_deref(), Some(r#"SetFlag("b", true)"#));
        assert!(!flag(&app, "b"));

        // Step mode off: the rest runs.
        app.resource_mut::<EventRunner>().set_step_mode(false);
        steps(&mut app, 1);
        assert!(flag(&app, "b") && flag(&app, "c") && !flag(&app, "first"));
        assert_eq!(running(&app), 0);
    });
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_command_is_read_from_its_text() {
    let registry = CommandRegistry::default();
    registry.add(crate::builtin::run);
    let command = registry
        .parse_command(r#"If([Flag("seen")], [Wait(0.5)], [])"#)
        .unwrap();
    let Some(Builtin::If(conditions, yes, no)) = command.get::<Builtin>() else {
        panic!("an If: {command:?}");
    };
    assert_eq!(conditions, &[Condition::Flag("seen".to_owned())]);
    assert_eq!((yes.len(), no.len()), (1, 0));
    assert_eq!(Some(&yes[0]), registry.command(Builtin::Wait(0.5)).as_ref());
    // The texts of a command set for a tool: a line for a reader, and one line of
    // help for a name. A set with no texts gives none.
    assert_eq!(registry.row_text(&command), None);
    registry.add_texts(crate::builtin::row_text, crate::builtin::help);
    assert_eq!(registry.row_text(&command).as_deref(), Some("If seen"));
    assert_eq!(registry.row_text(&yes[0]).as_deref(), Some("Wait 0.5 s"));
    assert!(registry.help("Wait").is_some() && registry.help("Explode").is_none());
    for bad in ["Wait(", "Wait(\"long\")", "Explode", ""] {
        assert!(registry.parse_command(bad).is_err(), "{bad}");
    }
}

/// The checks that the room loader makes for the events of a file, with the line of
/// each command, and the same texts from `check_events` for a tool.
#[test]
fn a_wrong_name_is_an_error_with_its_line_and_a_room_that_is_not_there_is_a_warning() {
    let directory = directory("checks");
    let path = directory.join("a.ron");
    let mut app = App::new(ConfigFile::default()).unwrap();
    app.add_module(PlatformModule)
        .add_module(SceneModule)
        .add_module(EventsModule);
    let registry = app.world().resource::<CommandRegistry>().clone();
    // A check of a module, added after the validator was made.
    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    enum Sound {
        Ring(i32),
    }
    registry.add(|_: &Sound, _: &mut Step, _: &mut World| true);
    registry.add_check(|Sound::Ring(times): &Sound, _: &CheckContext| {
        (*times < 1).then(|| Finding::error("a bell rings one time or more".to_owned()))
    });
    let validators = app.world().resource::<maki_scene::RoomValidators>();
    let check = |events: &str| maki_scene::check_room(&path, &room(events), validators);

    // `room` has its event objects from line 7, each object in the line of its
    // name. Two equal commands: each has its own line. A problem of an event has
    // the line of its component `event`.
    let bad = r#"
        (name: "a", pages: [(trigger: Interact, commands: [
            OpenDoor(1),
            If([Flag("x")], [
                OpenDoor(9)], [
                OpenDoor(9)]),
            Ring(0),
            LightOn(2)])]),
        (name: "b", pages: []),
        (name:
            "a", pages: [(trigger: Touch, commands: [LightOff(8)])])"#;
    let error = check(bad).unwrap_err().to_string();
    let lines: Vec<&str> = error.lines().collect();
    let file = path.display().to_string();
    let expected = [
        "9: event `a`: the room has no door or mover `id 9`",
        "10: event `a`: the room has no door or mover `id 9`",
        "11: event `a`: a bell rings one time or more",
        "14: event `a`: another event has the name `a`",
        "14: event `a`: the room has no light `id 8`",
    ];
    assert_eq!(lines, expected.map(|line| format!("{file}:{line}")));

    // The text of a problem is that of `check_events`, which a tool calls.
    let text = room(bad);
    let def = maki_scene::parse_room(&text).unwrap();
    let context = CheckContext {
        room: &def,
        file: "a.ron",
        directory: std::path::Path::new(""),
        spawn: &|_, _| None,
        registry: &registry,
    };
    let problems = registry.check_events(&registry.parse_events(&text).unwrap(), &context);
    let found: Vec<_> = problems
        .iter()
        .map(|problem| (problem.event, problem.warning, problem.text.as_str()))
        .collect();
    assert_eq!(found[0], (0, false, "the room has no door or mover `id 9`"));
    assert_eq!(found.len(), 5);
    let wrong = registry.command(Builtin::OpenDoor(ObjectId(9)));
    assert_eq!(problems[0].command, wrong.map(|command| (0, command)));

    // A `Transfer` to a file or a spawn point that is not there: the room loads.
    // The command after a `Transfer` runs in the other room: no check of its name.
    let transfer = r#"(name: "a", pages: [
        (trigger: Touch, commands: [Transfer("b.ron", 5), LightOn(7)]),
        (trigger: Touch, commands: [Transfer("a.ron", 4)]),
        (trigger: Touch, commands: [Transfer("a.ron", 6)])])"#;
    assert!(check(transfer).is_ok());
    let def = maki_scene::parse_room(&room(transfer)).unwrap();
    let texts = |spawn: &dyn Fn(&str, &maki_scene::ObjectId) -> Option<bool>| -> Vec<String> {
        let context = CheckContext {
            room: &def,
            file: "a.ron",
            directory: std::path::Path::new(""),
            spawn,
            registry: &registry,
        };
        let events = registry.parse_events(&room(transfer)).unwrap();
        let problems = registry.check_events(&events, &context);
        assert!(problems.iter().all(|problem| problem.warning));
        problems.into_iter().map(|problem| problem.text).collect()
    };
    assert_eq!(
        texts(&|_, _| None),
        ["no room file `b.ron`", "`a.ron` has no spawn point `id 6`"]
    );
    assert_eq!(
        texts(&|_, spawn| Some(*spawn == ObjectId(5))),
        ["`a.ron` has no spawn point `id 6`"]
    );
    std::fs::remove_dir_all(&directory).unwrap();
}

/// An event is the event of its object: two events with one id are an error, two
/// with one name and their own ids are not (a name is not a key).
#[test]
fn the_check_finds_two_events_of_one_object_by_id() {
    let directory = directory("check-ids");
    let app = game(&directory);
    let validators = app.world().resource::<maki_scene::RoomValidators>();
    let path = directory.join("a.ron");
    let check = |events: &str| maki_scene::check_room(&path, &room(events), validators);
    let twins = r#"(id: 7, name: "a", pages: []), (id: 7, name: "b", pages: [])"#;
    let error = check(twins).unwrap_err().to_string();
    assert!(
        error.ends_with("event `b`: another event has the id 7"),
        "{error}"
    );
    let named = r#"(id: 7, name: "a", pages: []), (id: 8, name: "a", pages: [])"#;
    assert!(check(named).is_ok());
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_question_runs_the_list_of_the_answer_that_the_player_takes() {
    let directory = directory("choice");
    let events = r#"(name: "ask", pages: [(trigger: Autorun, commands: [
        Message("A door."),
        Choice("Open it?", [
            ("Yes", [SetFlag("yes", true)]),
            ("No", [SetFlag("no", true),
                OpenDoor(9)]),
        ]),
        SetFlag("after", true)])])"#;
    // The check of the loader: the line of a command in the list of an answer, and
    // a question with no answer.
    let app = game(&directory);
    let validators = app.world().resource::<maki_scene::RoomValidators>();
    let path = directory.join("a.ron");
    let error = maki_scene::check_room(&path, &room(events), validators).unwrap_err();
    assert!(
        error
            .to_string()
            .ends_with(":11: event `ask`: the room has no door or mover `id 9`"),
        "{error}"
    );
    let empty = r#"(name: "ask", pages: [(trigger: Autorun, commands: [Choice("?", [])])])"#;
    let error = maki_scene::check_room(&path, &room(empty), validators).unwrap_err();
    assert!(
        error.to_string().contains("a question needs an answer"),
        "{error}"
    );

    let events = events.replace("OpenDoor(9)", "OpenDoor(1)");
    std::fs::write(&path, room(&events)).unwrap();
    app.run(|mut app| {
        let tap = |app: &mut App, key| {
            app.write_message(RawInput::KeyDown(key))
                .write_message(RawInput::KeyUp(key));
            app.advance(STEP);
        };
        let choice = |app: &App| app.world().resource::<ChoiceBox>().0.clone();
        let message = |app: &App| app.world().resource::<MessageBox>().0.clone();
        steps(&mut app, 3);
        assert_eq!(message(&app).as_deref(), Some("A door."));
        assert_eq!(choice(&app), None);
        // The press that closes the message is not the answer of the question.
        interact(&mut app);
        assert_eq!(message(&app).as_deref(), Some("Open it?"));
        let answers = vec!["Yes".to_owned(), "No".to_owned()];
        let shown = |selected| {
            Some(Choices {
                answers: answers.clone(),
                selected,
            })
        };
        assert_eq!(choice(&app), shown(0));
        steps(&mut app, 2);
        assert!(!flag(&app, "yes") && !flag(&app, "no"));
        // The mark goes down and up, and stops at the two ends.
        tap(&mut app, Key::ArrowUp);
        assert_eq!(choice(&app), shown(0));
        tap(&mut app, Key::ArrowDown);
        tap(&mut app, Key::ArrowDown);
        assert_eq!(choice(&app), shown(1));
        tap(&mut app, Key::W);
        assert_eq!(choice(&app), shown(0));
        tap(&mut app, Key::S);
        // The answer: its list runs, then the commands after the question.
        interact(&mut app);
        assert_eq!((choice(&app), message(&app)), (None, None));
        steps(&mut app, 20);
        assert!(flag(&app, "no") && !flag(&app, "yes") && flag(&app, "after"));
        assert_eq!(running(&app), 0);
    });
    std::fs::remove_dir_all(&directory).unwrap();
}

/// A script that counts its hooks in variables of the game state, and goes along X
/// by `speed` meters in each second.
#[derive(
    bevy_ecs::component::Component,
    Debug,
    Default,
    serde::Serialize,
    serde::Deserialize,
    maki_core::Fields,
)]
#[serde(default)]
struct Counter {
    speed: f32,
}

impl Counter {
    fn count(context: &mut ScriptContext, hook: &str) {
        let mut state = context.state();
        let count = state.variable(hook);
        state.set_variable(hook, count + 1);
    }
}

impl Script for Counter {
    fn start(&mut self, context: &mut ScriptContext) {
        Self::count(context, "start");
    }

    fn step(&mut self, context: &mut ScriptContext) {
        Self::count(context, "step");
        let mut pose = context.pose();
        pose.translation.x += self.speed * context.seconds();
        context.set_pose(pose);
    }

    fn interact(&mut self, context: &mut ScriptContext) {
        Self::count(context, "interact");
        // A command of the registry, as a row calls it: the message waits for a
        // press, and the player has no control until then.
        context.call(Builtin::Message("hello".into()));
    }

    fn enter_area(&mut self, context: &mut ScriptContext) {
        Self::count(context, "enter");
    }

    fn leave_area(&mut self, context: &mut ScriptContext) {
        Self::count(context, "leave");
    }
}

/// A room with the counter at (0, 0, 1.4): its area of 1 x 1 m is 0.9 m in front of
/// the player at the start, in reach.
fn counter_room(speed: f32, events: &str) -> String {
    room(events).replacen(
        "objects: [",
        &format!(
            r#"objects: [(id: 10, name: "counter", pose: (position: (0, 0, 1.4)), components: {{"counter": (speed: {speed}), "area": (width: 1.0, depth: 1.0)}}),"#
        ),
        1,
    )
}

fn counter_game(test: &str, room: &str) -> App {
    let directory = directory(test);
    std::fs::write(directory.join("a.ron"), room).unwrap();
    let mut app = game(&directory);
    add_script::<Counter>(&mut app, "counter");
    app.init_resource::<PlaceAt>().add_systems(
        maki_core::FixedFirst,
        |mut place: ResMut<PlaceAt>, mut players: Query<&mut Interpolated, With<Player>>| {
            if let Some(at) = place.0.take() {
                for mut pose in &mut players {
                    pose.snap(Transform::from_translation(at.into()));
                }
            }
        },
    );
    app
}

fn variable(app: &App, name: &str) -> i32 {
    app.world().resource::<GameState>().variable(name)
}

#[test]
fn each_hook_of_a_script_runs_at_its_time() {
    let app = counter_game("hooks", &counter_room(0.0, ""));
    app.run(|mut app| {
        // The room loads in the first frame, the script starts in the step of the
        // second.
        steps(&mut app, 3);
        assert_eq!(variable(&app, "start"), 1);
        assert_eq!(variable(&app, "step"), 2);
        assert_eq!(variable(&app, "enter"), 0);

        // The player faces the area and presses interact: the hook, and the message
        // that it calls.
        interact(&mut app);
        assert_eq!(variable(&app, "interact"), 1);
        assert!(app.world().resource::<MessageBox>().0.is_some());
        assert!(app.world().resource::<EventRunner>().blocks_player());
        // The press that closes the message goes to the message, not to the script.
        steps(&mut app, 3);
        interact(&mut app);
        steps(&mut app, 1);
        assert_eq!(variable(&app, "interact"), 1);
        assert!(!app.world().resource::<EventRunner>().blocks_player());

        // Into the area, and out.
        // The hooks see the sensor in the step after a jump of the player.
        place_player(&mut app, [0.0, 0.0, 1.4]);
        steps(&mut app, SENSOR_LAG);
        assert_eq!((variable(&app, "enter"), variable(&app, "leave")), (1, 0));
        steps(&mut app, 2);
        assert_eq!(variable(&app, "enter"), 1);
        place_player(&mut app, [0.0, 0.0, -2.0]);
        steps(&mut app, SENSOR_LAG);
        assert_eq!((variable(&app, "enter"), variable(&app, "leave")), (1, 1));
        // The player looks away from the area: no interact.
        interact(&mut app);
        assert_eq!(variable(&app, "interact"), 1);
        assert_eq!(variable(&app, "start"), 1);
    });
}

/// Where the player goes at the start of the next step.
#[derive(bevy_ecs::resource::Resource, Default)]
struct PlaceAt(Option<[f32; 3]>);

/// Steps from a jump of the player until the commands of a wire of an area have run: the
/// physics sees the new pose in the step of the jump, the events in the next step, and
/// the wire runs its commands in the step after.
const SENSOR_LAG: u32 = 3;

fn place_player(app: &mut App, at: [f32; 3]) {
    app.resource_mut::<PlaceAt>().0 = Some(at);
}

#[test]
fn a_script_moves_its_object_and_an_object_with_no_code_keeps_its_text() {
    let app = counter_game("move", &counter_room(5.0, ""));
    app.run(|mut app| {
        // One step of the script in the second frame: 5 m/s for 20 ms.
        steps(&mut app, 2);
        let world = app.world();
        let mut counters = world.try_query::<(&Counter, &Interpolated)>().unwrap();
        let (counter, pose) = counters.single(world).unwrap();
        assert_eq!(counter.speed, 5.0);
        assert!((pose.current.translation.x - 0.1).abs() < 1e-6, "{pose:?}");
    });
    // The same room with no script registered: the object keeps its component as
    // text, and the room loads.
    let directory = directory("no-code");
    std::fs::write(directory.join("a.ron"), counter_room(5.0, "")).unwrap();
    let app = game(&directory);
    app.run(|mut app| {
        steps(&mut app, 2);
        let room = &app.world().resource::<CurrentRoom>().def;
        assert_eq!(room.objects[0].components["counter"].as_str(), "(speed:5)");
    });
}

#[test]
fn an_event_and_a_script_of_one_object_share_its_area_and_it_moves_with_the_object() {
    // The counter goes along X at 1.5 m/s, with an event on the same object.
    let room = counter_room(1.5, "").replacen(
        r#""area": (width: 1.0, depth: 1.0)"#,
        r#""area": (width: 1.0, depth: 1.0), "event": (pages: [(trigger: Touch, commands: [AddVariable("touched", 1)])])"#,
        1,
    );
    let app = counter_game("shared-area", &room);
    app.run(|mut app| {
        steps(&mut app, 2);
        // At X = 2 the player is out of the area at the start (X from -0.5 to 0.5).
        place_player(&mut app, [2.0, 0.0, 1.4]);
        steps(&mut app, 2);
        assert_eq!((variable(&app, "touched"), variable(&app, "enter")), (0, 0));
        // After 1.2 s the area reaches X = 1.5: the player is in it, and the event
        // starts. It blocks the player in that step, so the hook `enter_area` does
        // not run: one area, the rules of stage 50.
        steps(&mut app, 60);
        assert_eq!((variable(&app, "touched"), variable(&app, "enter")), (1, 0));
    });
}

#[test]
fn a_script_and_a_row_use_one_store_of_variables() {
    // The counter writes the variable "step" in each step; a page waits for it.
    let events = r#"(name: "watch", pages: [
        (trigger: Parallel, commands: []),
        (conditions: [Variable("step", AtLeast, 5)], trigger: Autorun, commands: [SetVariable("seen", 1)])])"#;
    let app = counter_game("one-store", &counter_room(0.0, events));
    app.run(|mut app| {
        steps(&mut app, 4);
        assert_eq!(variable(&app, "seen"), 0);
        steps(&mut app, 4);
        assert_eq!(variable(&app, "seen"), 1);
    });
}

#[test]
fn one_press_starts_an_event_or_a_script_not_both() {
    let events = r#"(name: "sign", area: (center: (0.0, 1.4), size: (1.0, 1.0)), pages: [(trigger: Interact, commands: [SetFlag("read", true)])])"#;
    let app = counter_game("press", &counter_room(0.0, events));
    app.run(|mut app| {
        steps(&mut app, 2);
        interact(&mut app);
        // The event runs its commands from the next step.
        steps(&mut app, 1);
        assert!(flag(&app, "read"));
        assert_eq!(variable(&app, "interact"), 0);
    });
}

#[test]
fn a_row_sets_a_field_of_a_component_of_an_object() {
    // The speed of the script, and the width of the area of its object.
    let events = r#"(name: "tune", pages: [(trigger: Autorun, commands: [
        SetField(10, "counter", "speed", 2.5),
        SetField(10, "area", "width", 3),
    ])])"#;
    let app = counter_game("set-field", &counter_room(0.0, events));
    app.run(|mut app| {
        steps(&mut app, 3);
        let world = app.world();
        let mut counters = world.try_query::<(&Counter, &TriggerArea)>().unwrap();
        let (counter, area) = counters.single(world).unwrap();
        assert_eq!((counter.speed, area.width, area.depth), (2.5, 3.0, 1.0));
        assert_eq!(running(&app), 0);
    });
}

#[test]
fn a_row_sets_the_strength_color_and_radius_of_a_light_in_play() {
    let directory = directory("set-light");
    let events = r#"(name: "dim", pages: [(trigger: Autorun, commands: [
        SetField(2, "light", "intensity", 2.0),
        SetField(2, "light", "color", (1.0, 0.5, 0.0)),
        SetField(2, "light", "radius", 0.3),
    ])])"#;
    std::fs::write(directory.join("a.ron"), room(events)).unwrap();
    game(&directory).run(|mut app| {
        steps(&mut app, 3);
        let mut lights = app.world().try_query::<&Light>().unwrap();
        let lamp = *lights.single(app.world()).unwrap();
        let wanted = (2.0, glam::Vec3::new(1.0, 0.5, 0.0), 0.3, true);
        assert_eq!((lamp.intensity, lamp.color, lamp.radius, lamp.on), wanted);
        assert_eq!(running(&app), 0);
    });
}

#[test]
fn a_row_opens_a_door_and_takes_the_collider_from_a_prop_in_play() {
    let directory = directory("set-door-prop");
    let events = r#"(name: "open", pages: [(trigger: Autorun, commands: [
        SetField(1, "door", "open", true),
        SetField(1, "door", "seconds", 0.5),
        SetField(11, "prop", "collider", false),
    ])])"#;
    let text = room(events).replacen(
        "objects: [",
        r#"objects: [(id: 11, name: "crate", pose: (position: (2, 0, 2)), components: {"prop": (mesh: Cube(1.0))}),"#,
        1,
    );
    std::fs::write(directory.join("a.ron"), text).unwrap();
    game(&directory).run(|mut app| {
        let crate_collider = |app: &mut App| {
            let world = app.world();
            let query = world.try_query::<(&Name, bevy_ecs::query::Has<maki_scene::Collider>)>();
            let mut props = query.unwrap();
            let found = props.iter(world).find(|(name, _)| name.0 == "crate");
            found.unwrap().1
        };
        steps(&mut app, 1);
        assert!(crate_collider(&mut app));
        steps(&mut app, 2);
        let mut doors = app.world().try_query::<&maki_scene::Mover>().unwrap();
        let door = *doors.single(app.world()).unwrap();
        assert_ne!(door.state, maki_scene::MoverState::AtA);
        assert_eq!(door.duration, 0.5);
        assert!(!crate_collider(&mut app));
        assert_eq!(running(&app), 0);
    });
}

#[test]
fn the_check_finds_a_field_that_the_component_does_not_have_and_a_value_of_a_wrong_kind() {
    let events = r#"(name: "e", pages: [(trigger: Autorun, commands: [
        SetField(2, "light", "intensity", 2),
        SetField(2, "light", "strength", 2.0),
        SetField(2, "light", "intensity", "bright"),
        SetField(1, "door", "color", (1.0, 0.0, 0.0)),
    ])])"#;
    let directory = directory("check-field");
    let path = directory.join("a.ron");
    std::fs::write(&path, room(events)).unwrap();
    let mut app = game(&directory);
    // The check knows the types of components after the modules are built.
    app.update();
    let validators = app.world().resource::<maki_scene::RoomValidators>();
    let error = maki_scene::check_room(&path, &room(events), validators).unwrap_err();
    let text = format!("{error:#}");
    assert!(
        text.contains("the component `light` has no field `strength`"),
        "{text}"
    );
    assert!(
        text.contains("\"bright\" is not a value of the field `light.intensity`"),
        "{text}"
    );
    assert!(
        text.contains("the field `door.color` does not change in the game"),
        "{text}"
    );
    assert_eq!(text.matches("event `e`").count(), 3, "{text}");
}

#[test]
fn a_row_sets_a_field_of_a_component_of_the_prefab_of_an_instance() {
    let directory = directory("set-prefab");
    let lamp = r#"(objects: [(name: "bulb", pose: (position: (0, 1, 0)), components: {
        "light": (radius: 0.1, color: (1, 1, 1), intensity: 8.0)})])"#;
    std::fs::write(directory.join("lamp.ron"), lamp).unwrap();
    // By the id of the instance: the prefab object with the component.
    for (object, off) in [("6", ""), ("6", "LightOff(6)")] {
        let events = format!(
            r#"(name: "dim", pages: [(trigger: Autorun, commands: [
            SetField({object}, "light", "intensity", 3.0), {off}])])"#
        );
        let text = room(&events).replacen(
            "objects: [",
            r#"objects: [(id: 6, name: "standing", pose: (position: (2, 0, 2)), components: {"prefab": (file: "lamp.ron")}),"#,
            1,
        );
        std::fs::write(directory.join("a.ron"), text).unwrap();
        // The room loads only if the check finds no problem.
        game(&directory).run(|mut app| {
            steps(&mut app, 3);
            let mut lights = app.world().try_query::<(&Name, &Light)>().unwrap();
            let bulb = lights.iter(app.world()).find(|(name, _)| name.0 == "bulb");
            let bulb = bulb.unwrap().1;
            assert_eq!((bulb.intensity, bulb.on), (3.0, off.is_empty()));
            assert_eq!(running(&app), 0);
        });
    }
}

#[test]
fn a_row_calls_a_function_of_the_script_crate_with_the_object_of_the_event() {
    fn rename_count(context: &mut ScriptContext) {
        let name = context.name().to_owned();
        let mut state = context.state();
        let calls = state.variable(&name);
        state.set_variable(&name, calls + 1);
    }
    let events = r#"(name: "caller", pages: [(trigger: Autorun, commands: [
        CallScript("count"), CallScript("count")])])"#;
    let mut app = counter_game("call", &counter_room(0.0, events));
    add_script_function(&mut app, "count", rename_count);
    app.run(|mut app| {
        steps(&mut app, 3);
        // The context is the object of the event: its name.
        assert_eq!(variable(&app, "caller"), 2);
        assert_eq!(running(&app), 0);
    });
}

/// A script that names a light by the id in its field, and sets the intensity of the
/// light to the count of its steps.
#[derive(
    bevy_ecs::component::Component,
    Debug,
    Default,
    serde::Serialize,
    serde::Deserialize,
    maki_core::Fields,
)]
#[serde(default)]
struct Dimmer {
    light: Option<ObjectId>,
    steps: f32,
}

impl Script for Dimmer {
    fn step(&mut self, context: &mut ScriptContext) {
        self.steps += 1.0;
        let light = self.light.and_then(|id| context.object_mut::<Light>(id));
        if let Some(mut light) = light {
            light.intensity = self.steps;
        }
    }
}

#[test]
fn a_script_finds_an_object_by_the_id_of_its_field_after_a_rename() {
    // The room has a light "lamp" with the id 2; the light of the dimmer has the id 7.
    let room = room("").replacen(
        "objects: [",
        r#"objects: [(id: 7, name: "bulb", components: {"light": (radius: 0.1, color: (1, 1, 1), intensity: 5.0)}),
            (name: "dimmer", components: {"dimmer": (light: Some(7))}),"#,
        1,
    );
    let directory = directory("by-id");
    std::fs::write(directory.join("a.ron"), room).unwrap();
    let mut app = game(&directory);
    add_script::<Dimmer>(&mut app, "dimmer");
    // The rename: the object of the id 7 gets a new name, before the step.
    app.init_resource::<NewName>().add_systems(
        maki_core::FixedFirst,
        |mut new: ResMut<NewName>, mut names: Query<(&ObjectId, &mut Name)>| {
            let Some(text) = new.0.take() else { return };
            for (id, mut name) in &mut names {
                if *id == ObjectId(7) {
                    name.0 = text.clone();
                }
            }
        },
    );
    app.run(|mut app| {
        let intensity = |app: &App, id: Option<ObjectId>| {
            let world = app.world();
            let mut lights = world.try_query::<(&Light, Option<&ObjectId>)>().unwrap();
            let found = lights.iter(world).find(|(_, other)| other.copied() == id);
            found.unwrap().0.intensity
        };
        steps(&mut app, 3);
        assert_eq!(intensity(&app, Some(ObjectId(7))), 2.0);
        assert_eq!(intensity(&app, Some(ObjectId(2))), 5.0);
        // A new name: the id stays, and the script finds the light.
        app.resource_mut::<NewName>().0 = Some("old bulb".to_owned());
        steps(&mut app, 1);
        let world = app.world();
        let mut names = world.try_query::<(&ObjectId, &Name)>().unwrap();
        let renamed = names.iter(world).find(|(id, _)| **id == ObjectId(7));
        assert_eq!(renamed.unwrap().1.0, "old bulb");
        assert_eq!(intensity(&app, Some(ObjectId(7))), 3.0);
    });
}

/// The new name of the object of the id 7, at the start of the next step.
#[derive(bevy_ecs::resource::Resource, Default)]
struct NewName(Option<String>);

#[test]
fn a_called_function_shows_a_message_and_the_next_row_runs_after_it() {
    fn greet(context: &mut ScriptContext) {
        context.call(Builtin::SetVariable("order".to_owned(), 1));
        context.call(Builtin::Message("hello".into()));
    }
    let events = r#"(name: "caller", pages: [(trigger: Autorun, commands: [
        CallScript("greet"), AddVariable("order", 10)])])"#;
    let mut app = counter_game("call-wait", &counter_room(0.0, events));
    add_script_function(&mut app, "greet", greet);
    app.run(|mut app| {
        let message = |app: &App| app.world().resource::<MessageBox>().0.clone();
        steps(&mut app, 3);
        // The pushed commands ran in the step of the call; the next row waits.
        assert_eq!(message(&app).as_deref(), Some("hello"));
        assert_eq!(variable(&app, "order"), 1);
        interact(&mut app);
        steps(&mut app, 1);
        assert_eq!(message(&app), None);
        assert_eq!(variable(&app, "order"), 11);
        assert_eq!(running(&app), 0);
    });
}

#[test]
fn the_check_finds_a_wrong_object_component_or_function_of_a_general_command() {
    let mut app = App::new(ConfigFile::default()).unwrap();
    app.add_module(PlatformModule)
        .add_module(SceneModule)
        .add_module(EventsModule);
    add_script_function(&mut app, "count", |_| {});
    let registry = app.world().resource::<CommandRegistry>().clone();
    let text = room(
        r#"(name: "e", pages: [(trigger: Autorun, commands: [
        SetField(2, "light", "intensity", 2.0),
        SetField(8, "light", "intensity", 2.0),
        SetField(2, "swing", "angle", 2.0),
        CallScript("count"),
        CallScript("cont"),
    ])])"#,
    );
    let def = maki_scene::parse_room(&text).unwrap();
    let context = CheckContext {
        room: &def,
        file: "a.ron",
        directory: std::path::Path::new(""),
        spawn: &|_, _| None,
        registry: &registry,
    };
    let events = registry.parse_events(&text).unwrap();
    let problems = registry.check_events(&events, &context);
    let texts: Vec<&str> = problems
        .iter()
        .map(|problem| problem.text.as_str())
        .collect();
    assert_eq!(
        texts,
        [
            "the room has no object `id 8`",
            "the object `lamp` has no component `swing`",
            "no module added the script function `cont`",
        ]
    );
}

#[test]
fn interact_starts_nothing_outside_the_state_play() {
    let events = r#"(name: "sign", area: (center: (0.0, 1.4), size: (1.0, 1.0)), pages: [(trigger: Interact, commands: [SetFlag("read", true)])])"#;
    let app = counter_game("play-state", &counter_room(0.0, events));
    app.run(|mut app| {
        steps(&mut app, 2);
        for state in [maki_core::PlayState::Cutscene, maki_core::PlayState::Title] {
            *app.resource_mut::<maki_core::PlayState>() = state;
            interact(&mut app);
            steps(&mut app, 1);
            assert!(!flag(&app, "read"), "{state:?}");
        }
        *app.resource_mut::<maki_core::PlayState>() = maki_core::PlayState::Play;
        interact(&mut app);
        steps(&mut app, 1);
        assert!(flag(&app, "read"));
    });
}

/// A button: an area with a wire on each of its outputs.
fn button_room(wires: &str) -> String {
    room("").replacen(
        "objects: [",
        &format!(
            r#"objects: [(id: 12, name: "button", pose: (position: (0, 0, 1.4)), components: {{"area": (width: 1.0, depth: 1.0), "event": (wires: [{wires}])}}),"#
        ),
        1,
    )
}

#[test]
fn the_area_sends_its_outputs_to_the_wires_of_its_object() {
    let wires = r#"(output: "used", commands: [AddVariable("used", 1)]),
        (output: "entered", commands: [AddVariable("entered", 1)]),
        (output: "left", commands: [AddVariable("left", 1)]),
        (output: "used", commands: [OpenDoor(1)])"#;
    let app = counter_game("outputs", &button_room(wires));
    app.run(|mut app| {
        steps(&mut app, 3);
        // The player faces the area from 0.9 m and presses interact: each wire of
        // `used`, in the order of the list.
        interact(&mut app);
        steps(&mut app, 10);
        assert_eq!(variable(&app, "used"), 1);
        let world = app.world();
        let mut doors = world.try_query::<(&Name, &Mover)>().unwrap();
        let (_, door) = doors.single(world).unwrap();
        assert_eq!(door.state, MoverState::AtB);
        // Into the area and out.
        place_player(&mut app, [0.0, 0.0, 1.4]);
        steps(&mut app, SENSOR_LAG);
        place_player(&mut app, [0.0, 0.0, -2.0]);
        steps(&mut app, SENSOR_LAG);
        assert_eq!((variable(&app, "entered"), variable(&app, "left")), (1, 1));
    });
}

/// A script that sends its output `ping` in its first step.
#[derive(
    bevy_ecs::component::Component,
    Debug,
    Default,
    serde::Serialize,
    serde::Deserialize,
    maki_core::Fields,
)]
struct Pinger {}

impl Script for Pinger {
    fn start(&mut self, context: &mut ScriptContext) {
        context.signal("ping");
    }
}

#[test]
fn a_signal_row_and_a_script_send_outputs_to_wires() {
    let room = button_room(r#"(output: "used", commands: [AddVariable("used", 1)])"#).replacen(
        "objects: [",
        r#"objects: [
            (name: "pinger", components: {"pinger": (), "event": (wires: [(output: "ping", commands: [AddVariable("pinged", 1)])])}),
            (name: "pad", components: {"event": (pages: [(trigger: Autorun, commands: [Signal(12, "used")])])}),"#,
        1,
    );
    let mut app = counter_game("signal", &room);
    add_script::<Pinger>(&mut app, "pinger");
    add_outputs(&mut app, "pinger", &["ping"]);
    app.run(|mut app| {
        steps(&mut app, 6);
        // The row sent `used` of the button; the script sent its own `ping`.
        assert_eq!((variable(&app, "used"), variable(&app, "pinged")), (1, 1));
    });
}

#[test]
fn the_check_finds_a_wrong_output_of_a_wire_and_of_a_signal() {
    let mut app = App::new(ConfigFile::default()).unwrap();
    app.add_module(PlatformModule)
        .add_module(SceneModule)
        .add_module(EventsModule);
    let registry = app.world().resource::<CommandRegistry>().clone();
    let text = button_room(
        r#"(output: "used", commands: [Signal(12, "entered")]),
        (output: "pressed", commands: [Signal(12, "pushed"), Signal(13, "used")])"#,
    );
    let def = maki_scene::parse_room(&text).unwrap();
    let context = CheckContext {
        room: &def,
        file: "a.ron",
        directory: std::path::Path::new(""),
        spawn: &|_, _| None,
        registry: &registry,
    };
    let events = registry.parse_events(&text).unwrap();
    let problems = registry.check_events(&events, &context);
    let texts: Vec<&str> = problems
        .iter()
        .map(|problem| problem.text.as_str())
        .collect();
    assert_eq!(
        texts,
        [
            "the wire `pressed`: the object `button` has no output `pressed`",
            "the object `button` has no output `pushed`",
            "the room has no object `id 13`",
        ]
    );
}

#[test]
fn a_wire_that_sends_its_own_output_starts_once_in_a_step_and_an_event_goes_first() {
    // The second wire of `used` runs after the first, and sends `used` again.
    let wires = r#"(output: "used", commands: [AddVariable("count", 1)]),
        (output: "used", commands: [Signal(12, "used")])"#;
    let app = counter_game("loop", &button_room(wires));
    app.run(|mut app| {
        steps(&mut app, 3);
        interact(&mut app);
        steps(&mut app, 10);
        // One start in each step: about ten, not a loop with no end in one step.
        let count = variable(&app, "count");
        assert!((5..=11).contains(&count), "{count}");
    });
    // An event and a wire on one press of the same area: the event.
    let room = button_room(r#"(output: "used", commands: [SetFlag("wire", true)])"#).replacen(
        r#""event": (wires:"#,
        r#""event": (pages: [(trigger: Interact, commands: [SetFlag("page", true)])], wires:"#,
        1,
    );
    let app = counter_game("press-order", &room);
    app.run(|mut app| {
        steps(&mut app, 3);
        interact(&mut app);
        steps(&mut app, 2);
        assert!(flag(&app, "page") && !flag(&app, "wire"));
    });
}

#[test]
fn a_signal_and_entered_start_wires_only_in_the_state_play() {
    let wires = r#"(output: "used", commands: [AddVariable("used", 1)]),
        (output: "entered", commands: [AddVariable("entered", 1)])"#;
    let app = counter_game("wires-play-state", &button_room(wires));
    app.run(|mut app| {
        steps(&mut app, 3);
        let button = {
            let world = app.world();
            let mut objects = world
                .try_query::<(bevy_ecs::entity::Entity, &Name)>()
                .unwrap();
            let found = objects.iter(world).find(|(_, name)| name.0 == "button");
            found.unwrap().0
        };
        // Out of the area, a signal `used`, and into the area: `entered`.
        let signal_and_enter = |app: &mut App| {
            place_player(app, [0.0, 0.0, -2.0]);
            steps(app, SENSOR_LAG);
            let used = (button, TriggerArea::USED.to_owned());
            app.resource_mut::<Signals>().0.push(used);
            // The wire of `used` blocks the player while it runs: `entered` then waits.
            steps(app, 2);
            place_player(app, [0.0, 0.0, 1.4]);
            steps(app, SENSOR_LAG);
        };
        for state in [maki_core::PlayState::Cutscene, maki_core::PlayState::Title] {
            *app.resource_mut::<maki_core::PlayState>() = state;
            signal_and_enter(&mut app);
            let counts = (variable(&app, "used"), variable(&app, "entered"));
            assert_eq!(counts, (0, 0), "{state:?}");
        }
        *app.resource_mut::<maki_core::PlayState>() = maki_core::PlayState::Play;
        signal_and_enter(&mut app);
        assert_eq!((variable(&app, "used"), variable(&app, "entered")), (1, 1));
    });
}

#[test]
fn a_running_wire_has_the_start_of_a_wire_not_of_a_trigger() {
    let wires = r#"(output: "used", commands: [Wait(1.0)])"#;
    let app = counter_game("wire-start", &button_room(wires));
    app.run(|mut app| {
        steps(&mut app, 3);
        interact(&mut app);
        steps(&mut app, 1);
        let runner = app.world().resource::<EventRunner>();
        let starts: Vec<Start> = runner.running().iter().map(|event| event.start).collect();
        assert_eq!(starts, [Start::Wire]);
        // A wire blocks the player as an event that the player started.
        assert!(runner.blocks_player());
    });
}

#[test]
fn a_problem_of_a_wire_names_the_wire_as_its_tab_does() {
    let mut app = App::new(ConfigFile::default()).unwrap();
    app.add_module(PlatformModule)
        .add_module(SceneModule)
        .add_module(EventsModule);
    let registry = app.world().resource::<CommandRegistry>().clone();
    let text = button_room(
        r#"(output: "used", commands: []),
        (output: "pressed", commands: []),
        (output: "pressed", commands: [])"#,
    );
    let def = maki_scene::parse_room(&text).unwrap();
    let context = CheckContext {
        room: &def,
        file: "a.ron",
        directory: std::path::Path::new(""),
        spawn: &|_, _| None,
        registry: &registry,
    };
    let events = registry.parse_events(&text).unwrap();
    let problems = registry.check_events(&events, &context);
    let texts: Vec<&str> = problems
        .iter()
        .map(|problem| problem.text.as_str())
        .collect();
    assert_eq!(
        texts,
        [
            "the wire `pressed 1`: the object `button` has no output `pressed`",
            "the wire `pressed 2`: the object `button` has no output `pressed`",
        ]
    );
}

#[test]
fn a_command_enum_with_fields_gives_a_tool_its_table_and_reads_a_changed_tree() {
    use maki_core::fields::{Kind, Value};
    use maki_scene::ObjectId;

    /// A command of a script crate: it names an object of the room.
    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, maki_core::Fields)]
    enum Ring {
        Ring(ObjectId, f32),
    }

    let mut app = App::new(ConfigFile::default()).unwrap();
    app.add_module(PlatformModule)
        .add_module(SceneModule)
        .add_module(EventsModule);
    let registry = app.world().resource::<CommandRegistry>().clone();
    registry.add(|_: &Ring, _, _| true);
    let bell = |id| registry.command(Ring::Ring(ObjectId(id), 0.5));
    let command = bell(3).unwrap();
    // No table before the module gives it.
    assert!(registry.fields(&command).is_none());
    registry.add_fields::<Ring>();

    let (kind, mut tree) = registry.fields(&command).unwrap();
    let Kind::Choice(variants) = kind else {
        panic!("an enum is a choice");
    };
    assert_eq!(variants[0].fields[0].kind, Kind::Object);
    // The id is a number of the tree; another id is another command.
    let id = variants[0].part(&mut tree, 0).unwrap();
    assert_eq!(*id, Value::from(3));
    *id = Value::from(4);
    assert_eq!(registry.from_tree(&command, tree), bell(4));
    // A command of an enum with no table: none.
    let wait = registry.parse_command("Wait(1.0)").unwrap();
    assert!(registry.fields(&wait).is_none());
}

#[test]
fn a_hidden_area_gives_no_output_and_gives_it_again_when_shown() {
    let wires = r#"(output: "entered", commands: [AddVariable("entered", 1)]),
        (output: "left", commands: [AddVariable("left", 1)])"#;
    let switch = r#"objects: [(name: "switch", components: {"event": (pages: [
        (conditions: [Flag("hide")], trigger: Autorun, commands: [Hide(12)]),
        (conditions: [Flag("show")], trigger: Autorun, commands: [Show(12)])])}),"#;
    let room = button_room(wires).replacen("objects: [", switch, 1);
    let app = counter_game("hidden-area", &room);
    app.run(|mut app| {
        steps(&mut app, 4);
        place_player(&mut app, [0.0, 0.0, 1.4]);
        steps(&mut app, SENSOR_LAG);
        assert_eq!((variable(&app, "entered"), variable(&app, "left")), (1, 0));
        // Hidden with the player in it: no output, not even `left`.
        app.resource_mut::<GameState>().set_flag("hide", true);
        steps(&mut app, SENSOR_LAG);
        assert_eq!((variable(&app, "entered"), variable(&app, "left")), (1, 0));
        // The player walks out and in while it is hidden: nothing.
        place_player(&mut app, [0.0, 0.0, -2.0]);
        steps(&mut app, SENSOR_LAG);
        place_player(&mut app, [0.0, 0.0, 1.4]);
        steps(&mut app, SENSOR_LAG);
        assert_eq!((variable(&app, "entered"), variable(&app, "left")), (1, 0));
        // Shown with the player in it: it enters the area.
        app.resource_mut::<GameState>().set_flag("show", true);
        // `Show` runs in the step after its page starts.
        steps(&mut app, SENSOR_LAG + 1);
        assert_eq!((variable(&app, "entered"), variable(&app, "left")), (2, 0));
    });
}

/// A room with an object at (0, 0, 1.4) of this pose (after the position) and these
/// components, with wires that count `entered` and `left`.
fn area_room(pose: &str, components: &str) -> String {
    let object = format!(
        r#"objects: [(id: 12, name: "button", pose: (position: (0, 0, 1.4){pose}), components: {{{components}, "event": (wires: [
        (output: "entered", commands: [AddVariable("entered", 1)]),
        (output: "left", commands: [AddVariable("left", 1)])])}}),"#
    );
    room("").replacen("objects: [", &object, 1)
}

#[test]
fn an_area_on_a_mover_has_its_own_sensor() {
    let mover = r#""mover": (mesh: Cube(0.5), b: (position: (0, 0, 1.4)), seconds: 1.0), "area": (width: 1.0, depth: 1.0)"#;
    let app = counter_game("mover-area", &area_room("", mover));
    app.run(|mut app| {
        steps(&mut app, 3);
        // The mover keeps its body that blocks.
        let physics = app.world().resource::<maki_physics::Physics>();
        let filter = maki_physics::QueryFilter::default();
        let hit = physics.ray(glam::Vec3::new(0.0, 0.1, 0.5), glam::Vec3::Z, 3.0, filter);
        assert!(
            hit.is_some_and(|hit| (hit.distance - 0.65).abs() < 0.01),
            "{hit:?}"
        );
        place_player(&mut app, [0.0, 0.0, 1.4]);
        steps(&mut app, SENSOR_LAG);
        place_player(&mut app, [0.0, 0.0, -2.0]);
        steps(&mut app, SENSOR_LAG);
        assert_eq!((variable(&app, "entered"), variable(&app, "left")), (1, 1));
    });
}

#[test]
fn the_sensor_of_an_area_does_not_turn_with_its_object() {
    // 2 m along X and 0.4 m along Z, on an object turned by 90 degrees.
    let area = r#""area": (width: 2.0, depth: 0.4)"#;
    let app = counter_game("turned-area", &area_room(", rotation: (0, 90, 0)", area));
    app.run(|mut app| {
        steps(&mut app, 3);
        // In the rectangle along the axes, out of the turned one: it enters.
        place_player(&mut app, [0.8, 0.0, 1.4]);
        steps(&mut app, SENSOR_LAG);
        assert_eq!(variable(&app, "entered"), 1);
        place_player(&mut app, [0.0, 0.0, -2.0]);
        steps(&mut app, SENSOR_LAG);
        // Out of the rectangle along the axes, in the turned one: it does not enter.
        place_player(&mut app, [0.0, 0.0, 2.2]);
        steps(&mut app, SENSOR_LAG);
        assert_eq!((variable(&app, "entered"), variable(&app, "left")), (1, 1));
    });
}
