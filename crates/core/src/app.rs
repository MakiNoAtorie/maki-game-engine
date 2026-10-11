use std::any::{TypeId, type_name};
use std::collections::HashSet;
use std::time::{Duration, Instant};

use anyhow::{Result, ensure};
use bevy_ecs::message::{Message, MessageRegistry, Messages, message_update_system};
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::{
    IntoScheduleConfigs, LogLevel, Schedule, ScheduleBuildSettings, ScheduleLabel, Schedules,
};
use bevy_ecs::system::ScheduleSystem;
use bevy_ecs::world::{FromWorld, World};

use crate::{Config, Time};

/// Runs one time, before the first frame.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Startup;
/// Turns platform events into input actions.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Input;
/// Simulation. Runs 0..N times in one frame, always with `Time::fixed_delta`.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FixedUpdate;
/// Per-frame logic that does not need a fixed rate.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Update;
/// Copies the data that the renderer needs out of the game state.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Extract;
/// Draws from extracted data only.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Render;
/// Shows the finished frame.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Present;

/// A feature. It adds its own systems and resources to the app,
/// so a new feature does not change the code of another feature.
pub trait Module {
    fn build(&self, app: &mut App);
}

/// Owns the ECS world and runs the schedules in a fixed order.
pub struct App {
    // Private: with `&mut World` a module could replace a schedule or run one out of order.
    world: World,
    modules: HashSet<TypeId>,
    max_fixed_steps: u32,
    last_frame: Option<Instant>,
    /// True when a fixed step ran after the last message buffer swap.
    fixed_step_since_message_update: bool,
}

impl App {
    pub fn new(config: Config) -> Result<Self> {
        // The config file is user input. A bad rate would make the step length infinite or zero.
        ensure!(
            config.fixed_hz.is_finite() && config.fixed_hz > 0.0,
            "fixed_hz must be a positive number, got {}",
            config.fixed_hz
        );
        ensure!(
            config.max_fixed_steps > 0,
            "max_fixed_steps must be 1 or more"
        );

        let mut world = World::new();
        // All schedules exist from the start, so running an empty one is not an error.
        add_schedule(&mut world, Startup);
        add_schedule(&mut world, Input);
        add_schedule(&mut world, FixedUpdate);
        add_schedule(&mut world, Update);
        add_schedule(&mut world, Extract);
        add_schedule(&mut world, Render);
        add_schedule(&mut world, Present);

        world.insert_resource(Time {
            fixed_delta: Duration::from_secs_f64(1.0 / config.fixed_hz),
            ..Default::default()
        });
        let max_fixed_steps = config.max_fixed_steps;
        world.insert_resource(config);

        Ok(Self {
            world,
            modules: HashSet::new(),
            max_fixed_steps,
            last_frame: None,
            fixed_step_since_message_update: false,
        })
    }

    /// Read access for drivers and tests. Changes go through the `App` methods.
    pub fn world(&self) -> &World {
        &self.world
    }

