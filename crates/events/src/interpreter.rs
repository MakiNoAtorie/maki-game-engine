//! The event interpreter: starts events on their triggers and runs their commands.
//!
//! A running event is state, not a function call: the command lists that are open,
//! the index of the current command, and what that command waits for. In each fixed
//! step the interpreter runs commands until one must wait (a wait time, a door that
//! turns, a message that the player has not closed). The game loop never stops for
//! an event.
//!
//! Each command writes a component or a resource of the scene crate or of this
//! crate. No command calls the renderer or the user interface: they read what the
//! commands wrote.

use bevy_ecs::query::With;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Query, Res, ResMut, SystemParam};
use glam::Vec3;
use maki_core::{Interpolated, Time};
use maki_platform::{Action, FixedActionState};
use maki_scene::{
    CurrentRoom, Light, Mover, MoverState, Name, Player, RoomRequest, RoomTarget, ScreenFade,
};

use crate::GameState;
use crate::model::{Command, EventDef, Trigger, active_page, parse_events};

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

/// An event whose commands run.
#[derive(Debug, Clone, PartialEq)]
pub struct RunningEvent {
    pub name: String,
    pub trigger: Trigger,
    /// The command lists that are open: the page, and in it the branch of each `If`
    /// that was taken. With each list, the index of its next command.
    stack: Vec<(Vec<Command>, usize)>,
    /// True: the current command ran in a step before and waits.
    waiting: bool,
    /// For `Wait`: the seconds that are left.
    seconds_left: f32,
    /// For `Transfer`: `CurrentRoom::loads` when the room was asked for.
    loads_before: u64,
}

/// What the commands can reach.
pub(crate) struct EventWorld<'a, 'w, 's> {
    /// Length of the fixed step.
    pub seconds: f32,
    /// The player pressed interact in this step.
    pub interact: bool,
    pub state: &'a mut GameState,
    pub message: &'a mut MessageBox,
    pub fade: &'a mut ScreenFade,
    pub request: &'a mut RoomRequest,
    pub room: &'a CurrentRoom,
    pub named: &'a mut NamedEntities<'w, 's>,
}

/// The entities of the room that a command finds by name.
#[derive(SystemParam)]
pub struct NamedEntities<'w, 's> {
    movers: Query<'w, 's, (&'static Name, &'static mut Mover)>,
    lights: Query<'w, 's, (&'static Name, &'static mut Light)>,
}

impl EventWorld<'_, '_, '_> {
    /// Sends each mover of this name to pose B or A (only `start`), and tells if all
    /// of them are there.
    fn send_mover(&mut self, name: &str, to_b: bool, start: bool) -> bool {
        let (mut found, mut arrived) = (false, true);
        for (mover_name, mut mover) in self.named.movers.iter_mut() {
            if mover_name.0 != name {
                continue;
            }
            found = true;
            let target = if to_b {
                MoverState::AtB
            } else {
                MoverState::AtA
            };
            if mover.state == target {
                continue;
            }
            // Only a mover that is not there is written: a write is a change.
            if start {
                if to_b {
                    mover.go_to_b();
                } else {
                    mover.go_to_a();
                }
            }
            arrived = false;
        }
        if !found && start {
            tracing::warn!(
                name,
                "an event names a door or mover that the room does not have"
            );
        }
        arrived
    }

    fn set_light(&mut self, name: &str, change: impl Fn(&Light) -> Light) {
        let mut found = false;
        for (light_name, mut light) in self.named.lights.iter_mut() {
            if light_name.0 == name {
                found = true;
                let new = change(&light);
                // A write is a change of the lighting for the renderer.
                if *light != new {
                    *light = new;
                }
            }
        }
        if !found {
            tracing::warn!(name, "an event names a light that the room does not have");
        }
    }
}

impl RunningEvent {
    fn new(name: &str, trigger: Trigger, commands: &[Command]) -> Self {
        Self {
            name: name.to_owned(),
            trigger,
            stack: vec![(commands.to_vec(), 0)],
            waiting: false,
            seconds_left: 0.0,
            loads_before: 0,
        }
    }

    /// The command that runs or waits now.
    pub fn current(&self) -> Option<&Command> {
        let (list, index) = self.stack.last()?;
        list.get(*index)
    }

