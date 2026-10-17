//! Data assets read by a script.

use maki_assets::{DataAsset, add_data_type};
use serde::{Deserialize, Serialize};

use super::*;

/// A data type of the test.
#[derive(Debug, Default, Serialize, Deserialize, maki_core::Fields)]
struct Price {
    coins: i32,
}

impl DataAsset for Price {
    const NAME: &'static str = "price";
}

/// A script function: the variable `coins` gets the price of the file, and the flag
/// `missing` tells that a file that is not there gives no value.
fn pay(context: &mut ScriptContext) {
    let coins = context
        .data::<Price>("data/apple.price.ron")
        .map_or(-1, |price| price.coins);
    context.state().set_variable("coins", coins);
    let missing = context.data::<Price>("data/none.price.ron").is_none();
    context.state().set_flag("missing", missing);
}

#[test]
fn a_script_reads_a_data_asset_by_its_file() {
    let directory = directory("data-script");
    std::fs::create_dir_all(directory.join("data")).unwrap();
    std::fs::write(directory.join("data/apple.price.ron"), "(coins: 7)").unwrap();
    let events = r#"(name: "pay", pages: [(trigger: Autorun, commands: [CallScript("pay")])])"#;
    std::fs::write(directory.join("a.ron"), room(events)).unwrap();
    let mut app = game(&directory);
    add_data_type::<Price>(&mut app);
    add_script_function(&mut app, "pay", pay);
    app.run(|mut app| {
        steps(&mut app, 4);
        assert_eq!(app.world().resource::<GameState>().variable("coins"), 7);
        assert!(flag(&app, "missing"));
    });
    std::fs::remove_dir_all(directory).unwrap();
}
