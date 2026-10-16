//! A random number in its range, from a seed; a compare of two variables.

use super::*;
use crate::{Compare, Condition, Random};

#[test]
fn a_random_number_is_in_its_range_and_a_seed_gives_the_same_numbers() {
    let draw = |seed| {
        let mut random = Random::new(seed);
        (0..1000).map(|_| random.range(-2, 3)).collect::<Vec<_>>()
    };
    let numbers = draw(7);
    assert!(numbers.iter().all(|n| (-2..=3).contains(n)));
    // Both ends are drawn.
    assert!(numbers.contains(&-2) && numbers.contains(&3));
    assert_eq!(numbers, draw(7));
    assert_ne!(numbers, draw(8));
    assert_eq!(Random::new(1).range(5, 5), 5);
    // A wrong order and the full range do not panic.
    assert!((1..=2).contains(&Random::new(1).range(2, 1)));
    let _ = Random::new(0).range(i32::MIN, i32::MAX);
}

#[test]
fn the_command_sets_a_variable_in_its_range() {
    let events = r#"(name: "e", pages: [(trigger: Autorun, commands: [
        RandomVariable("r", 3, 5), RandomVariable("one", 7, 7)])])"#;
    let directory = directory("numbers-command");
    std::fs::write(directory.join("a.ron"), room(events)).unwrap();
    let app = game(&directory);
    app.run(|mut app| {
        steps(&mut app, 5);
        let state = app.world().resource::<GameState>();
        assert!((3..=5).contains(&state.variable("r")));
        assert_eq!(state.variable("one"), 7);
    });
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_compare_of_two_variables_holds_as_it_says() {
    let mut state = GameState::default();
    state.set_variable("a", 2);
    state.set_variable("b", 5);
    let holds = |state: &GameState, compare| {
        Condition::Variables("a".into(), compare, "b".into()).holds(&crate::Facts::of(state))
    };
    assert!(!holds(&state, Compare::Equals));
    assert!(holds(&state, Compare::NotEquals));
    assert!(holds(&state, Compare::Less));
    assert!(!holds(&state, Compare::AtLeast));
    state.set_variable("a", 5);
    assert!(holds(&state, Compare::Equals) && holds(&state, Compare::AtLeast));
    let condition = Condition::Variables("a".into(), Compare::Less, "b".into());
    let names = condition.names();
    assert_eq!(names, vec![(true, "a"), (true, "b")]);
}
