use std::any::{TypeId, type_name};
use std::collections::HashSet;
use std::time::{Duration, Instant};

use anyhow::Result;
use bevy_ecs::component::Component;
use bevy_ecs::message::{Message, MessageRegistry, Messages, message_update_system};
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::{
    IntoScheduleConfigs, LogLevel, Schedule, ScheduleBuildSettings, ScheduleLabel, Schedules,
};
use bevy_ecs::system::ScheduleSystem;
use bevy_ecs::world::{FromWorld, World};

use crate::{ConfigFile, PlayState, Settings, Time, TimeSettings};

/// Runs one time, before the first frame.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Startup;
/// Turns platform events into input actions.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Input;
/// Simulation. Runs 0..N times in one frame, always with `Time::fixed_delta`.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FixedUpdate;
/// Runs before each FixedUpdate step. For bookkeeping that must come before all
/// simulation systems of the step.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FixedFirst;
/// Runs after each FixedUpdate step. For bookkeeping that must follow all
/// simulation systems of the step, such as clearing one-step flags.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FixedLast;
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
/// "The play state goes back to its start": not a part of the frame. The caller runs
/// it at once (`App::reset_play`, or `World::run_schedule` in a system with the
/// world): at the start of the game, and a tool on play and on stop. Each module
/// with state that a game leaves behind (an event that runs, a fade, a shake) adds a
/// system that sets its own resources back. Write only a value that differs: a
/// write is a change for the renderer.
///
/// The flags and variables of the game are not play state of this signal: a tool
/// sets them for a test before play. Who starts a new game clears them.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResetPlay;