    /// One fixed step. True: the event has ended.
    fn step(&mut self, world: &mut EventWorld) -> bool {
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
            let start = !std::mem::replace(&mut self.waiting, true);
            if !self.run(&command, start, world) {
                return false;
            }
            self.waiting = false;
            self.stack.last_mut().expect("tested above").1 += 1;
            if let Command::If(flag, then, otherwise) = command {
                let branch = if world.state.flag(&flag) {
                    then
                } else {
                    otherwise
                };
                self.stack.push((branch, 0));
            }
        }
        tracing::warn!(
            event = self.name,
            "{MAX_COMMANDS_PER_STEP} commands with no wait: the event goes on in the next step"
        );
        false
    }

    /// Runs the current command. `start`: this is its first step. False: the command
    /// waits, and runs again in the next step.
    fn run(&mut self, command: &Command, start: bool, world: &mut EventWorld) -> bool {
        match command {
            Command::SetFlag(name, value) => world.state.set_flag(name, *value),
            Command::SetVariable(name, value) => world.state.set_variable(name, *value),
            Command::AddVariable(name, value) => {
                let sum = world.state.variable(name).saturating_add(*value);
                world.state.set_variable(name, sum);
            }
            Command::Wait(seconds) => {
                if start {
                    self.seconds_left = *seconds;
                }
                self.seconds_left -= world.seconds;
                // The small number: the sum of 60 steps of 1/60 s is not exactly 1 s.
                return self.seconds_left <= 1e-5;
            }
            // `step` opens the branch.
            Command::If(..) => {}
            Command::OpenDoor(name) => return world.send_mover(name, true, start),
            Command::CloseDoor(name) => return world.send_mover(name, false, start),
            Command::LightOn(name) => world.set_light(name, |light| Light { on: true, ..*light }),
            Command::LightOff(name) => world.set_light(name, |light| Light {
                on: false,
                ..*light
            }),
            Command::LightIntensity(name, intensity) => world.set_light(name, |light| Light {
                intensity: *intensity,
                ..*light
            }),
            Command::Message(text) => {
                if start {
                    tracing::debug!(event = self.name, text, "message");
                    world.message.0 = Some(text.clone());
                    return false;
                }
                if !world.interact {
                    return false;
                }
                world.message.0 = None;
            }
            Command::FadeOut(seconds) | Command::FadeIn(seconds) => {
                let part = if *seconds > 0.0 {
                    world.seconds / seconds
                } else {
                    1.0
                };
                let out = matches!(command, Command::FadeOut(_));
                let fade = world.fade.0 + if out { part } else { -part };
                world.fade.0 = fade.clamp(0.0, 1.0);
                return world.fade.0 == if out { 1.0 } else { 0.0 };
            }
            Command::Transfer(room, spawn) => {
                if start {
                    let directory = world.room.path.parent().unwrap_or(std::path::Path::new(""));
                    world.request.0 = Some(RoomTarget {
                        path: directory.join(room),
                        spawn: Some(spawn.clone()),
                    });
                    self.loads_before = world.room.loads;
                    return false;
                }
                if world.room.loads == self.loads_before {
                    // The loader has not run yet (two fixed steps in one frame).
                    if world.request.0.is_some() {
                        return false;
                    }
                    // The loader logged why. The event goes on in the old room.
                    tracing::error!(event = self.name, room, "the transfer did not happen");
                }
            }
        }
        true
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
    running: Vec<RunningEvent>,
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
    pub(crate) fn step(&mut self, world: &mut EventWorld, player: Option<(Vec3, Vec3)>) {
        // An interact press that goes to a running event (it closes a message)
        // starts no other event in the same step.
        let mut blocked = self.blocks_player();
        let interact = world.interact && !blocked;

        let mut index = 0;
        while index < self.running.len() {
            if self.running[index].step(world) {
                self.running.remove(index);
            } else {
                index += 1;
            }
        }
        blocked |= self.blocks_player();

        for (index, event) in self.events.iter().enumerate() {
            let inside = player.is_some_and(|(at, _)| event.area.distance(at.x, at.z) == 0.0);
            let entered = inside && !self.inside[index] && !self.new_room;
            self.inside[index] = inside;

            let page_index = active_page(event, world.state);
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
                    interact && !blocked && player.is_some_and(|player| faces(player, event))
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
            self.running
                .push(RunningEvent::new(&event.name, page.trigger, &page.commands));
        }
        self.new_room = false;
    }
}

/// True if the player is at the area of the event and looks toward it.
fn faces((position, facing): (Vec3, Vec3), event: &EventDef) -> bool {
    let [x, z] = event.area.nearest(position.x, position.z);
    let to_event = Vec3::new(x - position.x, 0.0, z - position.z);
    let distance = to_event.length();
    // In the area: each direction faces it.
    distance <= INTERACT_REACH
        && (distance < 1e-3 || to_event.dot(facing) / distance > INTERACT_FACING)
}

/// Runs in `Update` after the room loader: reads the events of a new room.
pub(crate) fn load_room_events(room: Res<CurrentRoom>, mut runner: ResMut<EventRunner>) {
    use bevy_ecs::change_detection::DetectChanges;

    // The resource also counts as changed before the first room.
    if !room.is_changed() || room.loads == 0 {
        return;
    }
    match parse_events(&room.source) {
        Ok(events) => {
            tracing::info!(events = events.len(), "events loaded");
            runner.set_events(events);
        }
        // `validate_events` stops such a room before it loads. This is for a game
        // that did not add the check.
        Err(error) => {
            tracing::error!("{}:{error}: the room has no events", room.path.display());
            runner.set_events(Vec::new());
        }
    }
}

/// Runs in `FixedUpdate`: one step of the interpreter. A system that writes a mover
/// or a light in `FixedUpdate` runs `.after(run_events)`.
#[allow(clippy::too_many_arguments)]
pub fn run_events(
    time: Res<Time>,
    actions: Res<FixedActionState>,
    room: Res<CurrentRoom>,
    mut runner: ResMut<EventRunner>,
    mut state: ResMut<GameState>,
    mut message: ResMut<MessageBox>,
    mut fade: ResMut<ScreenFade>,
    mut request: ResMut<RoomRequest>,
    mut named: NamedEntities,
    players: Query<&Interpolated, With<Player>>,
) {
    use bevy_ecs::change_detection::DetectChangesMut;

    // Nothing to do: no write, so no resource is marked as changed in each step.
    if runner.events.is_empty() && runner.running.is_empty() {
        return;
    }
    // The front of the player model is its +Z.
    let player = players.iter().next().map(|pose| {
        let pose = pose.current;
        (pose.translation, pose.rotation * Vec3::Z)
    });
    // Change detection of these resources has no reader that does work on a change.
    let mut world = EventWorld {
        seconds: time.fixed_delta.as_secs_f32(),
        interact: actions.just_pressed(Action::Interact),
        state: state.bypass_change_detection(),
        message: message.bypass_change_detection(),
        fade: fade.bypass_change_detection(),
        request: request.bypass_change_detection(),
        room: &room,
        named: &mut named,
    };
    runner.bypass_change_detection().step(&mut world, player);
}
