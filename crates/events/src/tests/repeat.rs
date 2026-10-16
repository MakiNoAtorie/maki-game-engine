//! An autorun page with `repeat` runs again while it is the active page.

use super::*;

/// Runs a room with these events for 30 steps and gives the variable "n".
fn count(events: &str, name: &str) -> i32 {
    let directory = directory(name);
    std::fs::write(directory.join("a.ron"), room(events)).unwrap();
    let app = game(&directory);
    let mut n = 0;
    app.run(|mut app| {
        steps(&mut app, 30);
        n = app.world().resource::<GameState>().variable("n");
    });
    std::fs::remove_dir_all(directory).unwrap();
    n
}

#[test]
fn a_repeat_page_runs_again_and_a_plain_autorun_runs_once() {
    let again = r#"(name: "e", pages: [(trigger: Autorun, repeat: true, commands: [AddVariable("n", 1), Wait(0.1)])])"#;
    let once =
        r#"(name: "e", pages: [(trigger: Autorun, commands: [AddVariable("n", 1), Wait(0.1)])])"#;
    assert!(count(again, "repeat-on") >= 3);
    assert_eq!(count(once, "repeat-off"), 1);
}

#[test]
fn a_repeat_page_with_no_wait_runs_at_most_once_for_each_step() {
    let events = r#"(name: "e", pages: [(trigger: Autorun, repeat: true, commands: [AddVariable("n", 1)])])"#;
    let n = count(events, "repeat-nowait");
    // The test ends in 30 steps: a run that held the frame would not end it.
    assert!((10..=30).contains(&n), "{n}");
}

#[test]
fn a_repeat_page_stops_when_another_page_is_active() {
    let events = r#"(name: "e", pages: [
        (trigger: Autorun, repeat: true, commands: [AddVariable("n", 1), Wait(0.1)]),
        (conditions: [Flag("stop")], trigger: Interact)])"#;
    let directory = directory("repeat-stop");
    std::fs::write(directory.join("a.ron"), room(events)).unwrap();
    let app = game(&directory);
    app.run(|mut app| {
        steps(&mut app, 20);
        app.resource_mut::<GameState>().set_flag("stop", true);
        // The run in progress ends; no new run starts.
        steps(&mut app, 10);
        let n = app.world().resource::<GameState>().variable("n");
        steps(&mut app, 30);
        assert_eq!(app.world().resource::<GameState>().variable("n"), n);
    });
    std::fs::remove_dir_all(directory).unwrap();
}
