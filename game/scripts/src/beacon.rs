//! A script of this game: a light pulses while the player stands in an area, and the
//! area says how often it was touched. It uses each hook of a script.
//!
//! The beacon is an object of its own with an area, and names the light by its id
//! (the editor shows the name):
//!
//! ```ron
//! (name: "beacon", components: {"beacon": (light: 3), "area": (width: 1.6, depth: 1.6)})
//! ```
//!
//! Not on the entity of the light: a component more there changes the order of the
//! lights for the renderer, and so the picture of the benchmark.

use bevy_ecs::component::Component;
use bevy_ecs::world::Mut;
use maki_engine::assets::{DataAsset, DataRef, add_data_type};
use maki_engine::core::{App, Fields};
use maki_engine::events::{Builtin, Script, ScriptContext, add_script, add_script_function};
use maki_engine::scene::{Light, ObjectId};
use serde::{Deserialize, Serialize};

/// The variable that counts the presses of interact at the beacon.
const TOUCHES: &str = "beacon_touches";

#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
#[fields(default)]
pub struct Beacon {
    /// The light object. The editor picks it by its name and stores its id.
    pub light: ObjectId,
    /// Pulses in a second.
    #[field(range = 0.1..=10.0, step = 0.1)]
    pub pulses: f32,
    /// The largest change of the brightness, as a part of it.
    #[field(range = 0.0..=1.0, step = 0.05)]
    pub strength: f32,
    /// A style file: its pulses and strength replace those above at the start.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<DataRef<BeaconStyle>>,
    /// The brightness of the light at the start. Not in the file.
    #[serde(skip)]
    #[field(skip)]
    base: f32,
    /// Seconds since the player came in. `None`: the player is not in the area.
    #[serde(skip)]
    #[field(skip)]
    time: Option<f32>,
}

impl Default for Beacon {
    fn default() -> Self {
        Self {
            light: ObjectId::default(),
            pulses: 1.5,
            strength: 0.6,
            style: None,
            base: 0.0,
            time: None,
        }
    }
}

/// A data asset of this game: a look of a beacon that many beacons share
/// (`data/<name>.beacon_style.ron`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
#[fields(default)]
pub struct BeaconStyle {
    /// Pulses in a second.
    #[field(range = 0.1..=10.0, step = 0.1)]
    pub pulses: f32,
    /// The largest change of the brightness, as a part of it.
    #[field(range = 0.0..=1.0, step = 0.05)]
    pub strength: f32,
}

impl Default for BeaconStyle {
    fn default() -> Self {
        Self {
            pulses: 1.5,
            strength: 0.6,
        }
    }
}

impl DataAsset for BeaconStyle {
    const NAME: &'static str = "beacon_style";
}

impl Beacon {
    fn light<'a>(&self, context: &'a mut ScriptContext) -> Option<Mut<'a, Light>> {
        context.object_mut::<Light>(self.light)
    }

    fn set_intensity(&self, context: &mut ScriptContext, intensity: f32) {
        if let Some(mut light) = self.light(context) {
            light.intensity = intensity;
        }
    }
}

impl Script for Beacon {
    fn start(&mut self, context: &mut ScriptContext) {
        let style = self
            .style
            .as_ref()
            .map(|style| style.file.to_string_lossy());
        if let Some(style) = style.and_then(|file| context.data::<BeaconStyle>(&file)) {
            self.pulses = style.pulses;
            self.strength = style.strength;
        }
        self.base = self.light(context).map_or(0.0, |light| light.intensity);
    }

    fn step(&mut self, context: &mut ScriptContext) {
        // At rest no write: a change of a light is a change for the renderer.
        let Some(time) = &mut self.time else { return };
        *time += context.seconds();
        let wave = (*time * self.pulses * std::f32::consts::TAU).sin();
        self.set_intensity(context, self.base * (1.0 + self.strength * wave));
    }

    fn interact(&mut self, context: &mut ScriptContext) {
        // Two commands of the engine, through the same registry as the rows of an
        // event: the count, and a message that waits for a press.
        context.call(Builtin::AddVariable(TOUCHES.to_owned(), 1));
        context.call(Builtin::Message("beacon.touched".into()));
    }

    fn enter_area(&mut self, _context: &mut ScriptContext) {
        self.time = Some(0.0);
    }

    fn leave_area(&mut self, context: &mut ScriptContext) {
        self.time = None;
        self.set_intensity(context, self.base);
    }
}

/// A script function that a row calls (`CallScript("beacon_forget")`): the count of
/// the touches starts again, and a message says so. The context is the object of the
/// event.
fn forget(context: &mut ScriptContext) {
    context.state().set_variable(TOUCHES, 0);
    // A text shows variables only, not the name of the object: the text names the dial.
    context.call(Builtin::Message("beacon.forget".into()));
}

