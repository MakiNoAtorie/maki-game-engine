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

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::query::{QueryState, With};
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{EntityCommands, Res, ResMut};
use bevy_ecs::world::{EntityWorldMut, Mut, World};
use glam::Vec3;
use maki_core::{Interpolated, PlayState};
use maki_platform::FixedActionState;
use maki_scene::{ComponentText, CurrentRoom, Name, Player, RoomItem};

use crate::command::{Command, CommandRegistry, Step};
use crate::model::{EventDef, EventPages, Trigger, active_page};
use crate::script::{ScriptRunners, TriggerArea};
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

/// What started a running event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// A page of the event, on its trigger.
    Page(Trigger),
    /// An output of the object: its wires. A wire blocks the player as an event that
    /// the player started.
    Wire,
}

/// An event whose commands run.
#[derive(Debug)]
pub struct RunningEvent {
    pub name: String,
    /// The entity of the object of the event. `None`: a script or a tool started the
    /// commands.
    pub(crate) entity: Option<Entity>,
    pub start: Start,
    /// The command lists that are open: the page, and in it the branch of each `If`
    /// that was taken. With each list, the index of its next command.
    stack: Vec<(Vec<Command>, usize)>,
    /// True: the current command ran in a step before and waits.
    waiting: bool,
    /// What the current command keeps between its steps (`Step::memory`).
    memory: Option<Box<dyn std::any::Any + Send + Sync>>,
}

