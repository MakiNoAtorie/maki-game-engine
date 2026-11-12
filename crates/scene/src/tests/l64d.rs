//! Light animation (`maki_scene::light_animation`): `kelvin_rgb`, the fields of the file,
//! flicker, pulse, fade, the `Live` form, `patch_room`.

use std::time::Duration;

use bevy_ecs::entity::Entity;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::world::World;
use maki_core::{App, ConfigFile, Update};
use serde_json::json;

use crate::light_animation::planck_xy;
use crate::object::ComponentTypes;
use crate::{
    AnimationModule, CurrentRoom, FieldWrite, Light, LightAnimation, LightSource, ObjectId,
    RoomRequest, RoomTarget, SceneModule, kelvin_rgb, patch_room, room_text, write_field,
};

type Change = Box<dyn FnOnce(&mut World) + Send + Sync>;

/// Changes of the world for the next `Update`: the `App` gives no `&mut World`.
#[derive(Resource, Default)]
struct Pending(Vec<Change>);

fn apply_pending(world: &mut World) {
    for change in std::mem::take(&mut world.resource_mut::<Pending>().0) {
        change(world);
    }
}

/// Runs `change` in the `Update` of one frame, after its fixed step.
fn change(app: &mut App, change: impl FnOnce(&mut World) + Send + Sync + 'static) {
    app.resource_mut::<Pending>().0.push(Box::new(change));
    app.advance(Duration::from_millis(20));
}

