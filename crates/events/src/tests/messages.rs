//! Messages, questions, items, and scripts by text key.

use super::*;
use crate::{TextEntry, TextKey, TextTable};

/// Writes a text table `data/text/<file>.text.ron` of the language.
fn table(directory: &std::path::Path, file: &str, language: &str, texts: &[(&str, &str)]) {
    let table = TextTable {
        language: language.to_owned(),
        texts: texts
            .iter()
            .map(|(key, text)| TextEntry {
                key: (*key).to_owned(),
                text: (*text).to_owned(),
            })
            .collect(),
    };
    let path = directory.join(format!("data/text/{file}.text.ron"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, maki_assets::data_text(&table)).unwrap();
}

fn message(app: &App) -> Option<String> {
    app.world().resource::<MessageBox>().0.clone()
}

fn answers(app: &App) -> Vec<String> {
    let choice = app.world().resource::<ChoiceBox>().0.clone();
    choice.map(|choice| choice.answers).unwrap_or_default()
}

#[test]
fn a_message_and_a_question_show_the_texts_of_their_keys_in_the_language() {
    let directory = directory("messages-by-key");
    let en = [
        ("door.locked", "Locked."),
        ("door.ask", "Open it?"),
        ("yes", "Yes"),
        ("no", "No"),
    ];
    table(&directory, "en", "en", &en);
    let ja = [
        ("door.locked", "Shimatte iru."),
        ("door.ask", "Akeru?"),
        ("yes", "Hai"),
    ];
    table(&directory, "ja", "ja", &ja);
    let events = r#"(name: "door", pages: [(trigger: Autorun, commands: [
        Message("door.locked"), Choice("door.ask", [("yes", []), ("no", [])]),
        Message("door.locked"), Choice("door.ask", [("yes", []), ("no", [])]),
        Message("The lamp hums.")])])"#;
    std::fs::write(directory.join("a.ron"), room(events)).unwrap();
    let app = game(&directory);
    app.run(|mut app| {
        steps(&mut app, 3);
        assert_eq!(message(&app).as_deref(), Some("Locked."));
        interact(&mut app);
        assert_eq!(message(&app).as_deref(), Some("Open it?"));
        assert_eq!(answers(&app), ["Yes", "No"]);
        // The texts of a row are looked up when it starts.
        app.resource_mut::<Texts>().set_language("ja");
        steps(&mut app, 1);
        interact(&mut app);
        assert_eq!(message(&app).as_deref(), Some("Shimatte iru."));
        steps(&mut app, 1);
        interact(&mut app);
        assert_eq!(message(&app).as_deref(), Some("Akeru?"));
        // `ja` has no text of `no`: the text of the fallback.
        assert_eq!(answers(&app), ["Hai", "No"]);
        steps(&mut app, 1);
        interact(&mut app);
        // A text of a room file of before, in place of a key: the text shows.
        assert_eq!(message(&app).as_deref(), Some("The lamp hums."));
    });
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_name_in_braces_shows_a_game_variable_or_an_argument_of_a_routine() {
    let directory = directory("messages-variables");
    table(
        &directory,
        "en",
        "en",
        &[("coins", "You have {coins} coins.")],
    );
    let routine = r#"(arguments: ["coins"], commands: [Message("coins")])"#;
    std::fs::create_dir_all(directory.join("routines")).unwrap();
    std::fs::write(directory.join("routines/say.routine.ron"), routine).unwrap();
    let events = r#"(name: "count", pages: [(trigger: Autorun, commands: [
        SetVariable("coins", 7), Message("coins"),
        CallRoutine("routines/say.routine.ron", [3])])])"#;
    std::fs::write(directory.join("a.ron"), room(events)).unwrap();
    let app = game(&directory);
    app.run(|mut app| {
        steps(&mut app, 3);
        assert_eq!(message(&app).as_deref(), Some("You have 7 coins."));
        interact(&mut app);
        steps(&mut app, 1);
        // The argument hides the game variable of its name.
        assert_eq!(message(&app).as_deref(), Some("You have 3 coins."));
    });
    std::fs::remove_dir_all(directory).unwrap();
}

/// What the script function of the test found.
#[derive(bevy_ecs::resource::Resource, Default)]
struct Said(Vec<String>);

