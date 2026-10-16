//! The event interpreter: starts events on their triggers and runs their commands.
//!
//! A running event is state, not a function call: the command lists that are open,
//! the index of the current command, and what that command waits for. In each fixed
//! step the interpreter runs commands until one must wait (a wait time, a door that
//! turns, a message that the player has not closed). The game loop never stops for
//! an event.
//!
//! A command is a value of an open set (`command` module). Its handler gets the
//! world, so `run_events` is a system with the full world.

use bevy_ecs::query::{QueryState, With};
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Res, ResMut};
use bevy_ecs::world::{Mut, World};
use glam::Vec3;
use maki_core::Interpolated;
use maki_platform::FixedActionState;
use maki_scene::{CurrentRoom, Player};

use crate::command::{Command, CommandRegistry, Step};
use crate::model::{EventDef, Trigger, active_page};
use crate::script::ScriptRunners;
use crate::{GameState, INTERACT};

/// An interact event starts if its area is not farther from the player than this, in
/// meters, and the player faces it.
const INTERACT_REACH: f32 = 0.9;
/// "Faces": the cosine of the angle between the view direction of the player and the
/// direction to the event is above this (about 70 degrees to each side).
const INTERACT_FACING: f32 = 0.3;
/// An event runs at most this many commands in one step. More are a loop with no
/// wait in it: the rest runs in the next step, and the game goes on.
const MAX_COMMANDS_PER_STEP: usize = 256;

/// The text that the game shows to the player. `None`: no message. The command
/// `Message` writes it and waits; the game draws it.
#[derive(Resource, Debug, Clone, PartialEq, Default)]
pub struct MessageBox(pub Option<String>);

/// The answers of a question that the game shows under the message. `None`: no
/// question. The command `Choice` writes it and waits; the game draws it.
#[derive(Resource, Debug, Clone, PartialEq, Default)]
pub struct ChoiceBox(pub Option<Choices>);

#[derive(Debug, Clone, PartialEq)]
pub struct Choices {
    pub answers: Vec<String>,
    /// The index of the answer that has the mark.
    pub selected: usize,
}

/// An event whose commands run.
#[derive(Debug)]
pub struct RunningEvent {
    pub name: String,
    pub trigger: Trigger,
    /// The command lists that are open: the page, and in it the branch of each `If`
    /// that was taken. With each list, the index of its next command.
    stack: Vec<(Vec<Command>, usize)>,
    /// True: the current command ran in a step before and waits.
    waiting: bool,
    /// What the current command keeps between its steps (`Step::memory`).
    memory: Option<Box<dyn std::any::Any + Send + Sync>>,
}

impl RunningEvent {
    pub(crate) fn new(name: &str, trigger: Trigger, commands: &[Command]) -> Self {
        Self {
            name: name.to_owned(),
            trigger,
            stack: vec![(commands.to_vec(), 0)],
            waiting: false,
            memory: None,
        }
    }

    /// The command that runs or waits now.
    pub fn current(&self) -> Option<&Command> {
        let (list, index) = self.stack.last()?;
        list.get(*index)
    }

    /// One fixed step. True: the event has ended. `steps`: see `EventRunner::steps`.
    pub(crate) fn step(&mut self, world: &mut World, steps: &mut Option<u32>) -> bool {
        for _ in 0..MAX_COMMANDS_PER_STEP {
            // A list at its end is closed: the list around it goes on.
            while self
                .stack
                .last()
                .is_some_and(|(list, index)| *index >= list.len())
            {
                self.stack.pop();
            }
            let Some((list, index)) = self.stack.last() else {
                return true;
            };
            let command = list[*index].clone();
            // Step mode: a command starts only if a step was asked for.
            if let (false, Some(left)) = (self.waiting, steps.as_mut()) {
                if *left == 0 {
                    return false;
                }
                *left -= 1;
            }
            let start = !std::mem::replace(&mut self.waiting, true);
            if start {
                self.memory = None;
            }
            let mut opened = None;
            let mut step = Step {
                start,
                event: &self.name,
                memory: &mut self.memory,
                opened: &mut opened,
            };
            if !command.run(&mut step, world) {
                return false;
            }
            self.waiting = false;
            self.stack.last_mut().expect("tested above").1 += 1;
            // The branch of an `If`.
            if let Some(list) = opened {
                self.stack.push((list, 0));
            }
        }
        tracing::warn!(
            event = self.name,
            "{MAX_COMMANDS_PER_STEP} commands with no wait: the event goes on in the next step"
        );
        false
    }
}

