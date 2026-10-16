//! The flow of an event: "any of", a loop and its break, "wait until".

use super::*;

#[test]
fn any_of_holds_when_one_of_its_conditions_holds() {
    let directory = directory("any-of");
    let events = r#"(name: "or", pages: [(trigger: Autorun, commands: [
        SetFlag("b", true),
        If([Any([Flag("a"), Flag("b")])], [SetVariable("one", 1)], [SetVariable("one", 2)]),
        If([Any([Flag("a"), Variable("one", Equals, 5)])], [SetVariable("none", 1)], [SetVariable("none", 2)]),
        If([Any([])], [SetVariable("empty", 1)], [SetVariable("empty", 2)])])])"#;
    std::fs::write(directory.join("a.ron"), room(events)).unwrap();
    game(&directory).run(|mut app| {
        steps(&mut app, 4);
        assert_eq!(variable(&app, "one"), 1);
        assert_eq!(variable(&app, "none"), 2);
        // An "any of" with no condition does not hold.
        assert_eq!(variable(&app, "empty"), 2);
    });
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_loop_with_a_break_ends_and_one_with_no_wait_goes_on_over_steps() {
    let directory = directory("loop");
    let events = r#"
        (name: "counted", pages: [(trigger: Autorun, commands: [
            Loop([
                AddVariable("count", 1),
                If([Variable("count", AtLeast, 3)], [Break], []),
                AddVariable("after", 1)]),
            SetFlag("done", true)])]),
        (name: "busy", pages: [(trigger: Parallel, commands: [
            Loop([AddVariable("spins", 1)])])])"#;
    std::fs::write(directory.join("a.ron"), room(events)).unwrap();
    game(&directory).run(|mut app| {
        steps(&mut app, 3);
        // The break left the loop and its `If`: the rows after the loop ran.
        assert_eq!(variable(&app, "count"), 3);
        assert_eq!(variable(&app, "after"), 2);
        assert!(app.world().resource::<GameState>().flag("done"));
        // A loop with no wait stops at the step limit and goes on in the next step.
        let spins = variable(&app, "spins");
        assert!(spins > 0);
        steps(&mut app, 2);
        assert_eq!(app.world().resource::<maki_core::Time>().fixed_tick, 5);
        assert!(variable(&app, "spins") > spins);
    });
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn wait_until_goes_on_when_its_conditions_hold() {
    let directory = directory("wait-until");
    let events = r#"
        (name: "waits", pages: [(trigger: Autorun, commands: [
            WaitUntil([Variable("ticks", AtLeast, 5)]), SetFlag("done", true)])]),
        (name: "clock", pages: [(trigger: Parallel, commands: [AddVariable("ticks", 1)])])"#;
    std::fs::write(directory.join("a.ron"), room(events)).unwrap();
    game(&directory).run(|mut app| {
        let done = |app: &maki_core::App| app.world().resource::<GameState>().flag("done");
        steps(&mut app, 4);
        assert!(variable(&app, "ticks") < 5 && !done(&app));
        steps(&mut app, 4);
        assert!(variable(&app, "ticks") >= 5 && done(&app));
    });
    std::fs::remove_dir_all(directory).unwrap();
}
