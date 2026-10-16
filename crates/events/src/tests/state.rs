//! Object state: switches and numbers of an object, by the room file and its id.

use super::*;

/// The test room with a chest (20) in front of the start: an interact opens it, by
/// "this object" (0). Its second page counts each time it is active after a load.
fn chest_room(events: &str) -> String {
    room(events).replacen(
        "objects: [",
        r#"objects: [(id: 20, name: "chest", pose: (position: (0, 0, 1.4)), components: {
            "area": (width: 1.0, depth: 1.0),
            "state": (switches: [(name: "open", on: false)], numbers: [(name: "coins", value: 5)]),
            "event": (pages: [
                (trigger: Interact, commands: [SetSwitch(0, "open", true), AddNumber(0, "coins", -2)]),
                (conditions: [Switch(0, "open")], trigger: Autorun, commands: [AddVariable("opened", 1)]),
            ])}),"#,
        1,
    )
}

/// The value of a switch and a number of the chest in room `a.ron`.
fn chest(app: &App, directory: &std::path::Path) -> (bool, i32) {
    let states = app.world().resource::<ObjectStates>();
    let values = states.get(&room_key(&directory.join("a.ron")), ObjectId(20));
    values.map_or((false, 0), |values| {
        (values.switch("open"), values.number("coins"))
    })
}

fn go_to(app: &mut App, path: PathBuf) {
    app.resource_mut::<RoomRequest>().0 = Some(RoomTarget {
        path,
        spawn: Some(ObjectId(3)),
        text: None,
    });
    steps(app, 3);
}

#[test]
fn a_chest_opens_by_an_interact_and_stays_open_over_a_change_of_room() {
    let directory = directory("state-chest");
    std::fs::write(directory.join("a.ron"), chest_room("")).unwrap();
    std::fs::write(directory.join("b.ron"), room("")).unwrap();
    let app = game(&directory);
    app.run(|mut app| {
        steps(&mut app, 2);
        // The first values of the component.
        assert_eq!(chest(&app, &directory), (false, 5));
        interact(&mut app);
        steps(&mut app, 6);
        assert_eq!(chest(&app, &directory), (true, 3));
        assert_eq!(app.world().resource::<GameState>().variable("opened"), 1);

        go_to(&mut app, directory.join("b.ron"));
        assert!(
            app.world()
                .resource::<CurrentRoom>()
                .path
                .ends_with("b.ron")
        );
        go_to(&mut app, directory.join("a.ron"));
        // Not the first values again: the state of the room stayed in the resource.
        assert_eq!(chest(&app, &directory), (true, 3));
        // The page of the open chest is active in the new load.
        assert_eq!(app.world().resource::<GameState>().variable("opened"), 2);
    });
    std::fs::remove_dir_all(directory).unwrap();
}

/// A script function reads and writes the state of the chest by its id.
fn peek(context: &mut ScriptContext) {
    let open = context.switch(ObjectId(20), "open");
    context.state().set_flag("saw_open", open);
    let coins = context.number(ObjectId(20), "coins");
    context.set_number(ObjectId(20), "coins", coins * 3);
    context.set_switch(ObjectId(20), "lid", true);
}

#[test]
fn a_script_and_the_conditions_read_the_state_of_an_object_by_id() {
    let directory = directory("state-script");
    let events = r#"
        (name: "open", pages: [(trigger: Autorun, commands: [SetSwitch(20, "open", true)])]),
        (name: "peek", pages: [(conditions: [Switch(20, "open")], trigger: Autorun,
            commands: [CallScript("peek")])]),
        (name: "watch", pages: [
            (conditions: [Number(20, "coins", Equals, 15), NotSwitch(20, "shut")],
                trigger: Autorun, commands: [SetFlag("fifteen", true)]),
        ])"#;
    std::fs::write(directory.join("a.ron"), chest_room(events)).unwrap();
    let mut app = game(&directory);
    add_script_function(&mut app, "peek", peek);
    app.run(|mut app| {
        steps(&mut app, 20);
        assert!(flag(&app, "saw_open") && flag(&app, "fifteen"));
        assert_eq!(chest(&app, &directory), (true, 15));
        let states = app.world().resource::<ObjectStates>();
        let room = room_key(&directory.join("a.ron"));
        assert!(states.get(&room, ObjectId(20)).unwrap().switch("lid"));
    });
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn the_check_reports_state_with_no_id_and_a_row_on_an_object_with_no_state() {
    let directory = directory("state-check");
    let events = r#"(name: "e", pages: [(trigger: Autorun, commands: [
        SetSwitch(2, "open", true), SetSwitch(0, "open", true)])])"#;
    let path = directory.join("a.ron");
    let mut app = game(&directory);
    app.update();
    let validators = app.world().resource::<maki_scene::RoomValidators>();
    // The lamp (2) has no `state`; object 0 is the object of the event.
    let error = maki_scene::check_room(&path, &room(events), validators).unwrap_err();
    let error = error.to_string();
    assert!(error.contains("`id 2` has no component `state`"), "{error}");
    assert_eq!(error.lines().count(), 1, "{error}");
    let no_id = room(r#"(name: "e", pages: [])"#).replacen(
        "objects: [",
        r#"objects: [(name: "box", components: {"state": ()}),"#,
        1,
    );
    let error = maki_scene::check_room(&path, &no_id, validators).unwrap_err();
    assert!(error.to_string().contains("needs an object with an id"));
    std::fs::remove_dir_all(directory).unwrap();
}