#[test]
fn a_script_gets_a_text_by_key_changes_the_language_and_names_an_item_by_key() {
    let directory = directory("messages-script");
    table(
        &directory,
        "en",
        "en",
        &[("hello", "Hello, {visits} visits."), ("key.name", "Key")],
    );
    table(
        &directory,
        "ja",
        "ja",
        &[("hello", "Konnichiwa, {visits}.")],
    );
    let item = crate::ItemKind {
        name: "key.name".into(),
        description: "key.description".into(),
        ..Default::default()
    };
    std::fs::write(
        directory.join("data/key.item.ron"),
        maki_assets::data_text(&item),
    )
    .unwrap();
    let events = r#"(name: "speak", pages: [(trigger: Autorun, commands: [
        SetVariable("visits", 2), CallScript("speak")])])"#;
    std::fs::write(directory.join("a.ron"), room(events)).unwrap();
    let mut app = game(&directory);
    app.init_resource::<Said>();
    add_script_function(&mut app, "speak", |context: &mut ScriptContext| {
        let mut said = vec![context.text("hello")];
        context.set_language("ja");
        said.push(context.text("hello"));
        let kind = context
            .data::<crate::ItemKind>("data/key.item.ron")
            .unwrap();
        assert_eq!(kind.name, TextKey::from("key.name"));
        // `ja` has no text of the name: the fallback.
        said.push(context.text(&kind.name.0));
        // No table has the description: the key.
        said.push(context.text(&kind.description.0));
        context.world.resource_mut::<Said>().0 = said;
    });
    app.run(|mut app| {
        steps(&mut app, 4);
        let said = &app.world().resource::<Said>().0;
        let expected = [
            "Hello, 2 visits.",
            "Konnichiwa, 2.",
            "Key",
            "key.description",
        ];
        assert_eq!(said, &expected);
        assert_eq!(app.world().resource::<Texts>().language(), "ja");
    });
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn the_check_warns_of_a_key_with_no_text_and_finds_a_wrong_table() {
    let directory = directory("messages-check");
    let path = directory.join("a.ron");
    table(
        &directory,
        "en",
        "en",
        &[("known", "Known."), ("yes", "Yes")],
    );
    // Only the fallback counts: `ja` has the key `ja.only`, `en` does not.
    table(&directory, "ja", "ja", &[("ja.only", "Hai")]);
    let app = game(&directory);
    let registry = app.world().resource::<CommandRegistry>().clone();
    let validators = app.world().resource::<maki_scene::RoomValidators>();
    let events = r#"(name: "a", pages: [(trigger: Interact, commands: [
        Message("known"), Message("ja.only"), Choice("known", [("yes", []), ("maybe", [])])])])"#;
    let text = room(events);
    // Warnings: the room loads.
    assert!(maki_scene::check_room(&path, &text, validators).is_ok());
    let def = maki_scene::parse_room(&text).unwrap();
    let context = CheckContext {
        room: &def,
        file: "a.ron",
        directory: &directory,
        spawn: &|_, _| None,
        registry: &registry,
    };
    let problems = registry.check_events(&registry.parse_events(&text).unwrap(), &context);
    let found: Vec<(bool, &str)> = problems
        .iter()
        .map(|problem| (problem.warning, problem.text.as_str()))
        .collect();
    assert_eq!(
        found,
        [
            (
                true,
                "the language `en` has no text of the key `ja.only`: the key shows"
            ),
            (
                true,
                "the language `en` has no text of the key `maybe`: the key shows"
            ),
        ]
    );

    // A key in two tables of one language, and a table with no language: errors.
    table(&directory, "more", "en", &[("known", "Again.")]);
    table(&directory, "none", "", &[]);
    let error = maki_scene::check_room(&path, &text, validators)
        .unwrap_err()
        .to_string();
    let wrong = |file: &str| format!("data{0}text{0}{file}", std::path::MAIN_SEPARATOR);
    let two = format!(
        "the key `known` of the language `en` is in `{}` and in `{}`",
        wrong("en.text.ron"),
        wrong("more.text.ron")
    );
    assert!(error.contains(&two), "{error}");
    let none = format!(
        "the text table `{}` has no language",
        wrong("none.text.ron")
    );
    assert!(error.contains(&none), "{error}");
    std::fs::remove_dir_all(directory).unwrap();
}

/// The messages of the rooms and of the beacon of `game/` are keys of
/// `data/text/en.text.ron`, with the texts of before, letter by letter: the pictures
/// of the trip show them.
#[test]
fn the_messages_of_the_game_are_the_texts_of_before() {
    let rooms = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../game/assets/rooms");
    let table: TextTable = maki_assets::read_data(&rooms.join("data/text/en.text.ron")).unwrap();
    let text = |key: &str| {
        let entry = table.texts.iter().find(|entry| entry.key == key);
        entry.map(|entry| crate::fill(&entry.text, |_| 4))
    };
    let before = [
        (
            "room_a.ron",
            "room_a.door_locked",
            "The door is locked. Something hums behind it.",
        ),
        (
            "room_a.ron",
            "room_a.door_opens",
            "The lock is dead. The door gives way.",
        ),
        (
            "room_b.ron",
            "room_b.switch_off",
            "Click. The lamp dies. Far away, a lock lets go.",
        ),
        (
            "room_b.ron",
            "room_b.switch_stuck",
            "The switch does not move.",
        ),
        (
            "room_b.ron",
            "room_b.ball_rings",
            "The ball rings. Something under the floor answers.",
        ),
    ];
    for (room, key, old) in before {
        let file = std::fs::read_to_string(rooms.join(room)).unwrap();
        assert!(file.contains(&format!("Message({key:?})")), "{room}: {key}");
        assert_eq!(text(key).as_deref(), Some(old), "{key}");
    }
    // The beacon: the count is the variable `beacon_touches`.
    let touched = text("beacon.touched");
    assert_eq!(touched.as_deref(), Some("The lamp hums. Touched 4 times."));
    let forget = text("beacon.forget");
    assert_eq!(forget.as_deref(), Some("dial turns. The lamp forgets."));
}
