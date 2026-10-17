//! Menus: the file, the stack, the focus and the input, a pick, and a script menu.

use maki_core::PlayState;

use super::*;
use crate::{Menu, MenuCommand, MenuRow, Menus, ShownMenus, ShownRow};

/// An event that a test starts.
#[derive(bevy_ecs::resource::Resource, Default)]
struct Probe {
    start: Option<ObjectId>,
}

/// A menu with a row that shows only with the flag `open`, a row that calls a
/// routine, and one that calls a script function.
const MAIN: &str = r#"(title: "Main", rows: [
    Commands(text: "First", commands: [SetFlag("first", true)]),
    Commands(text: "Hidden", conditions: [Flag("open")], commands: [SetFlag("hidden", true)]),
    Commands(text: "Routine", commands: [CallRoutine("r.routine.ron", [])]),
    Commands(text: "Script", commands: [CallScript("picked")]),
])"#;

/// A menu that the back action does not close, and a HUD.
const FIXED: &str = r#"(back: false, rows: [Commands(text: "Stay")])"#;
const HUD: &str = r#"(focus: false, place: TopLeft, rows: [Commands(text: "Coins")])"#;

/// A first row with no focus, and two rows that show only with the flag `x`.
const HEAD: &str = r#"(rows: [
    Text("Head"),
    Commands(text: "A", commands: [SetFlag("a", true)]),
    Commands(text: "B"),
    Commands(text: "C", conditions: [Flag("x")]),
    Commands(text: "D", conditions: [Flag("x")]),
    Commands(text: "E", commands: [SetFlag("e", true)]),
])"#;

/// A menu with focus and no place of focus.
const ONLY_TEXT: &str = r#"(rows: [Text("Only")])"#;

/// A game whose event "opener" runs these commands at the start. The menu files
/// are `data/menus/<name>.menu.ron`.
fn menu_game(test: &str, commands: &str) -> App {
    let directory = directory(test);
    let menus = directory.join("data/menus");
    std::fs::create_dir_all(&menus).unwrap();
    let files = [
        ("main", MAIN),
        ("fixed", FIXED),
        ("hud", HUD),
        ("head", HEAD),
        ("only", ONLY_TEXT),
    ];
    for (name, text) in files {
        std::fs::write(menus.join(format!("{name}.menu.ron")), text).unwrap();
    }
    let routine = r#"(commands: [SetFlag("routine", true)])"#;
    std::fs::write(directory.join("r.routine.ron"), routine).unwrap();
    let events = format!(
        r#"(id: 10, name: "opener", pages: [(trigger: Autorun, commands: [{commands}])]),
        (id: 11, name: "closer", pages: [(trigger: Interact, commands: [CallScript("shut")])]),
        (id: 12, name: "all", pages: [(trigger: Interact, commands: [CloseMenus])])"#
    );
    std::fs::write(directory.join("a.ron"), room(&events)).unwrap();
    let mut app = game(&directory);
    add_script_function(&mut app, "picked", |context: &mut ScriptContext| {
        let name = context.name().to_owned();
        context.state().set_flag(&format!("script {name}"), true);
    });
    add_script_function(&mut app, "shut", |context: &mut ScriptContext| {
        context.close_menu();
    });
    app.init_resource::<Probe>().add_systems(
        maki_core::Update,
        (|world: &mut World| {
            if let Some(id) = world.resource_mut::<Probe>().start.take() {
                assert!(start_event(world, id, 0));
            }
        })
        .ambiguous_with_all(),
    );
    app
}

/// Starts the page of an event; it runs in the next fixed step.
fn start(app: &mut App, id: u32) {
    app.resource_mut::<Probe>().start = Some(ObjectId(id));
    steps(app, 2);
}

/// A tap of a key, and a frame with no key: a tap of the same action in the next
/// frame is a new press.
fn tap(app: &mut App, key: Key) {
    app.write_message(RawInput::KeyDown(key))
        .write_message(RawInput::KeyUp(key));
    steps(app, 2);
}

fn menus(app: &App) -> &Menus {
    app.world().resource::<Menus>()
}

/// The rows of the top menu that show, with a mark at the focus.
fn rows(app: &App) -> Vec<String> {
    let shown = &app.world().resource::<ShownMenus>().0;
    let rows = &shown.last().unwrap().rows;
    let line = |row: &ShownRow| format!("{}{}", if row.focus { "> " } else { "" }, row.text);
    rows.iter().map(line).collect()
}