    /// Panics if a module of the same type was added before:
    /// its systems would run two times per frame.
    pub fn add_module<M: Module + 'static>(&mut self, module: M) -> &mut Self {
        assert!(
            self.modules.insert(TypeId::of::<M>()),
            "module {} was added two times",
            type_name::<M>()
        );
        module.build(self);
        self
    }

    pub fn add_systems<M>(
        &mut self,
        schedule: impl ScheduleLabel,
        systems: impl IntoScheduleConfigs<ScheduleSystem, M>,
    ) -> &mut Self {
        self.world
            .resource_mut::<Schedules>()
            .add_systems(schedule, systems);
        self
    }

    pub fn insert_resource(&mut self, resource: impl Resource) -> &mut Self {
        self.world.insert_resource(resource);
        self
    }

    /// Adds a resource with its default value. Keeps the value if the resource already exists.
    pub fn init_resource<R: Resource + FromWorld>(&mut self) -> &mut Self {
        self.world.init_resource::<R>();
        self
    }

    /// Registers a buffered event type. bevy_ecs names these "messages":
    /// systems send with `MessageWriter<T>` and receive with `MessageReader<T>`.
    pub fn add_message<T: Message>(&mut self) -> &mut Self {
        // Two modules can register the same type. A second registration would swap
        // the buffer two times per frame and drop messages too early.
        if !self.world.contains_resource::<Messages<T>>() {
            MessageRegistry::register_message::<T>(&mut self.world);
            // Many systems can send the same message type. Their order changes only
            // the order of messages in the buffer, so it is not an ambiguity error.
            self.world.allow_ambiguous_resource::<Messages<T>>();
        }
        self
    }

    /// Runs Startup, then gives the app to `driver`.
    /// The driver owns the loop and calls `update` one time per frame.
    /// The window event loop will be a driver; a headless loop is another.
    pub fn run<R>(mut self, driver: impl FnOnce(App) -> R) -> R {
        self.world.run_schedule(Startup);
        driver(self)
    }

    /// Runs one frame. Measures the real time since the previous call.
    pub fn update(&mut self) {
        let now = Instant::now();
        // The first frame has no previous frame, so its delta is zero.
        let delta = self.last_frame.map_or(Duration::ZERO, |last| now - last);
        self.last_frame = Some(now);
        self.advance(delta);
    }

    /// Runs one frame with a given delta. `update` calls this; tests call it directly.
    pub fn advance(&mut self, delta: Duration) {
        let fixed_delta = {
            let mut time = self.world.resource_mut::<Time>();
            time.delta = delta;
            time.elapsed += delta;
            time.frame += 1;
            time.accumulator += delta;
            time.fixed_delta
        };

        // A message stays readable until the second buffer swap after it was sent.
        // Swap only after a fixed step ran, or a FixedUpdate reader would miss
        // messages sent during frames that had no fixed step.
        if self.fixed_step_since_message_update {
            self.fixed_step_since_message_update = false;
            self.world
                .run_system_cached(message_update_system)
                .expect("message_update_system has no parameter that can fail");
        }

        self.world.run_schedule(Input);

        let mut steps = 0;
        while self.world.resource::<Time>().accumulator >= fixed_delta {
            if steps == self.max_fixed_steps {
                // The frame was too slow to catch up. Without this limit, each slow frame
                // would cause more steps and thus a slower next frame (spiral of death).
                // The simulation runs slow for this frame instead.
                let mut time = self.world.resource_mut::<Time>();
                tracing::warn!(dropped = ?time.accumulator, "fixed step limit reached");
                time.accumulator = Duration::ZERO;
                break;
            }
            let mut time = self.world.resource_mut::<Time>();
            time.accumulator -= fixed_delta;
            time.fixed_tick += 1;
            self.world.run_schedule(FixedUpdate);
            steps += 1;
        }
        self.fixed_step_since_message_update |= steps > 0;

        let mut time = self.world.resource_mut::<Time>();
        time.alpha = time.accumulator.div_duration_f32(fixed_delta);

        self.world.run_schedule(Update);
        self.world.run_schedule(Extract);
        self.world.run_schedule(Render);
        self.world.run_schedule(Present);
    }
}

fn add_schedule(world: &mut World, label: impl ScheduleLabel) {
    let mut schedule = Schedule::new(label);
    // Two systems that use the same data in a conflicting way, without a declared order,
    // make the schedule fail on its first run. The error names the two systems.
    // Fix with `.before()` / `.after()`, or `.ambiguous_with()` if the order has no effect.
    schedule.set_build_settings(ScheduleBuildSettings {
        ambiguity_detection: LogLevel::Error,
        ..Default::default()
    });
    world.add_schedule(schedule);
}

#[cfg(test)]
mod tests {
    use bevy_ecs::system::{Res, ResMut};

    use super::*;