/// Runs a schedule in a `tracing` span that has the name of the schedule. A profiler
/// (Tracy, with the `profile` feature of the game) shows the time of each schedule.
/// A macro, because a span name must be a constant.
macro_rules! run_schedule {
    ($world:expr, $label:ident) => {{
        let _span = tracing::trace_span!(stringify!($label)).entered();
        $world.run_schedule($label);
    }};
}

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
    /// `config`: the config file. Each module reads its own table of it
    /// (`add_settings`); this reads the table of the fixed steps.
    pub fn new(mut config: ConfigFile) -> Result<Self> {
        let time: TimeSettings = config.read();
        // Now, not with the errors of the other tables: the step length must be valid.
        config.finish_errors()?;

        let mut world = World::new();
        // All schedules exist from the start, so running an empty one is not an error.
        add_schedule(&mut world, Startup);
        add_schedule(&mut world, Input);
        add_schedule(&mut world, FixedFirst);
        add_schedule(&mut world, FixedUpdate);
        add_schedule(&mut world, FixedLast);
        add_schedule(&mut world, Update);
        add_schedule(&mut world, Extract);
        add_schedule(&mut world, Render);
        add_schedule(&mut world, Present);
        add_schedule(&mut world, ResetPlay);

        world.insert_resource(Time {
            fixed_delta: Duration::from_secs_f64(1.0 / time.fixed_hz),
            ..Default::default()
        });
        world.init_resource::<PlayState>();
        let max_fixed_steps = time.max_fixed_steps;
        world.insert_resource(time);
        world.insert_resource(config);

        let mut app = Self {
            world,
            modules: HashSet::new(),
            max_fixed_steps,
            last_frame: None,
            fixed_step_since_message_update: false,
        };
        // Built in, as `Time` is: see the `interpolation` module. Each simulation
        // system writes the pose of its own entities (the player, the movers), so
        // their order has no effect.
        app.allow_ambiguous_component::<crate::Interpolated>()
            .add_systems(FixedFirst, crate::interpolation::shift_poses)
            .add_systems(Update, crate::interpolation::interpolate_transforms);
        Ok(app)
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

    /// Adds the settings of a module as a resource, with the values of its table of
    /// the config file. A bad table gives the defaults, and `check_config` returns
    /// the error.
    pub fn add_settings<S: Settings>(&mut self) -> &mut Self {
        let settings: S = self.world.resource_mut::<ConfigFile>().read();
        self.insert_resource(settings)
    }

    /// As [`App::add_settings`], and the table is written back to the config file
    /// when a system changes the resource (a value that the user sets in a panel).
    /// The other tables and the comments of the file stay as they are. Only a
    /// module that asks is written: a value of a debug panel does not reach the file.
    pub fn add_saved_settings<S: Settings>(&mut self) -> &mut Self {
        // In any order with the system that changes the resource: a change is
        // written some time after it, not in its frame.
        let save = crate::config::save_settings::<S>.ambiguous_with_all();
        self.add_settings::<S>().add_systems(Update, save)
    }

    /// Runs the [`ResetPlay`] schedule now.
    pub fn reset_play(&mut self) {
        run_schedule!(self.world, ResetPlay);
    }

    /// Call after the last module was added. Returns the errors of the config tables
    /// of all modules, with the file, the table, and the key. Writes the config file
    /// with the defaults of all modules if there was none.
    pub fn check_config(&self) -> Result<()> {
        self.world.resource::<ConfigFile>().finish()
    }

    /// Panics if the resource already exists: a second module would silently
    /// replace the value of the first.
    pub fn insert_resource<R: Resource>(&mut self, resource: R) -> &mut Self {
        assert!(
            !self.world.contains_resource::<R>(),
            "resource {} already exists",
            type_name::<R>()
        );
        self.world.insert_resource(resource);
        self
    }

    /// Changes a resource while the app is built. For a table that many modules add
    /// their entries to (key bindings, room checks). Panics if the resource does not
    /// exist: add the module that owns it first.
    pub fn resource_mut<R: Resource<Mutability = bevy_ecs::component::Mutable>>(
        &mut self,
    ) -> bevy_ecs::world::Mut<'_, R> {
        self.world.resource_mut::<R>()
    }

    /// Adds a resource with its default value. Keeps the value if the resource already exists.
    pub fn init_resource<R: Resource + FromWorld>(&mut self) -> &mut Self {
        self.world.init_resource::<R>();
        self
    }

    /// Tells the ambiguity check that systems can use this resource in any order.
    /// Only for a resource where the order has no effect on the result, such as a
    /// container in which each system writes its own part.
    pub fn allow_ambiguous_resource<R: Resource>(&mut self) -> &mut Self {
        self.world.allow_ambiguous_resource::<R>();
        self
    }

    /// Tells the ambiguity check that systems can use this component in any order.
    /// Only for a component where each system writes its own entities.
    pub fn allow_ambiguous_component<C: Component>(&mut self) -> &mut Self {
        self.world.allow_ambiguous_component::<C>();
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

    /// Sends a message from outside the schedules. The driver uses this to hand
    /// OS events (key, resize) to the systems of the next frame.
    /// Panics if the type was not registered with `add_message`.
    pub fn write_message<T: Message>(&mut self, message: T) -> &mut Self {
        assert!(
            self.world.write_message(message).is_some(),
            "message {} is not registered",
            type_name::<T>()
        );
        self
    }

    /// Runs Startup, then gives the app to `driver`.
    /// The driver owns the loop and calls `update` one time per frame.
    /// The window event loop will be a driver; a headless loop is another.
    pub fn run<R>(mut self, driver: impl FnOnce(App) -> R) -> R {
        run_schedule!(self.world, Startup);
        // A game starts.
        self.reset_play();
        driver(self)
    }

    /// Runs one frame. Measures the real time since the previous call.
    pub fn update(&mut self) {
        let now = Instant::now();
        // The first frame has no previous frame, so its delta is zero.
        let delta = self.last_frame.map_or(Duration::ZERO, |last| now - last);
        self.last_frame = Some(now);
        let frame_time = self.world.resource::<Time>().frame_time;
        self.advance(frame_time.unwrap_or(delta));
    }

    /// Sets `Time::frame_time` before the first frame.
    pub fn set_frame_time(&mut self, frame_time: Option<Duration>) -> &mut Self {
        self.world.resource_mut::<Time>().frame_time = frame_time;
        self
    }

    /// Makes the delta of the next `update` zero. The driver calls this after a pause
    /// (minimized window), so the pause does not count as one slow frame.
    pub fn reset_clock(&mut self) {
        self.last_frame = None;
    }

    /// The game is in the state pause: no fixed step runs. Apart from
    /// `Time::paused`, which a tool sets.
    fn game_paused(&self) -> bool {
        *self.world.resource::<PlayState>() == PlayState::Pause
    }

    /// Runs one frame with a given delta. `update` calls this; tests call it directly.
    pub fn advance(&mut self, delta: Duration) {
        let fixed_delta = {
            let game_paused = self.game_paused();
            let mut time = self.world.resource_mut::<Time>();
            // A frame that was too slow counts as a shorter frame: game time runs slow
            // for that frame. Without this limit, each slow frame would cause more fixed
            // steps and thus a slower next frame (spiral of death).
            // The limit also keeps `Update` systems from a large jump.
            let max_delta = time.fixed_delta * self.max_fixed_steps;
            if delta > max_delta {
                tracing::warn!(dropped = ?(delta - max_delta), "slow frame, delta limited");
            }
            let delta = delta.min(max_delta);
            time.delta = delta;
            time.elapsed += delta;
            time.frame += 1;
            // The time of a pause does not come back as steps after it.
            if !time.paused && !game_paused {
                time.accumulator += delta;
            }
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

        run_schedule!(self.world, Input);

        // The delta limit above keeps this loop at `max_fixed_steps` steps or less.
        let mut steps = 0;
        // A step that pauses the game is the last one.
        while self.world.resource::<Time>().accumulator >= fixed_delta && !self.game_paused() {
            let mut time = self.world.resource_mut::<Time>();
            time.accumulator -= fixed_delta;
            time.fixed_tick += 1;
            run_schedule!(self.world, FixedFirst);
            run_schedule!(self.world, FixedUpdate);
            run_schedule!(self.world, FixedLast);
            steps += 1;
        }
        self.fixed_step_since_message_update |= steps > 0;

        let game_paused = self.game_paused();
        let mut time = self.world.resource_mut::<Time>();
        // A pause drops the full steps left; the rest keeps `alpha`, so the picture stands.
        while game_paused && time.accumulator >= fixed_delta {
            time.accumulator -= fixed_delta;
        }
        time.alpha = time.accumulator.div_duration_f32(fixed_delta);

        run_schedule!(self.world, Update);
        run_schedule!(self.world, Extract);
        run_schedule!(self.world, Render);
        run_schedule!(self.world, Present);

        // Drops the records of removed components (`RemovedComponents`) that are two
        // frames old. Without this call the records stay in memory forever.
        self.world.clear_trackers();
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
        let mut app = App::new(ConfigFile::from_text(
            "[time]
fixed_hz = 50.0
max_fixed_steps = 4",
        ))
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

            // A very long frame counts as `max_fixed_steps` steps of time.
            app.advance(Duration::from_secs(10));
            let time = app.world.resource::<Time>();
            assert_eq!(time.delta, Duration::from_millis(80));
            assert_eq!(time.fixed_tick, 56);
            assert_eq!(time.alpha, 0.75);
            assert_eq!(time.frame, 103);
        });
    }

    #[test]
    fn no_fixed_step_runs_in_a_pause_and_a_step_that_pauses_is_the_last() {
        let mut app = App::new(ConfigFile::from_text("[time]\nfixed_hz = 50.0")).unwrap();
        // The third step pauses the game, as a command of an event can.
        app.add_systems(
            FixedUpdate,
            |time: Res<Time>, mut state: ResMut<crate::PlayState>| {
                if time.fixed_tick == 3 {
                    state.change(crate::PlayState::Pause);
                }
            },
        );
        app.run(|mut app| {
            // 110 ms is five and a half steps of time; the pause stops the frame at
            // the third, and the picture stands at the half step.
            app.advance(Duration::from_millis(110));
            assert_eq!(app.world.resource::<Time>().fixed_tick, 3);
            assert_eq!(app.world.resource::<Time>().alpha, 0.5);
            app.advance(Duration::from_millis(100));
            assert_eq!(app.world.resource::<Time>().fixed_tick, 3);
            assert_eq!(app.world.resource::<Time>().alpha, 0.5);
            // Play again: the time of the pause does not come back as steps.
            app.world
                .resource_mut::<crate::PlayState>()
                .change(crate::PlayState::Play);
            app.advance(Duration::from_millis(20));
            assert_eq!(app.world.resource::<Time>().fixed_tick, 4);
        });
    }

    #[derive(Message)]
    struct Ping;

    #[derive(bevy_ecs::resource::Resource, Default)]
    struct Received(u32);

    #[test]
    fn fixed_update_reads_a_message_sent_frames_before() {
        use bevy_ecs::message::{MessageReader, MessageWriter};

        let mut app = App::new(ConfigFile::from_text("[time]\nfixed_hz = 50.0")).unwrap();
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

        let mut app = App::new(ConfigFile::from_text("[time]\nfixed_hz = 50.0")).unwrap();
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
    #[should_panic(expected = "already exists")]
    fn duplicate_resource_is_rejected() {
        let mut app = App::new(ConfigFile::default()).unwrap();
        app.insert_resource(TimeSettings::default());
    }

    #[test]
    #[should_panic(expected = "added two times")]
    fn duplicate_module_is_rejected() {
        let mut app = App::new(ConfigFile::default()).unwrap();
        app.add_module(Empty).add_module(Empty);
    }

    #[test]
    fn conflicting_systems_need_a_declared_order() {
        // `IntoScheduleConfigs` is in scope from `super`, for `.chain()`.
        let frame = |systems_are_ordered: bool| {
            let mut app = App::new(ConfigFile::default()).unwrap();
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
    fn bad_config_values_are_rejected() {
        for text in [
            "[time]
fixed_hz = 0.0",
            "[time]
max_fixed_steps = 0",
            "[time]
fixed_hz = nan",
            "[time]
fixed = 60.0",
        ] {
            assert!(App::new(ConfigFile::from_text(text)).is_err(), "{text}");
        }
    }
}
