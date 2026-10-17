//! Item kinds and the inventory.

use maki_assets::FileId;

use super::*;

const KEY: &str = "data/key.item.ron";
const KEY_ID: FileId = FileId(0x1234);

/// A directory with the item file of the key and its id file.
fn items_directory(test: &str) -> PathBuf {
    let directory = directory(&format!("items-{test}"));
    std::fs::create_dir_all(directory.join("data")).unwrap();
    let key = directory.join(KEY);
    std::fs::write(&key, r#"(name: "key", description: "Opens the door.")"#).unwrap();
    KEY_ID.write(&key).unwrap();
    directory
}

fn keys(app: &App) -> i32 {
    app.world().resource::<Inventory>().count(KEY_ID)
}

fn door(app: &App) -> MoverState {
    let world = app.world();
    let mut movers = world.try_query::<&Mover>().unwrap();
    movers.single(world).unwrap().state
}

#[test]
fn a_given_key_opens_the_door_of_an_item_condition() {
    let directory = items_directory("door");
    let events = format!(
        r#"(name: "give", pages: [(conditions: [Flag("go")], trigger: Autorun, commands: [GiveItem("{KEY}", 1)])]),
        (name: "gate", pages: [(conditions: [Item("{KEY}", AtLeast, 1)], trigger: Autorun, commands: [OpenDoor(1)])])"#
    );
    std::fs::write(directory.join("a.ron"), room(&events)).unwrap();
    let app = game(&directory);
    app.run(|mut app| {
        steps(&mut app, 4);
        assert_eq!(door(&app), MoverState::AtA);
        app.resource_mut::<GameState>().set_flag("go", true);
        steps(&mut app, 12);
        assert_eq!(keys(&app), 1);
        assert_eq!(door(&app), MoverState::AtB);
    });
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn take_does_not_go_below_0_and_the_inventory_stays_over_a_change_of_room() {
    let directory = items_directory("take");
    let events = format!(
        r#"(name: "trade", pages: [(trigger: Autorun, commands: [
            GiveItem("{KEY}", 3), TakeItem("{KEY}", 5), GiveItem("{KEY}", 1),
            Transfer("b.ron", 3)])])"#
    );
    std::fs::write(directory.join("a.ron"), room(&events)).unwrap();
    let give =
        format!(r#"(name: "more", pages: [(trigger: Autorun, commands: [GiveItem("{KEY}", 2)])])"#);
    std::fs::write(directory.join("b.ron"), room(&give)).unwrap();
    let app = game(&directory);
    app.run(|mut app| {
        steps(&mut app, 12);
        // 3 - 5: 0, not -2. Then 1 in room A and 2 in room B: 3.
        assert!(
            app.world()
                .resource::<CurrentRoom>()
                .path
                .ends_with("b.ron")
        );
        assert_eq!(keys(&app), 3);
        app.reset_play();
        assert_eq!(*app.world().resource::<Inventory>(), Inventory::default());
    });
    let mut inventory = Inventory::default();
    inventory.add(KEY_ID, 1);
    inventory.add(KEY_ID, -2);
    assert_eq!(inventory, Inventory::default());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn the_check_reports_a_missing_item_file_a_file_that_is_no_item_and_no_id() {
    let directory = items_directory("check");
    std::fs::write(directory.join("data/coin.item.ron"), "()").unwrap();
    std::fs::write(directory.join("data/bad.item.ron"), "(weight: 3)").unwrap();
    FileId(7)
        .write(&directory.join("data/bad.item.ron"))
        .unwrap();
    let mut app = App::new(ConfigFile::default()).unwrap();
    app.add_module(PlatformModule)
        .add_module(SceneModule)
        .add_module(EventsModule);
    app.update();
    let validators = app.world().resource::<maki_scene::RoomValidators>();
    let events = format!(
        r#"(name: "a", pages: [(conditions: [Item("data/gone.item.ron", AtLeast, 1)], trigger: Interact, commands: [
            GiveItem("{KEY}", 1), TakeItem("data/coin.item.ron", 1), GiveItem("data/bad.item.ron", 1),
            If([Item("{KEY}", Less, 2)], [], [])])])"#
    );
    let error = maki_scene::check_room(&directory.join("a.ron"), &room(&events), validators)
        .unwrap_err()
        .to_string();
    for wanted in [
        "no item file `data/gone.item.ron`",
        "the item file `data/coin.item.ron` has no id",
        "bad.item.ron:1:",
    ] {
        assert!(error.contains(wanted), "{wanted}: {error}");
    }
    assert_eq!(error.lines().count(), 3, "{error}");
    std::fs::remove_dir_all(directory).unwrap();
}

/// A script function: the variable `seen` gets the count of keys, then the script
/// gives 4 more.
fn count_keys(context: &mut ScriptContext) {
    let key = context.item(KEY).unwrap();
    let count = context.items().count(key);
    context.state().set_variable("seen", count);
    context.items().add(key, 4);
}

#[test]
fn a_script_reads_and_changes_the_inventory() {
    let directory = items_directory("script");
    let events = format!(
        r#"(name: "a", pages: [(trigger: Autorun, commands: [GiveItem("{KEY}", 1), CallScript("count")])])"#
    );
    std::fs::write(directory.join("a.ron"), room(&events)).unwrap();
    let mut app = game(&directory);
    add_script_function(&mut app, "count", count_keys);
    app.run(|mut app| {
        steps(&mut app, 4);
        assert_eq!(app.world().resource::<GameState>().variable("seen"), 1);
        assert_eq!(keys(&app), 5);
    });
    std::fs::remove_dir_all(directory).unwrap();
}