#[test]
fn a_menu_reads_and_writes_as_its_file() {
    let app = game(&directory("menu-file"));
    let registry = app.world().resource::<CommandRegistry>().clone();
    let menu = registry.parse_menu(MAIN).unwrap();
    assert_eq!(menu.title, Some("Main".into()));
    assert!(menu.focus && menu.back);
    assert_eq!(menu.rows.len(), 4);
    assert_eq!(menu.rows[1].conditions(), [Condition::Flag("open".into())]);
    assert_eq!(registry.parse_menu(&menu_text(&menu)).unwrap(), menu);
    let hud = registry.parse_menu(HUD).unwrap();
    assert_eq!((hud.focus, hud.place), (false, crate::MenuPlace::TopLeft));
    assert_eq!(registry.parse_menu(&menu_text(&hud)).unwrap(), hud);
}

#[test]
fn menus_open_close_and_close_all_but_a_hud() {
    let commands = r#"OpenMenu("data/menus/main.menu.ron"), OpenMenu("data/menus/hud.menu.ron"),
        OpenMenu("data/menus/fixed.menu.ron"), OpenMenu("data/menus/main.menu.ron"), CloseMenu"#;
    let app = menu_game("menu-stack", commands);
    app.run(|mut app| {
        steps(&mut app, 3);
        // Main, the HUD, and the fixed menu: the last main one was closed.
        let rows: Vec<_> = menus(&app)
            .0
            .iter()
            .map(|open| open.menu.rows.len())
            .collect();
        assert_eq!(rows, [4, 1, 1]);
        start(&mut app, 12);
        // The HUD stays.
        assert_eq!(menus(&app).0.len(), 1);
        assert!(!menus(&app).0[0].menu.focus);
    });
}