pub fn add(app: &mut App) {
    add_script::<Beacon>(app, "beacon");
    add_data_type::<BeaconStyle>(app);
    add_script_function(app, "beacon_forget", forget);
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use maki_engine::core::{ConfigFile, FixedFirst, Interpolated, Startup, Transform};
    use maki_engine::events::{CommandRegistry, EventsModule, GameState, MessageBox};
    use maki_engine::physics::{Kinematic, Shape};
    use maki_engine::platform::{Key, PlatformModule, RawInput};
    use maki_engine::scene::{Player, RoomRequest, RoomTarget, SceneModule};

    use super::*;

    #[test]
    fn the_lamp_pulses_while_the_player_is_under_it_and_counts_the_touches() {
        let room = r#"(width: 8, depth: 8, height: 3, objects: [
            (id: 1, name: "lamp", pose: (position: (0.0, 2.3, 1.0)), components: {
                "light": (radius: 0.15, color: (1.0, 0.9, 0.8), intensity: 10.0)}),
            (pose: (position: (0.0, 0.0, 1.0)), components: {
                "beacon": (light: 1, pulses: 1.0, strength: 0.5),
                "area": (width: 1.0, depth: 1.0)})])"#;
        let mut app = App::new(ConfigFile::from_text("[time]\nfixed_hz = 50.0")).unwrap();
        app.add_module(PlatformModule)
            .add_module(SceneModule)
            .add_module(maki_engine::physics::PhysicsModule)
            .add_module(EventsModule);
        add(&mut app);
        app.add_systems(
            Startup,
            move |mut commands: bevy_ecs::system::Commands,
                  mut request: bevy_ecs::system::ResMut<RoomRequest>| {
                // The capsule is what the sensor of the area sees.
                let body = Kinematic {
                    shape: Shape::Capsule {
                        radius: 0.3,
                        half_height: 0.5,
                    },
                    center: [0.0, 0.8, 0.0].into(),
                    sensor: false,
                };
                commands.spawn((Player, Interpolated::at(Transform::IDENTITY), body));
                // In the directory of the rooms of the game: the message is of its text table.
                request.0 = Some(RoomTarget {
                    path: concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/rooms/beacon.ron").into(),
                    spawn: None,
                    text: Some(room.to_owned()),
                });
            },
        );
        // The player walks under the lamp in the step after the frame number 10.
        app.add_systems(
            FixedFirst,
            |time: bevy_ecs::system::Res<maki_engine::core::Time>,
             mut players: bevy_ecs::system::Query<
                &mut Interpolated,
                bevy_ecs::query::With<Player>,
            >| {
                if time.fixed_tick == 10 {
                    for mut pose in &mut players {
                        pose.snap(Transform::from_translation([0.0, 0.0, 1.0].into()));
                    }
                }
            },
        );
        app.run(|mut app| {
            let step = Duration::from_millis(20);
            let intensity = |app: &App| {
                let mut lights = app.world().try_query::<&Light>().unwrap();
                lights.single(app.world()).unwrap().intensity
            };
            for _ in 0..8 {
                app.advance(step);
            }
            // Far from the lamp: the light of the file.
            assert_eq!(intensity(&app), 10.0);
            for _ in 0..12 {
                app.advance(step);
            }
            assert_ne!(intensity(&app), 10.0);
            assert!((5.0..=15.0).contains(&intensity(&app)));

            // In the area each direction faces it: a press shows the message.
            app.write_message(RawInput::KeyDown(Key::Z))
                .write_message(RawInput::KeyUp(Key::Z));
            app.advance(step);
            assert_eq!(app.world().resource::<GameState>().variable(TOUCHES), 1);
            let message = app.world().resource::<MessageBox>().0.clone();
            assert_eq!(message.as_deref(), Some("The lamp hums. Touched 1 times."));
        });
    }

    #[test]
    fn a_beacon_with_a_style_takes_its_pulses_from_the_file() {
        let directory = std::env::temp_dir().join(format!("maki-beacon-{}", std::process::id()));
        std::fs::create_dir_all(directory.join("data")).unwrap();
        let style = BeaconStyle {
            pulses: 4.0,
            strength: 0.2,
        };
        let text = maki_engine::assets::data_text(&style);
        std::fs::write(directory.join("data/fast.beacon_style.ron"), text).unwrap();
        let room = maki_engine::scene::room_text(
            8.0,
            8.0,
            3.0,
            &[r#"(pose: (position: (0.0, 0.0, 1.0)), components: {
                "beacon": (style: "data/fast.beacon_style.ron"),
                "area": (width: 1.0, depth: 1.0)})"#],
        );
        let mut app = App::new(ConfigFile::from_text("[time]\nfixed_hz = 50.0")).unwrap();
        app.add_module(PlatformModule)
            .add_module(SceneModule)
            .add_module(EventsModule);
        add(&mut app);
        let path = directory.join("beacon.ron");
        app.add_systems(
            Startup,
            move |mut request: bevy_ecs::system::ResMut<RoomRequest>| {
                request.0 = Some(RoomTarget {
                    path: path.clone(),
                    spawn: None,
                    text: Some(room.to_owned()),
                });
            },
        );
        app.run(|mut app| {
            for _ in 0..4 {
                app.advance(Duration::from_millis(20));
            }
            let mut beacons = app.world().try_query::<&Beacon>().unwrap();
            let beacon = beacons.single(app.world()).unwrap();
            assert_eq!((beacon.pulses, beacon.strength), (4.0, 0.2));
        });
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_row_calls_the_function_that_the_beacon_adds() {
        let mut app = App::new(ConfigFile::default()).unwrap();
        app.add_module(PlatformModule)
            .add_module(SceneModule)
            .add_module(EventsModule);
        add(&mut app);
        let registry = app.world().resource::<CommandRegistry>();
        assert!(registry.functions().contains(&"beacon_forget"));
        let types = app.world().resource::<maki_engine::assets::DataTypes>();
        assert!(types.get("beacon_style").is_some());
    }
}
