//! The commands `PlayTimeline` and `StopTimeline`.

use maki_assets::clip::{Channel, Key, KeyMode, KeyValue, Track};
use maki_scene::{AnimationModule, ComponentText, Timeline, TimelineTrack};

use super::*;

/// The crate 11 and the timeline object 20: a step track on the crate, up 1 m at 0.1 s,
/// in a timeline of `length` s.
fn timeline_objects(length: f32, playing: bool) -> String {
    let up = |time, y| Key {
        time,
        value: KeyValue::Vector([0.0, y, 0.0]),
    };
    let timeline = Timeline {
        length,
        playing,
        tracks: vec![TimelineTrack {
            object: ObjectId(11),
            track: Track {
                channel: Channel::Position,
                target: None,
                mode: Some(KeyMode::Step),
                keys: vec![up(0.0, 0.0), up(0.1, 1.0)],
            },
        }],
        ..Timeline::default()
    };
    let text = ComponentText::of(&timeline);
    format!(
        r#"(id: 11, name: "crate", pose: (position: (2, 0, 2)), components: {{"prop": (mesh: Cube(1.0)), "light": (radius: 0.1, color: (1, 1, 1), intensity: 1.0)}}),
        (id: 20, name: "tl", components: {{"timeline": {}}}),"#,
        text.as_str()
    )
}

fn timeline_game(test: &str, length: f32, playing: bool, events: &str) -> App {
    let directory = directory(&format!("l64c-{test}"));
    let objects = format!("objects: [\n{}", timeline_objects(length, playing));
    let text = room(events).replacen("objects: [", &objects, 1);
    std::fs::write(directory.join("a.ron"), text).unwrap();
    let mut app = game(&directory);
    app.add_module(AnimationModule);
    app
}

fn height(app: &App) -> f32 {
    let world = app.world();
    let mut query = world.try_query::<(&ObjectId, &Interpolated)>().unwrap();
    let found = query.iter(world).find(|(id, _)| id.0 == 11);
    found.unwrap().1.current.translation.y
}

fn playing(app: &App) -> bool {
    let world = app.world();
    let mut query = world.try_query::<&Timeline>().unwrap();
    query.single(world).unwrap().playing
}

#[test]
fn play_starts_at_zero_and_wait_holds_the_next_row_to_the_end() {
    // Length 0.4 s: 20 steps. The second play restarts at 0 after the first ended.
    let events = r#"(name: "go", pages: [(trigger: Autorun, commands: [
        PlayTimeline(20, true), SetFlag("first", true), PlayTimeline(20, true), SetFlag("second", true)])])"#;
    timeline_game("play", 0.4, false, events).run(|mut app| {
        steps(&mut app, 3);
        assert!(playing(&app));
        assert!(!flag(&app, "first"), "waits");
        let mut heights = Vec::new();
        let mut first_at = None;
        for step in 0..60 {
            steps(&mut app, 1);
            heights.push(height(&app));
            if first_at.is_none() && flag(&app, "first") {
                first_at = Some(step);
            }
        }
        assert!(first_at.is_some_and(|at| at >= 10), "{first_at:?}");
        assert!(flag(&app, "second"));
        // Up, back to 0 at the restart, up again.
        let drops = heights.windows(2).filter(|pair| pair[1] < pair[0]).count();
        assert_eq!(drops, 1, "{heights:?}");
        assert!(!playing(&app));
    });
}

#[test]
fn stop_stops_and_stop_with_end_writes_the_last_keys() {
    for (end, wanted) in [(false, 0.0), (true, 1.0)] {
        let events = format!(
            r#"(name: "stop", pages: [(trigger: Autorun, commands: [Wait(0.05), StopTimeline(20, {end}), SetFlag("stopped", true)])])"#
        );
        timeline_game(&format!("stop{end}"), 1.0, true, &events).run(|mut app| {
            steps(&mut app, 2);
            assert!(playing(&app), "plays from the load");
            steps(&mut app, 10);
            assert!(flag(&app, "stopped"));
            assert!(!playing(&app));
            assert_eq!(height(&app), wanted, "end: {end}");
            // It stays stopped.
            steps(&mut app, 10);
            assert_eq!(height(&app), wanted);
            assert!(!playing(&app));
        });
    }
}

#[test]
fn the_check_finds_an_unknown_object_and_one_with_no_timeline() {
    let directory = directory("l64c-check");
    let mut app = App::new(ConfigFile::default()).unwrap();
    app.add_module(PlatformModule)
        .add_module(SceneModule)
        .add_module(EventsModule)
        .add_module(AnimationModule);
    app.update();
    let validators = app.world().resource::<maki_scene::RoomValidators>();
    let events = r#"(name: "a", pages: [(trigger: Autorun, commands: [
        PlayTimeline(99, false), StopTimeline(2, true), PlayTimeline(0, true)])])"#;
    let error = maki_scene::check_room(&directory.join("a.ron"), &room(events), validators)
        .unwrap_err()
        .to_string();
    assert!(error.contains("no object `id 99`"), "{error}");
    assert!(error.contains("`id 2` has no timeline"), "{error}");
    assert_eq!(error.lines().count(), 2, "{error}");
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn play_on_an_object_with_no_timeline_warns_and_ends_at_once() {
    // Object 0 (the event's object, no timeline) passes the room check: only the run finds it.
    let events = r#"(name: "none", pages: [(trigger: Autorun, commands: [PlayTimeline(0, true), SetFlag("after", true)])])"#;
    timeline_game("none", 0.4, false, events).run(|mut app| {
        steps(&mut app, 3);
        assert!(flag(&app, "after"));
        assert!(!playing(&app));
    });
}
