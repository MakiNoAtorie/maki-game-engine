//! Each template of "New script" builds as it is, registers, and runs: in a room
//! with an object that has its component, for some steps.

use std::time::Duration;

use maki_engine::core::{App, ConfigFile, Interpolated, Startup, Transform};
use maki_engine::scene::{Player, RoomRequest, RoomTarget};
use maki_engine::{events, gameplay, platform, scene};

#[path = "../templates/empty.rs"]
mod empty;
#[path = "../templates/engine_module.rs"]
mod engine_module;
#[path = "../templates/event_command.rs"]
mod event_command;
#[path = "../templates/flickering_light.rs"]
mod flickering_light;
#[path = "../templates/interactable.rs"]
mod interactable;
#[path = "../templates/movement.rs"]
mod movement;
#[path = "../templates/patrol.rs"]
mod patrol;
#[path = "../templates/state_machine.rs"]
mod state_machine;
#[path = "../templates/talker.rs"]
mod talker;
#[path = "../templates/trigger_area.rs"]
mod trigger_area;

/// A game with the modules that a template needs, the template, and a room whose
/// object has the component `script_name` (with its default values), a light, and
/// an area. Some steps run, with the player in the area.
fn runs(add: fn(&mut App)) {
    let room = r#"(width: 8, depth: 8, height: 3, objects: [
        (name: "thing", pose: (position: (0, 1, 0)), components: {
            "script_name": (), "area": (),
            "light": (radius: 0.1, color: (1, 1, 1), intensity: 5.0)})],
        events: [(name: "row", pages: [(trigger: Autorun, commands: [])])])"#;
    let mut app = App::new(ConfigFile::default()).unwrap();
    app.add_module(platform::PlatformModule)
        .add_module(scene::SceneModule)
        .add_module(events::EventsModule)
        .add_module(gameplay::ControlsModule);
    add(&mut app);
    app.add_systems(
        Startup,
        move |mut commands: bevy_ecs::system::Commands,
              mut request: bevy_ecs::system::ResMut<RoomRequest>| {
            commands.spawn((Player, Interpolated::at(Transform::IDENTITY)));
            request.0 = Some(RoomTarget {
                path: "template.ron".into(),
                spawn: None,
                text: Some(room.to_owned()),
            });
        },
    );
    app.check_config().unwrap();
    app.run(|mut app| {
        for _ in 0..200 {
            app.advance(Duration::from_millis(16));
        }
        assert_eq!(app.world().resource::<scene::CurrentRoom>().loads, 1);
    });
}

#[test]
fn each_template_builds_registers_and_runs() {
    let templates: [fn(&mut App); 10] = [
        empty::add,
        engine_module::add,
        event_command::add,
        flickering_light::add,
        interactable::add,
        movement::add,
        patrol::add,
        state_machine::add,
        talker::add,
        trigger_area::add,
    ];
    for add in templates {
        runs(add);
    }
}

#[test]
fn the_engine_lists_each_template_file() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("templates");
    assert_eq!(std::fs::read_dir(dir).unwrap().count(), 10);
}