/// The events of the current room, and those that run. A resource: an event that
/// asked for a room change goes on in the new room.
#[derive(Resource, Debug, Default)]
pub struct EventRunner {
    events: Vec<EventDef>,
    /// For each event: the player was in its area in the step before.
    inside: Vec<bool>,
    /// For each event: the page whose `Autorun` commands were started.
    autorun: Vec<Option<usize>>,
    /// True until the first step in a new room: a player that starts in an area did
    /// not walk into it.
    new_room: bool,
    pub(crate) running: Vec<RunningEvent>,
    /// True in a step with a press of interact that started no event: a script can
    /// take it (`script` module).
    pub(crate) interact_free: bool,
    /// Step mode, for a tool that shows an event command by command. `None`: off,
    /// the events run. `Some`: the number of commands that can still start; at 0
    /// each event waits before its next command.
    steps: Option<u32>,
}

impl EventRunner {
    /// True while an event runs that the player started or that runs by itself one
    /// time (all triggers but `Parallel`): the player has no control.
    pub fn blocks_player(&self) -> bool {
        let blocks = |event: &RunningEvent| event.trigger != Trigger::Parallel;
        self.running.iter().any(blocks)
    }

    /// The events that run, for a debug panel.
    pub fn running(&self) -> &[RunningEvent] {
        &self.running
    }

    /// The events of the current room.
    pub fn events(&self) -> &[EventDef] {
        &self.events
    }

    /// Turns the step mode on or off. On: no command starts until [`Self::step`].
    pub fn set_step_mode(&mut self, on: bool) {
        self.steps = on.then_some(0);
    }

    pub fn step_mode(&self) -> bool {
        self.steps.is_some()
    }

    /// In step mode: one more command can start.
    pub fn step_command(&mut self) {
        if let Some(steps) = &mut self.steps {
            *steps += 1;
        }
    }

    /// Starts the commands of a page of an event of the room now, whatever its
    /// trigger and its conditions are: for a test of the page. False: the room has
    /// no such event or page.
    pub fn start_event(&mut self, name: &str, page: usize) -> bool {
        let event = self.events.iter().find(|event| event.name == name);
        let Some(page) = event.and_then(|event| event.pages.get(page)) else {
            return false;
        };
        tracing::debug!(event = name, "event starts for a test");
        let running = RunningEvent::new(name, page.trigger, &page.commands);
        self.running.push(running);
        true
    }

    /// The events of a new room take the place of the old ones. A `Parallel` event of
    /// the old room stops. Each other running event goes on: one of them asked for
    /// the new room.
    pub fn set_events(&mut self, events: Vec<EventDef>) {
        self.running
            .retain(|event| event.trigger != Trigger::Parallel);
        self.inside = vec![false; events.len()];
        self.autorun = vec![None; events.len()];
        self.new_room = true;
        self.events = events;
    }

    /// One fixed step: runs the running events, then starts those whose trigger
    /// fires. `player`: position and view direction.
    pub(crate) fn step(&mut self, world: &mut World, player: Option<(Vec3, Vec3)>) {
        // An interact press that goes to a running event (it closes a message)
        // starts no other event in the same step.
        let mut blocked = self.blocks_player();
        let pressed = world.resource::<FixedActionState>().just_pressed(INTERACT);
        let mut interact = pressed && !blocked;

        let mut index = 0;
        while index < self.running.len() {
            if self.running[index].step(world, &mut self.steps) {
                self.running.remove(index);
            } else {
                index += 1;
            }
        }
        blocked |= self.blocks_player();

        let state = world.resource::<GameState>();
        for (index, event) in self.events.iter().enumerate() {
            let inside =
                |area: crate::Area| player.is_some_and(|(at, _)| area.distance(at.x, at.z) == 0.0);
            let inside = event.area.is_some_and(inside);
            let entered = inside && !self.inside[index] && !self.new_room;
            self.inside[index] = inside;

            let page_index = active_page(event, state);
            let page = page_index.map(|page| &event.pages[page]);
            // An autorun page starts again if another page was active in between.
            if page.is_none_or(|page| page.trigger != Trigger::Autorun) {
                self.autorun[index] = None;
            }
            let Some(page) = page else { continue };
            let runs = |trigger| {
                let same = |running: &RunningEvent| {
                    running.name == event.name && running.trigger == trigger
                };
                self.running.iter().any(same)
            };
            let start = match page.trigger {
                Trigger::Parallel => !runs(Trigger::Parallel),
                Trigger::Autorun => !blocked && self.autorun[index] != page_index,
                Trigger::Touch => !blocked && entered,
                Trigger::Interact => {
                    interact
                        && !blocked
                        && player
                            .zip(event.area)
                            .is_some_and(|(player, area)| faces(player, area))
                }
            };
            if !start {
                continue;
            }
            tracing::debug!(event = event.name, trigger = ?page.trigger, "event starts");
            if page.trigger == Trigger::Autorun {
                self.autorun[index] = page_index;
            }
            blocked |= page.trigger != Trigger::Parallel;
            if page.trigger == Trigger::Interact {
                interact = false;
            }
            self.running
                .push(RunningEvent::new(&event.name, page.trigger, &page.commands));
        }
        self.new_room = false;
        self.interact_free = interact;
    }
}