impl RunningEvent {
    pub(crate) fn new(name: &str, start: Start, commands: &[Command]) -> Self {
        Self {
            name: name.to_owned(),
            entity: None,
            start,
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
                entity: self.entity,
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

/// What the interpreter keeps of the event of an object between two steps.
#[derive(Component, Debug, Default)]
pub(crate) struct EventMemory {
    /// The player was in the area in the step before.
    inside: bool,
    /// The page whose `Autorun` commands were started.
    autorun: Option<usize>,
}

/// The events that run. The events of the room are the components `event` of its
/// objects ([`EventPages`]). A resource: an event that asked for a room change goes on
/// in the new room.
#[derive(Resource, Debug, Default)]
pub struct EventRunner {
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

/// The outputs that a row or a script sent. Their wires start in the step of the
/// interpreter after the commands that run: a wire that sends its own output starts
/// at most one time in each step, not in a loop. Not a part of the runner: a command
/// runs while the runner is out of the world.
#[derive(Resource, Debug, Default)]
pub struct Signals(pub(crate) Vec<(Entity, String)>);

impl Signals {
    pub(crate) fn send(world: &mut World, entity: Entity, output: &str) {
        use bevy_ecs::change_detection::DetectChangesMut;

        let mut signals = world.resource_mut::<Signals>();
        // No reader of the change.
        let signals = signals.bypass_change_detection();
        signals.0.push((entity, output.to_owned()));
    }
}

impl EventRunner {
    /// True while an event runs that the player started or that runs by itself one
    /// time (all triggers but `Parallel`): the player has no control.
    pub fn blocks_player(&self) -> bool {
        let blocks = |event: &RunningEvent| event.start != Start::Page(Trigger::Parallel);
        self.running.iter().any(blocks)
    }

    /// The events that run, for a debug panel.
    pub fn running(&self) -> &[RunningEvent] {
        &self.running
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

    /// A new room: a `Parallel` event of the old room stops. Each other running event
    /// goes on: one of them asked for the new room.
    pub fn room_changed(&mut self) {
        self.running
            .retain(|event| event.start != Start::Page(Trigger::Parallel));
        self.new_room = true;
    }

    /// One fixed step: runs the running events, then starts those whose trigger
    /// fires. `player`: position and view direction.
    pub(crate) fn step(&mut self, world: &mut World, player: Option<(Vec3, Vec3)>) {
        // An interact press that goes to a running event (it closes a message)
        // starts no other event in the same step.
        let mut blocked = self.blocks_player();
        // Outside play the player starts nothing; a message reads the press itself.
        let playing = world
            .get_resource::<PlayState>()
            .is_none_or(|state| *state == PlayState::Play);
        let pressed = playing && world.resource::<FixedActionState>().just_pressed(INTERACT);
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

        let mut signals = std::mem::take(&mut world.resource_mut::<Signals>().0);
        // In the order of the objects in the file: on one press, the first event wins.
        let mut events = world.query::<(
            Entity,
            &EventPages,
            Option<&Name>,
            Option<&TriggerArea>,
            Option<&Interpolated>,
            Option<&RoomItem>,
            Option<&EventMemory>,
        )>();
        let mut events: Vec<_> = events.iter(world).collect();
        events.sort_by_key(|event| event.5.map_or(usize::MAX, |item| item.0));

        let state = world.resource::<GameState>();
        let mut memories = Vec::new();
        for (entity, pages, name, area, pose, _, memory) in events {
            let name = name.map_or("", |name| name.0.as_str());
            let at = pose.map_or(Vec3::ZERO, |pose| pose.current.translation);
            let area = area.map(|area| EventDef::area_at(area, at.to_array()));
            let before = memory.map_or((false, None), |memory| (memory.inside, memory.autorun));
            let inside =
                |area: crate::Area| player.is_some_and(|(at, _)| area.distance(at.x, at.z) == 0.0);
            let inside = area.is_some_and(inside);
            let entered = inside && !before.0 && !self.new_room;

            let page_index = active_page(&pages.pages, state);
            let page = page_index.map(|page| &pages.pages[page]);
            // An autorun page starts again if another page was active in between.
            let mut autorun = before.1;
            if page.is_none_or(|page| page.trigger != Trigger::Autorun) {
                autorun = None;
            }
            let start = page.is_some_and(|page| {
                let runs = |trigger| {
                    let same = |running: &RunningEvent| {
                        running.entity == Some(entity) && running.start == Start::Page(trigger)
                    };
                    self.running.iter().any(same)
                };
                match page.trigger {
                    Trigger::Parallel => !runs(Trigger::Parallel),
                    Trigger::Autorun => !blocked && autorun != page_index,
                    Trigger::Touch => !blocked && entered,
                    Trigger::Interact => {
                        interact
                            && !blocked
                            && player
                                .zip(area)
                                .is_some_and(|(player, area)| faces(player, area))
                    }
                }
            });
            if let (true, Some(page)) = (start, page) {
                tracing::debug!(event = name, trigger = ?page.trigger, "event starts");
                if page.trigger == Trigger::Autorun {
                    autorun = page_index;
                }
                blocked |= page.trigger != Trigger::Parallel;
                if page.trigger == Trigger::Interact {
                    interact = false;
                }
                let start = Start::Page(page.trigger);
                let mut running = RunningEvent::new(name, start, &page.commands);
                running.entity = Some(entity);
                self.running.push(running);
            }

            // The outputs of the area, with the rules of the triggers, and the outputs
            // that were sent to the object.
            let mut outputs: Vec<String> = Vec::new();
            if area.is_some() {
                // Not in the first step of a room, as the hook `interact` of a script.
                let used = interact
                    && !blocked
                    && !self.new_room
                    && player
                        .zip(area)
                        .is_some_and(|(player, area)| faces(player, area));
                let used = used
                    && pages
                        .wires
                        .iter()
                        .any(|wire| wire.output == TriggerArea::USED);
                if used {
                    interact = false;
                    outputs.push(TriggerArea::USED.to_owned());
                }
                if entered && !blocked {
                    outputs.push(TriggerArea::ENTERED.to_owned());
                }
                if before.0 && !inside {
                    outputs.push(TriggerArea::LEFT.to_owned());
                }
            }
            signals.retain(|(to, output)| {
                let of_this = *to == entity;
                if of_this {
                    outputs.push(output.clone());
                }
                !of_this
            });
            // Outside play no wire starts; the outputs of this step are dropped.
            if !playing {
                outputs.clear();
            }
            for output in &outputs {
                // The wires of an output run one after the other, in the order of the
                // list: one command list.
                let wires = pages.wires.iter().filter(|wire| wire.output == *output);
                let commands: Vec<Command> = wires.flat_map(|wire| wire.commands.clone()).collect();
                if commands.is_empty() {
                    continue;
                }
                tracing::debug!(event = name, output, "wires start");
                let mut running = RunningEvent::new(name, Start::Wire, &commands);
                running.entity = Some(entity);
                self.running.push(running);
                blocked = true;
            }

            if memory.is_none_or(|memory| (memory.inside, memory.autorun) != (inside, autorun)) {
                memories.push((entity, EventMemory { inside, autorun }));
            }
        }
        for (entity, memory) in memories {
            world.entity_mut(entity).insert(memory);
        }
        self.new_room = false;
        self.interact_free = interact;
    }
}

/// Starts the commands of a page of the event of an object now, whatever its trigger
/// and its conditions are: for a test of the page. `name`: of the object. False: the
/// room has no such event or page.
pub fn start_event(world: &mut World, name: &str, page: usize) -> bool {
    let mut events = world.query::<(Entity, &EventPages, &Name)>();
    let found = events
        .iter(world)
        .find(|(_, _, other)| other.0 == name)
        .and_then(|(entity, pages, _)| Some((entity, pages.pages.get(page)?.clone())));
    let Some((entity, page)) = found else {
        return false;
    };
    tracing::debug!(event = name, "event starts for a test");
    let mut running = RunningEvent::new(name, Start::Page(page.trigger), &page.commands);
    running.entity = Some(entity);
    world.resource_mut::<EventRunner>().running.push(running);
    true
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

/// Runs in `Update` after the room loader: a new room.
pub fn load_room_events(
    room: Res<CurrentRoom>,
    mut runner: ResMut<EventRunner>,
    mut signals: ResMut<Signals>,
) {
    use bevy_ecs::change_detection::DetectChanges;

    // The resource also counts as changed before the first room.
    if !room.is_changed() || room.loads == 0 {
        return;
    }
    runner.room_changed();
    // The objects of the outputs are gone.
    signals.0.clear();
}

/// Reads the text of a component `event` into its entity. The commands are read with
/// the registry, a resource: so the read waits until the commands of the loader
/// run with the world.
pub(crate) fn insert_event(
    text: &ComponentText,
    entity: &mut EntityCommands,
) -> Result<(), String> {
    let text = text.clone();
    entity.queue(move |mut entity: EntityWorldMut| {
        let registry = entity.world().resource::<CommandRegistry>().clone();
        match registry.parse_pages(&text) {
            Ok(pages) => drop(entity.insert(pages)),
            // The check of the room files stops such a room before it loads.
            Err(error) => tracing::error!("an event left out: {error}"),
        }
    });
    Ok(())
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
    // The runner is out of the world while its commands have the world.
    world.resource_scope(|world, mut runner: Mut<EventRunner>| {
        // Change detection of the runner has no reader.
        runner.bypass_change_detection().step(world, player);
    });
    // The scripts, after the events: a press that started an event starts no script.
    for index in 0..world.resource::<ScriptRunners>().0.len() {
        let run = world.resource::<ScriptRunners>().0[index];
        run(world, player);
    }
}