/// A game at 50 Hz in a room with the light object 1 (component `light` of `fields`
/// after the base fields) and the plain light object 2.
fn room(test: &str, fields: &str) -> App {
    let directory =
        std::env::temp_dir().join(format!("maki-scene-l64d-{test}-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let lamp = format!(
        r#"(id: 1, name: "lamp", pose: (position: (0, 2, 0)), components: {{"light": (radius: 0.1, color: (1, 1, 1), intensity: 10.0{fields})}})"#
    );
    let plain = r#"(id: 2, name: "plain", pose: (position: (1, 2, 0)), components: {"light": (radius: 0.1, color: (1, 1, 1), intensity: 10.0)})"#;
    std::fs::write(
        directory.join("a.ron"),
        room_text(4.0, 4.0, 3.0, &[&lamp, plain]),
    )
    .unwrap();
    let config = "[time]\nfixed_hz = 50.0\n[assets]\nbackground = false";
    let mut app = App::new(ConfigFile::from_text(config)).unwrap();
    app.add_module(maki_assets::AssetsModule)
        .add_module(SceneModule)
        .add_module(AnimationModule)
        .init_resource::<Pending>()
        .add_systems(Update, apply_pending.ambiguous_with_all());
    app.resource_mut::<RoomRequest>().0 = Some(RoomTarget {
        path: directory.join("a.ron"),
        spawn: None,
        text: None,
    });
    app.advance(Duration::from_millis(20));
    app
}

fn object(app: &App, id: u32) -> Entity {
    let mut objects = app.world().try_query::<(Entity, &ObjectId)>().unwrap();
    let found = objects.iter(app.world()).find(|(_, found)| found.0 == id);
    found.expect("the object entity").0
}

fn light(app: &App, id: u32) -> Light {
    *app.world().get::<Light>(object(app, id)).unwrap()
}

/// The intensity of object 1 after each of `count` steps.
fn intensities(app: &mut App, count: usize) -> Vec<f32> {
    (0..count)
        .map(|_| {
            app.advance(Duration::from_millis(20));
            light(app, 1).intensity
        })
        .collect()
}

/// The `Live` tree of the light of object 1.
fn live(app: &App) -> serde_json::Value {
    let live = app
        .world()
        .resource::<ComponentTypes>()
        .get("light")
        .unwrap()
        .live
        .unwrap();
    (live.get)(app.world(), object(app, 1)).unwrap()
}

/// A `SetField` (a field track writes the same way) on the light of object 1.
fn set(app: &mut App, field: &'static str, value: serde_json::Value) {
    let entity = object(app, 1);
    change(app, move |world| {
        let written = write_field(world, entity, "light", field, &value);
        assert_eq!(written, Ok(FieldWrite::Written));
    });
}

#[test]
fn kelvin_rgb_follows_the_planckian_locus() {
    // CIE illuminant A (2856 K): x 0.44757, y 0.40745 (CIE 15).
    let (x, y) = planck_xy(2856.0);
    assert!(
        (x - 0.44757).abs() < 0.001 && (y - 0.40745).abs() < 0.001,
        "{x} {y}"
    );
    // White within 3 % at 6500 K (balanced to it).
    let white = kelvin_rgb(6500.0);
    assert!(
        white.min_element() > 0.97 && white.max_element() == 1.0,
        "{white}"
    );
    // A candle: red, little green, no blue.
    let candle = kelvin_rgb(1900.0);
    assert!(
        candle.x == 1.0 && candle.y < 0.4 && candle.z < 0.05,
        "{candle}"
    );
    // A blue sky: blue, then green, then red.
    let sky = kelvin_rgb(10000.0);
    assert!(sky.z == 1.0 && sky.y < 1.0 && sky.x < sky.y, "{sky}");
    // Below the range of the spline: its lower end.
    assert_eq!(kelvin_rgb(1000.0), kelvin_rgb(1667.0));
}

#[test]
fn the_new_fields_are_not_written_at_their_defaults() {
    let plain: LightSource =
        ron::from_str("(radius: 0.1, color: (1, 1, 1), intensity: 10.0)").unwrap();
    let text = ron::to_string(&plain).unwrap();
    for name in ["temperature", "flicker", "pulse", "fade"] {
        assert!(!text.contains(name), "{text}");
    }
    let flicker = LightSource {
        flicker: 0.5,
        ..plain
    };
    let text = ron::to_string(&flicker).unwrap();
    assert!(
        text.contains("flicker:0.5") && !text.contains("flicker_rate"),
        "{text}"
    );
    assert_eq!(ron::from_str::<LightSource>(&text).unwrap(), flicker);
}

#[test]
fn a_light_with_no_new_field_gets_no_animation() {
    let app = room("plain", ", flicker: 0.5");
    assert!(app.world().get::<LightAnimation>(object(&app, 1)).is_some());
    assert!(app.world().get::<LightAnimation>(object(&app, 2)).is_none());
}

#[test]
fn flicker_is_the_same_in_two_runs_and_holds_each_value_for_one_step_of_its_rate() {
    let first = intensities(&mut room("flicker_a", ", flicker: 0.5"), 200);
    let second = intensities(&mut room("flicker_b", ", flicker: 0.5"), 200);
    assert_eq!(first, second);
    assert!(first.iter().all(|i| (5.0..=10.0).contains(i)), "{first:?}");
    // 10 changes a second at 50 steps a second: a change at most each 5th step.
    let changes: Vec<usize> = (1..first.len())
        .filter(|&i| first[i] != first[i - 1])
        .collect();
    assert!(changes.len() > 20, "{changes:?}");
    assert!(
        changes.windows(2).all(|w| (w[1] - w[0]) % 5 == 0),
        "{changes:?}"
    );
}

#[test]
fn a_pulse_repeats_after_its_seconds() {
    let values = intensities(&mut room("pulse", ", pulse: 0.5, pulse_seconds: 2.0"), 200);
    for i in 0..100 {
        assert!((values[i] - values[i + 100]).abs() < 1e-3, "{i}");
    }
    let min = values.iter().copied().fold(f32::MAX, f32::min);
    let max = values.iter().copied().fold(0.0, f32::max);
    assert!(
        (min - 5.0).abs() < 0.01 && (max - 10.0).abs() < 0.01,
        "{min} {max}"
    );
}

#[test]
fn fade_takes_its_seconds_off_and_on() {
    let mut app = room("fade", ", fade: 0.5");
    set(&mut app, "on", json!(false));
    let down = intensities(&mut app, 25);
    assert!(down[..24].windows(2).all(|w| w[1] < w[0]), "{down:?}");
    assert!((down[0] - 9.6).abs() < 1e-3, "{down:?}");
    // Off at the end of the fade (an off light keeps its intensity).
    assert!(!light(&app, 1).on);
    set(&mut app, "on", json!(true));
    let up = intensities(&mut app, 25);
    assert!(light(&app, 1).on && (up[0] - 0.4).abs() < 1e-3, "{up:?}");
    assert!((up[24] - 10.0).abs() < 1e-3, "{up:?}");
}

#[test]
fn a_field_write_of_flicker_animates_a_plain_light() {
    let mut app = room("keyed", "");
    assert!(app.world().get::<LightAnimation>(object(&app, 1)).is_none());
    set(&mut app, "flicker", json!(0.5));
    assert!(app.world().get::<LightAnimation>(object(&app, 1)).is_some());
    let values = intensities(&mut app, 50);
    assert!(values.iter().any(|&i| i < 10.0), "{values:?}");
    assert_eq!(live(&app)["flicker"], json!(0.5));
}

#[test]
fn the_live_form_reads_and_writes_the_base_under_the_animation() {
    let mut app = room("base", ", flicker: 0.5, temperature: 1900.0");
    set(&mut app, "intensity", json!(20.0));
    let values = intensities(&mut app, 50);
    assert!(
        values.iter().all(|i| (10.0..=20.0).contains(i)),
        "{values:?}"
    );
    assert!(values.iter().any(|&i| i != 20.0));
    let tree = live(&app);
    assert_eq!(tree["intensity"], json!(20.0));
    assert_eq!(tree["color"], json!([1.0, 1.0, 1.0]));
    assert_eq!(tree["temperature"], json!(1900.0));
    // The effective color is that of the temperature.
    assert_eq!(light(&app, 1).color, kelvin_rgb(1900.0));
}

#[test]
fn patch_room_carries_the_new_fields() {
    let mut app = room("patch", "");
    let mut def = app.world().resource::<CurrentRoom>().def.clone();
    let mut lamp = def.objects[0].light().unwrap().unwrap();
    lamp.animation.temperature = 2700.0;
    def.objects[0].set_light(lamp);
    // As the editor writes it: the patch compares the texts.
    let plain = def.objects[1].light().unwrap().unwrap();
    def.objects[1].set_light(plain);
    change(&mut app, move |world| {
        assert!(patch_room(world, &def, String::new()))
    });
    let entity = object(&app, 1);
    assert!(app.world().get::<LightAnimation>(entity).is_some());
    assert_eq!(light(&app, 1).color, kelvin_rgb(2700.0));
    assert!(app.world().get::<LightAnimation>(object(&app, 2)).is_none());
}
