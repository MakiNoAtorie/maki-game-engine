//! Text tables: the texts of the game in its language.

use bevy_ecs::message::Messages;
use maki_assets::AssetChanged;

use super::*;
use crate::{Texts, text, texts};

/// The keys that the next update looks up, and their texts after it.
#[derive(bevy_ecs::resource::Resource, Default)]
struct Ask(Vec<String>, Vec<String>, String);

/// A game of the tests with the watch messages and a system that looks up `Ask`.
fn text_game(directory: &std::path::Path) -> App {
    let mut app = game(directory);
    app.add_module(maki_assets::AssetsModule)
        .init_resource::<Ask>()
        // In `Present`: an exclusive system in `Update` is in no order with the others.
        .add_systems(maki_core::Present, |world: &mut World| {
            let keys = std::mem::take(&mut world.resource_mut::<Ask>().0);
            let found: Vec<String> = keys.iter().map(|key| text(world, key, |_| 0)).collect();
            let language = texts(world).language().to_owned();
            let mut ask = world.resource_mut::<Ask>();
            (ask.1, ask.2) = (found, language);
        });
    app
}

fn table(directory: &std::path::Path, language: &str, texts: &[(&str, &str)]) {
    let entries: Vec<String> = texts
        .iter()
        .map(|(key, text)| format!("(key: {key:?}, text: {text:?})"))
        .collect();
    let file = directory.join(format!("data/text/{language}.text.ron"));
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let content = format!("(language: {language:?}, texts: [{}])", entries.join(", "));
    std::fs::write(file, content).unwrap();
}

fn get(app: &mut App, key: &str) -> String {
    app.resource_mut::<Ask>().0 = vec![key.to_owned()];
    app.update();
    app.world().resource::<Ask>().1[0].clone()
}

#[test]
fn a_key_shows_the_text_of_the_language_then_of_the_fallback_then_itself() {
    let directory = directory("text-lookup");
    table(
        &directory,
        "en",
        &[("door.locked", "Locked."), ("only.en", "English.")],
    );
    table(&directory, "ja", &[("door.locked", "Shimatte iru.")]);
    std::fs::write(directory.join("a.ron"), room("")).unwrap();
    let app = text_game(&directory);
    app.run(|mut app| {
        steps(&mut app, 2);
        assert_eq!(get(&mut app, "door.locked"), "Locked.");
        app.resource_mut::<Texts>().set_language("ja");
        assert_eq!(get(&mut app, "door.locked"), "Shimatte iru.");
        // A key that only the fallback has.
        assert_eq!(get(&mut app, "only.en"), "English.");
        // A key that no table has: the key itself, a room file of before still plays.
        assert_eq!(get(&mut app, "The lamp hums."), "The lamp hums.");
        assert_eq!(app.world().resource::<Ask>().2, "ja");
    });
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_changed_table_shows_its_new_text() {
    let directory = directory("text-changed");
    table(&directory, "en", &[("door.locked", "Locked.")]);
    std::fs::write(directory.join("a.ron"), room("")).unwrap();
    let app = text_game(&directory);
    app.run(|mut app| {
        steps(&mut app, 2);
        assert_eq!(get(&mut app, "door.locked"), "Locked.");
        table(&directory, "en", &[("door.locked", "Shut tight.")]);
        // No change of the texts until the watch tells of the file.
        assert_eq!(get(&mut app, "door.locked"), "Locked.");
        let path = directory.join("data/text/en.text.ron");
        app.resource_mut::<Messages<AssetChanged>>()
            .write(AssetChanged(path));
        app.update();
        assert_eq!(get(&mut app, "door.locked"), "Shut tight.");
    });
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn an_empty_text_keeps_the_fallback_and_a_new_table_shows_with_the_watch() {
    let directory = directory("text-new-table");
    table(&directory, "en", &[("door.locked", "Locked.")]);
    std::fs::write(directory.join("a.ron"), room("")).unwrap();
    let mut app = text_game(&directory);
    app.resource_mut::<maki_assets::AssetWatch>().on = true;
    app.run(|mut app| {
        steps(&mut app, 2);
        app.resource_mut::<Texts>().set_language("ja");
        assert_eq!(get(&mut app, "door.locked"), "Locked.");
        // A new table file, as "New language" writes it: no room load, no watch message.
        std::thread::sleep(std::time::Duration::from_millis(20));
        table(&directory, "ja", &[("door.locked", "")]);
        app.update();
        // Its text is empty: not yet written, so the fallback shows.
        assert_eq!(get(&mut app, "door.locked"), "Locked.");
        table(&directory, "ja", &[("door.locked", "Shimatte iru.")]);
        let path = directory.join("data/text/ja.text.ron");
        app.resource_mut::<Messages<AssetChanged>>()
            .write(AssetChanged(path));
        app.update();
        assert_eq!(get(&mut app, "door.locked"), "Shimatte iru.");
        // A stop and a play: the language of the settings again.
        app.reset_play();
        assert_eq!(get(&mut app, "door.locked"), "Locked.");
    });
    std::fs::remove_dir_all(directory).unwrap();
}