/// True if the player is at the area and looks toward it.
pub(crate) fn faces((position, facing): (Vec3, Vec3), area: crate::Area) -> bool {
    let [x, z] = area.nearest(position.x, position.z);
    let to_event = Vec3::new(x - position.x, 0.0, z - position.z);
    let distance = to_event.length();
    // In the area: each direction faces it.
    distance <= INTERACT_REACH
        && (distance < 1e-3 || to_event.dot(facing) / distance > INTERACT_FACING)
}

/// Runs in `Update` after the room loader: reads the events of a new room.
pub fn load_room_events(
    room: Res<CurrentRoom>,
    registry: Res<CommandRegistry>,
    mut runner: ResMut<EventRunner>,
) {
    use bevy_ecs::change_detection::DetectChanges;

    // The resource also counts as changed before the first room.
    if !room.is_changed() || room.loads == 0 {
        return;
    }
    match registry.parse_events(&room.source) {
        Ok(events) => {
            tracing::info!(events = events.len(), "events loaded");
            runner.set_events(events);
        }
        // The check of the room files stops such a room before it loads. This is
        // for a room that was loaded with no check.
        Err(error) => {
            tracing::error!("{}:{error}: the room has no events", room.path.display());
            runner.set_events(Vec::new());
        }
    }
}

/// Runs in `ResetPlay`: no event runs, no message shows. The flags and variables
/// stay: see `maki_core::ResetPlay`.
pub(crate) fn reset_play(
    mut runner: ResMut<EventRunner>,
    mut message: ResMut<MessageBox>,
    mut choice: ResMut<ChoiceBox>,
) {
    use bevy_ecs::change_detection::DetectChangesMut;

    *runner = EventRunner::default();
    message.set_if_neq(MessageBox(None));
    choice.set_if_neq(ChoiceBox(None));
}

/// Runs in `FixedUpdate`: one step of the interpreter.
///
/// It has the full world, because a command of any module can write any component
/// or resource. So each other system of `FixedUpdate` must declare its order to it:
/// `.before(run_events)` or `.after(run_events)`.
pub fn run_events(world: &mut World, players: &mut QueryState<&Interpolated, With<Player>>) {
    use bevy_ecs::change_detection::DetectChangesMut;

    // The front of the player model is its +Z.
    let player = players.iter(world).next().map(|pose| {
        let pose = pose.current;
        (pose.translation, pose.rotation * Vec3::Z)
    });
    let runner = world.resource::<EventRunner>();
    if runner.events.is_empty() && runner.running.is_empty() {
        // Nothing to run. Only a different value is a write: no resource is marked
        // as changed in each step. The press is free for a script.
        let pressed = world.resource::<FixedActionState>().just_pressed(INTERACT);
        let mut runner = world.resource_mut::<EventRunner>();
        if runner.interact_free != pressed {
            runner.bypass_change_detection().interact_free = pressed;
        }
    } else {
        // The runner is out of the world while its commands have the world.
        world.resource_scope(|world, mut runner: Mut<EventRunner>| {
            // Change detection of the runner has no reader.
            runner.bypass_change_detection().step(world, player);
        });
    }
    // The scripts, after the events: a press that started an event starts no script.
    for index in 0..world.resource::<ScriptRunners>().0.len() {
        let run = world.resource::<ScriptRunners>().0[index];
        run(world, player);
    }
}