    #[derive(bevy_ecs::resource::Resource, Default)]
    struct Order(Vec<&'static str>);

    fn push(name: &'static str) -> impl Fn(ResMut<Order>) {
        move |mut order| order.0.push(name)
    }

    #[test]
    fn schedules_run_in_order_and_fixed_steps_follow_the_rate() {
        let mut app = App::new(Config {
            fixed_hz: 50.0,
            max_fixed_steps: 4,
            ..Default::default()
        })
        .unwrap();
        app.init_resource::<Order>()
            .add_systems(Startup, push("startup"))
            .add_systems(Input, push("input"))
            .add_systems(FixedUpdate, push("fixed"))
            .add_systems(Update, push("update"))
            .add_systems(Extract, push("extract"))
            .add_systems(Render, push("render"))
            .add_systems(Present, |mut order: ResMut<Order>, time: Res<Time>| {
                assert!((0.0..1.0).contains(&time.alpha));
                order.0.push("present");
            });

        app.run(|mut app| {
            // 50 ms at 50 Hz: two full steps and half a step left.
            app.advance(Duration::from_millis(50));
            assert_eq!(
                app.world.resource::<Order>().0,
                [
                    "startup", "input", "fixed", "fixed", "update", "extract", "render", "present"
                ]
            );
            assert_eq!(app.world.resource::<Time>().alpha, 0.5);

            // A frame shorter than the remainder does no step.
            app.advance(Duration::from_millis(5));
            assert_eq!(app.world.resource::<Time>().fixed_tick, 2);

            // One second of frames gives 50 more steps.
            for _ in 0..100 {
                app.advance(Duration::from_millis(10));
            }
            assert_eq!(app.world.resource::<Time>().fixed_tick, 52);

            // A very long frame is limited to `max_fixed_steps` and the rest is dropped.
            app.advance(Duration::from_secs(10));
            let time = app.world.resource::<Time>();
            assert_eq!(time.fixed_tick, 56);
            assert_eq!(time.alpha, 0.0);
            assert_eq!(time.frame, 103);
        });
    }

    #[derive(Message)]
    struct Ping;

    #[derive(bevy_ecs::resource::Resource, Default)]
    struct Received(u32);

    #[test]
    fn fixed_update_reads_a_message_sent_frames_before() {
        use bevy_ecs::message::{MessageReader, MessageWriter};

        let mut app = App::new(Config {
            fixed_hz: 50.0,
            ..Default::default()
        })
        .unwrap();
        app.add_message::<Ping>()
            .init_resource::<Received>()
            .add_systems(Update, |mut pings: MessageWriter<Ping>, time: Res<Time>| {
                if time.frame == 1 {
                    pings.write(Ping);
                }
            })
            .add_systems(
                FixedUpdate,
                |mut pings: MessageReader<Ping>, mut received: ResMut<Received>| {
                    received.0 += pings.read().count() as u32;
                },
            );

        // Four frames without a fixed step, then frames with steps.
        for _ in 0..4 {
            app.advance(Duration::from_millis(1));
        }
        assert_eq!(app.world.resource::<Received>().0, 0);
        for _ in 0..5 {
            app.advance(Duration::from_millis(20));
        }
        assert_eq!(app.world.resource::<Received>().0, 1);
    }

    #[test]
    fn a_message_type_registered_by_two_modules_keeps_its_lifetime() {
        use bevy_ecs::message::{MessageReader, MessageWriter};

        let mut app = App::new(Config {
            fixed_hz: 50.0,
            ..Default::default()
        })
        .unwrap();
        app.add_message::<Ping>()
            .add_message::<Ping>()
            .init_resource::<Received>()
            .add_systems(Update, |mut pings: MessageWriter<Ping>, time: Res<Time>| {
                if time.frame == 1 {
                    pings.write(Ping);
                }
            })
            .add_systems(
                FixedUpdate,
                |mut pings: MessageReader<Ping>, mut received: ResMut<Received>| {
                    received.0 += pings.read().count() as u32;
                },
            );

        // Frame 1 sends after its fixed step. Frame 2 swaps the buffers once, then reads.
        app.advance(Duration::from_millis(20));
        app.advance(Duration::from_millis(20));
        assert_eq!(app.world.resource::<Received>().0, 1);
    }

    struct Empty;
    impl Module for Empty {
        fn build(&self, _app: &mut App) {}
    }

    #[test]
    #[should_panic(expected = "added two times")]
    fn duplicate_module_is_rejected() {
        let mut app = App::new(Config::default()).unwrap();
        app.add_module(Empty).add_module(Empty);
    }

    #[test]
    fn conflicting_systems_need_a_declared_order() {
        // `IntoScheduleConfigs` is in scope from `super`, for `.chain()`.
        let frame = |systems_are_ordered: bool| {
            let mut app = App::new(Config::default()).unwrap();
            app.init_resource::<Order>();
            if systems_are_ordered {
                app.add_systems(Update, (push("a"), push("b")).chain());
            } else {
                app.add_systems(Update, (push("a"), push("b")));
            }
            app.advance(Duration::ZERO);
        };
        frame(true);
        assert!(std::panic::catch_unwind(|| frame(false)).is_err());
    }

    #[test]
    fn bad_fixed_rate_is_rejected() {
        assert!(
            App::new(Config {
                fixed_hz: 0.0,
                ..Default::default()
            })
            .is_err()
        );
        assert!(
            App::new(Config {
                max_fixed_steps: 0,
                ..Default::default()
            })
            .is_err()
        );
    }
}
