//! The fixes of the code review of stage 56.

use super::*;

/// The text of the check of a room with these events, after one update.
fn check_text(events: &str) -> String {
    let directory = directory("review-check");
    let mut app = game(&directory);
    app.update();
    let validators = app.world().resource::<maki_scene::RoomValidators>();
    let path = directory.join("a.ron");
    let text = match maki_scene::check_room(&path, &room(events), validators) {
        Ok(_) => String::new(),
        Err(error) => error.to_string(),
    };
    std::fs::remove_dir_all(directory).unwrap();
    text
}

#[test]
fn the_check_reports_a_condition_of_object_state_on_an_object_with_no_state() {
    // The lamp (2) has no `state`; 9 is not in the room; the event of 30 has no state.
    let page = r#"(name: "p", id: 30, pages: [(conditions: [Switch(2, "open")], trigger: Interact, commands: [])])"#;
    let text = check_text(page);
    assert!(text.contains("`id 2` has no component `state`"), "{text}");
    let row = r#"(name: "r", id: 30, pages: [(trigger: Interact, commands: [
        If([Any([NotSwitch(9, "open")])], [], []), WaitUntil([Number(0, "n", Equals, 1)])])])"#;
    let text = check_text(row);
    assert!(text.contains("the room has no object `id 9`"), "{text}");
    assert!(text.contains("`id 30` has no component `state`"), "{text}");
    let fine = r#"(name: "f", id: 30, pages: [(conditions: [Flag("a")], trigger: Interact, commands: [])])"#;
    assert_eq!(check_text(fine), "");
}

#[test]
fn reset_play_sets_object_state_and_the_random_numbers_back() {
    let directory = directory("review-reset");
    std::fs::write(directory.join("a.ron"), room("")).unwrap();
    game(&directory).run(|mut app| {
        steps(&mut app, 2);
        let first: Vec<i32> = {
            let mut random = app.resource_mut::<Random>();
            (0..8).map(|_| random.range(0, 1000)).collect()
        };
        let mut states = app.resource_mut::<ObjectStates>();
        states
            .get_mut("a.ron", ObjectId(9))
            .switches
            .insert("open".into(), true);
        app.reset_play();
        assert_eq!(
            *app.world().resource::<ObjectStates>(),
            ObjectStates::default()
        );
        let mut random = app.resource_mut::<Random>();
        let again: Vec<i32> = (0..8).map(|_| random.range(0, 1000)).collect();
        assert_eq!(first, again);
    });
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn hide_show_and_move_reach_the_children_of_the_object() {
    use bevy_ecs::query::Has;
    use maki_scene::Hidden;
    let directory = directory("review-children");
    let events = r#"
        (name: "hide", pages: [(trigger: Autorun, commands: [Hide(11)])]),
        (name: "move", pages: [(conditions: [Flag("move")], trigger: Autorun,
            commands: [Show(11), MoveObject(11, (0.0, 0.0, 2.0), 0.0)])])"#;
    let text = room(events).replacen(
        "objects: [",
        r#"objects: [(id: 11, name: "crate", pose: (position: (2, 0, 2)), components: {"prop": (mesh: Cube(1.0))}),
        (id: 14, parent: 11, pose: (position: (2, 0, 3)), components: {"prop": (mesh: Cube(1.0))}),
        (id: 15, parent: 14, pose: (position: (2, 0, 4)), components: {"prop": (mesh: Cube(1.0))}),"#,
        1,
    );
    std::fs::write(directory.join("a.ron"), text).unwrap();
    // Of each object 11, 14, 15: hidden, and the x of its pose.
    let objects = |app: &App| {
        let world = app.world();
        let mut query = world
            .try_query::<(&ObjectId, &Transform, Has<Hidden>)>()
            .unwrap();
        let mut found: Vec<_> = query
            .iter(world)
            .filter(|(id, ..)| [11, 14, 15].contains(&id.0))
            .map(|(id, pose, hidden)| (id.0, hidden, pose.translation.x))
            .collect();
        found.sort_by_key(|(id, ..)| *id);
        found
    };
    game(&directory).run(|mut app| {
        steps(&mut app, 3);
        let hidden: Vec<bool> = objects(&app).iter().map(|(_, hidden, _)| *hidden).collect();
        assert_eq!(hidden, [true; 3]);
        app.resource_mut::<GameState>().set_flag("move", true);
        steps(&mut app, 3);
        for (id, hidden, x) in objects(&app) {
            assert!(!hidden, "{id}");
            assert!(x < 0.6, "{id}: {x}");
        }
    });
    std::fs::remove_dir_all(directory).unwrap();
}

/// A game in a room with these events and the routine files `(name, text)`.
fn routine_game(test: &str, events: &str, routines: &[(&str, &str)]) -> (App, PathBuf) {
    let directory = directory(test);
    std::fs::write(directory.join("a.ron"), room(events)).unwrap();
    for (name, text) in routines {
        std::fs::write(directory.join(name), text).unwrap();
    }
    (game(&directory), directory)
}

#[test]
fn a_routine_that_calls_itself_stops_at_the_depth_limit() {
    let routine = r#"(commands: [AddVariable("depth", 1), CallRoutine("self.routine.ron", [])])"#;
    let events = r#"(name: "e", pages: [(trigger: Autorun, commands: [
        CallRoutine("self.routine.ron", []), SetFlag("done", true)])])"#;
    let (app, directory) = routine_game("review-depth", events, &[("self.routine.ron", routine)]);
    app.run(|mut app| {
        steps(&mut app, 10);
        assert_eq!(variable(&app, "depth"), 64);
        assert!(flag(&app, "done"));
    });
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_break_in_a_routine_does_not_leave_the_loop_of_the_caller() {
    let routine = r#"(commands: [AddVariable("in", 1), Break, AddVariable("after", 1)])"#;
    let events = r#"(name: "e", pages: [(trigger: Autorun, commands: [
        Loop([
            AddVariable("laps", 1),
            CallRoutine("break.routine.ron", []),
            If([Variable("laps", AtLeast, 3)], [Break], [])]),
        SetFlag("done", true)])])"#;
    let (app, directory) = routine_game("review-break", events, &[("break.routine.ron", routine)]);
    app.run(|mut app| {
        steps(&mut app, 4);
        assert!(flag(&app, "done"));
        let counts = ["laps", "in", "after"].map(|name| variable(&app, name));
        assert_eq!(counts, [3, 3, 3]);
    });
    std::fs::remove_dir_all(directory).unwrap();
}