#[test]
fn the_focus_skips_a_row_that_does_not_hold_and_a_pick_runs_a_command_a_routine_and_a_script() {
    let app = menu_game("menu-pick", r#"OpenMenu("data/menus/main.menu.ron")"#);
    app.run(|mut app| {
        steps(&mut app, 3);
        assert_eq!(rows(&app), ["> First", "Routine", "Script"]);
        tap(&mut app, Key::Z);
        steps(&mut app, 1);
        assert!(flag(&app, "first"));
        tap(&mut app, Key::ArrowDown);
        assert_eq!(rows(&app), ["First", "> Routine", "Script"]);
        tap(&mut app, Key::Z);
        steps(&mut app, 1);
        assert!(flag(&app, "routine"));
        tap(&mut app, Key::S);
        tap(&mut app, Key::Z);
        steps(&mut app, 1);
        // The function has the object that opened the menu.
        assert!(flag(&app, "script opener"));
        // The focus stops at the last row, and goes up over the hidden row.
        tap(&mut app, Key::ArrowDown);
        tap(&mut app, Key::ArrowUp);
        tap(&mut app, Key::ArrowUp);
        assert_eq!(rows(&app), ["> First", "Routine", "Script"]);
        // A row shows when its conditions hold.
        app.resource_mut::<GameState>().set_flag("open", true);
        tap(&mut app, Key::ArrowDown);
        assert_eq!(rows(&app), ["First", "> Hidden", "Routine", "Script"]);
        assert!(!flag(&app, "hidden"));
    });
}

#[test]
fn back_closes_a_menu_with_back_only() {
    let commands = r#"OpenMenu("data/menus/main.menu.ron"), OpenMenu("data/menus/fixed.menu.ron")"#;
    let app = menu_game("menu-back", commands);
    app.run(|mut app| {
        steps(&mut app, 3);
        tap(&mut app, Key::X);
        assert_eq!(menus(&app).0.len(), 2);
        app.resource_mut::<Menus>().close();
        tap(&mut app, Key::Backspace);
        assert!(menus(&app).0.is_empty());
    });
}

#[test]
fn a_menu_with_focus_blocks_the_walk_and_a_hud_does_not() {
    let app = menu_game("menu-walk", r#"OpenMenu("data/menus/hud.menu.ron")"#);
    app.run(|mut app| {
        steps(&mut app, 3);
        assert_eq!(menus(&app).0.len(), 1);
        assert!(!app.world().resource::<EventRunner>().blocks_player());
        // A HUD takes no input.
        tap(&mut app, Key::X);
        assert_eq!(menus(&app).0.len(), 1);
        let registry = app.world().resource::<CommandRegistry>().clone();
        let main = registry.parse_menu(MAIN).unwrap();
        app.resource_mut::<Menus>().open(main, None);
        steps(&mut app, 1);
        assert!(app.world().resource::<EventRunner>().blocks_player());
        tap(&mut app, Key::X);
        assert!(!app.world().resource::<EventRunner>().blocks_player());
    });
}

#[test]
fn a_message_over_a_menu_takes_the_input() {
    let commands = r#"OpenMenu("data/menus/main.menu.ron"), Message("Over")"#;
    let app = menu_game("menu-message", commands);
    app.run(|mut app| {
        steps(&mut app, 3);
        assert!(app.world().resource::<MessageBox>().0.is_some());
        tap(&mut app, Key::ArrowDown);
        tap(&mut app, Key::Z);
        assert!(app.world().resource::<MessageBox>().0.is_none());
        assert_eq!(rows(&app), ["> First", "Routine", "Script"]);
        steps(&mut app, 1);
        assert!(!flag(&app, "first"));
        tap(&mut app, Key::Z);
        steps(&mut app, 1);
        assert!(flag(&app, "first"));
    });
}

#[test]
fn a_script_opens_a_menu_that_it_makes_and_closes_it() {
    let mut app = menu_game("menu-script", r#"CallScript("make")"#);
    add_script_function(&mut app, "make", |context: &mut ScriptContext| {
        let close = context
            .command(MenuCommand::CloseMenu)
            .into_iter()
            .collect();
        let menu = Menu {
            title: Some("Made".into()),
            rows: vec![MenuRow::Commands {
                text: "Close".into(),
                conditions: Vec::new(),
                commands: close,
            }],
            ..Menu::default()
        };
        context.open_menu(menu);
    });
    app.run(|mut app| {
        steps(&mut app, 3);
        let shown = &app.world().resource::<ShownMenus>().0;
        assert_eq!(shown[0].title.as_deref(), Some("Made"));
        // Its row closes it.
        tap(&mut app, Key::Z);
        steps(&mut app, 1);
        assert!(menus(&app).0.is_empty());
        // A script function closes the top menu.
        let registry = app.world().resource::<CommandRegistry>().clone();
        let main = registry.parse_menu(MAIN).unwrap();
        app.resource_mut::<Menus>().open(main, None);
        start(&mut app, 11);
        assert!(menus(&app).0.is_empty());
    });
}

#[test]
fn a_menu_takes_its_input_in_a_pause() {
    let app = menu_game("menu-pause", r#"OpenMenu("data/menus/main.menu.ron")"#);
    app.run(|mut app| {
        steps(&mut app, 3);
        assert!(app.resource_mut::<PlayState>().change(PlayState::Pause));
        // The first frame of the pause.
        steps(&mut app, 1);
        tap(&mut app, Key::ArrowDown);
        tap(&mut app, Key::ArrowUp);
        tap(&mut app, Key::Z);
        // The row ran with no fixed step.
        assert!(flag(&app, "first"));
        // The press of the pause is not a press of the step after it.
        app.resource_mut::<PlayState>().change(PlayState::Play);
        app.resource_mut::<GameState>().set_flag("first", false);
        steps(&mut app, 2);
        assert!(!flag(&app, "first"));
    });
}

#[test]
fn the_focus_is_on_the_first_place_at_open_and_goes_to_the_nearest_place() {
    // The message over the menu has the input: the menu takes none before the mark shows.
    let commands = r#"OpenMenu("data/menus/head.menu.ron"), Message("Over")"#;
    let app = menu_game("menu-focus", commands);
    app.run(|mut app| {
        steps(&mut app, 3);
        assert_eq!(rows(&app), ["Head", "> A", "B", "E"]);
        tap(&mut app, Key::Z);
        app.resource_mut::<GameState>().set_flag("x", true);
        tap(&mut app, Key::ArrowDown);
        tap(&mut app, Key::ArrowDown);
        tap(&mut app, Key::ArrowDown);
        assert_eq!(rows(&app), ["Head", "A", "B", "C", "> D", "E"]);
        // The row of the focus stops to hold: E is nearer than B.
        app.resource_mut::<GameState>().set_flag("x", false);
        steps(&mut app, 2);
        assert_eq!(rows(&app), ["Head", "A", "B", "> E"]);
        tap(&mut app, Key::Z);
        steps(&mut app, 1);
        assert!(flag(&app, "e") && !flag(&app, "a"));
    });
}

#[test]
fn a_menu_with_no_place_of_focus_has_no_mark_and_a_pick_does_nothing() {
    let app = menu_game("menu-no-focus", r#"OpenMenu("data/menus/only.menu.ron")"#);
    app.run(|mut app| {
        steps(&mut app, 3);
        assert_eq!(rows(&app), ["Only"]);
        tap(&mut app, Key::Z);
        steps(&mut app, 1);
        assert_eq!(running(&app), 0);
        assert_eq!(menus(&app).0.len(), 1);
    });
}

#[test]
fn close_menu_closes_the_top_menu_with_focus_and_a_hud_when_no_menu_has_focus() {
    let commands = r#"OpenMenu("data/menus/main.menu.ron"), OpenMenu("data/menus/hud.menu.ron"),
        CloseMenu"#;
    let app = menu_game("menu-close", commands);
    app.run(|mut app| {
        steps(&mut app, 3);
        assert_eq!(menus(&app).0.len(), 1);
        assert!(!menus(&app).0[0].menu.focus);
        app.resource_mut::<Menus>().close();
        assert!(menus(&app).0.is_empty());
    });
}
