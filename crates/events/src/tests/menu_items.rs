//! The rows `Items` and `Text` of a menu: the list of the items, a pick, and a HUD.

use maki_assets::FileId;

use super::*;
use crate::{Menus, ShownMenus, ShownRow};

const COIN: FileId = FileId(0xc01);
const KEY: FileId = FileId(0xe01);
const GEM: FileId = FileId(0x6e1);

const ITEMS: &str = r#"(title: Some("Items"), rows: [Text("Coins: {coins}"), Items])"#;
const HUD: &str = r#"(focus: false, place: TopLeft, rows: [Text("Coins: {coins}")])"#;

/// A game with three item files (the key runs a routine), and the inventory of a
/// player who has 15 coins, 2 keys, and no gem.
fn item_game(test: &str) -> App {
    let directory = directory(test);
    std::fs::create_dir_all(directory.join("data")).unwrap();
    let kinds = [
        ("coin", COIN, r#"(name: "Coin", description: "Round.")"#),
        (
            "key",
            KEY,
            r#"(name: "Key", description: "Opens the door.", use_routine: "r.routine.ron")"#,
        ),
        ("gem", GEM, r#"(name: "Gem", description: "Rare.")"#),
    ];
    for (name, id, text) in kinds {
        let file = directory.join(format!("data/{name}.item.ron"));
        std::fs::write(&file, text).unwrap();
        id.write(&file).unwrap();
    }
    let routine = r#"(commands: [SetFlag("used", true)])"#;
    std::fs::write(directory.join("r.routine.ron"), routine).unwrap();
    std::fs::write(directory.join("a.ron"), room("")).unwrap();
    game(&directory)
}

fn open(app: &mut App, text: &str) {
    let registry = app.world().resource::<CommandRegistry>().clone();
    let menu = registry.parse_menu(text).unwrap();
    app.resource_mut::<Menus>().open(menu, None);
}

fn give(app: &mut App) {
    let mut inventory = app.resource_mut::<Inventory>();
    inventory.add(COIN, 15);
    inventory.add(KEY, 2);
    inventory.add(GEM, 1);
    inventory.add(GEM, -1);
}

/// The lines of the top menu: a mark at the focus, the count of an item line.
fn lines(app: &App) -> Vec<String> {
    let shown = &app.world().resource::<ShownMenus>().0;
    let line = |row: &ShownRow| {
        let count = row
            .count
            .map_or(String::new(), |count| format!(" x{count}"));
        format!("{}{}{count}", if row.focus { "> " } else { "" }, row.text)
    };
    shown.last().unwrap().rows.iter().map(line).collect()
}

fn tap(app: &mut App, key: Key) {
    app.write_message(RawInput::KeyDown(key))
        .write_message(RawInput::KeyUp(key));
    steps(app, 2);
}

#[test]
fn the_items_row_lists_each_item_with_a_count_by_path_and_the_description_of_the_focus() {
    item_game("items-list").run(|mut app| {
        give(&mut app);
        open(&mut app, ITEMS);
        steps(&mut app, 3);
        // By path: coin, key. The gem has 0. The focus skips the text line.
        assert_eq!(lines(&app), ["Coins: 0", "> Coin x15", "Key x2"]);
        let shown = &app.world().resource::<ShownMenus>().0;
        assert_eq!(shown[0].description.as_deref(), Some("Round."));
        tap(&mut app, Key::ArrowDown);
        assert_eq!(lines(&app), ["Coins: 0", "Coin x15", "> Key x2"]);
        let shown = &app.world().resource::<ShownMenus>().0;
        assert_eq!(shown[0].description.as_deref(), Some("Opens the door."));
    });
}

#[test]
fn a_pick_of_an_item_runs_its_routine_and_an_item_with_none_runs_nothing() {
    item_game("items-pick").run(|mut app| {
        give(&mut app);
        open(&mut app, ITEMS);
        steps(&mut app, 3);
        tap(&mut app, Key::Z);
        steps(&mut app, 2);
        assert!(!flag(&app, "used"));
        tap(&mut app, Key::ArrowDown);
        tap(&mut app, Key::Z);
        steps(&mut app, 2);
        assert!(flag(&app, "used"));
    });
}

#[test]
fn a_text_row_fills_a_variable_and_a_hud_takes_no_input_and_does_not_block_the_walk() {
    item_game("items-hud").run(|mut app| {
        app.resource_mut::<GameState>()
            .variables
            .insert("coins".into(), 7);
        open(&mut app, HUD);
        steps(&mut app, 3);
        assert_eq!(lines(&app), ["Coins: 7"]);
        assert!(!app.world().resource::<EventRunner>().blocks_player());
        tap(&mut app, Key::X);
        tap(&mut app, Key::Z);
        assert_eq!(app.world().resource::<Menus>().0.len(), 1);
        assert_eq!(app.world().resource::<Menus>().0[0].row, 0);
    });
}
