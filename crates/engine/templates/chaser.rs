//! ScriptName: the object rests, follows the player that it sees around walls, goes to
//! the last place where it saw the player, gives up, and goes back to its start. Give
//! an object the components `"walker": ()` (it sets the speed) and `"script_name": ()`.

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::query::With;
use glam::Vec3;
use maki_engine::core::{App, Fields, Interpolated};
use maki_engine::events::{Script, ScriptContext, add_script};
use maki_engine::gameplay::WalkGrid;
use maki_engine::scene::Player;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum State {
    /// Stands at its start.
    #[default]
    Rest,
    /// Sees the player and walks to the player.
    Follow,
    /// Lost the player: waits at the last place where it saw the player.
    Search,
    /// Gave up: walks back to its start.
    GiveUp,
}

#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
#[fields(default)]
pub struct ScriptName {
    /// How far it sees, in meters.
    #[field(range = 0.0..=100.0, step = 0.5)]
    pub distance: f32,
    /// The full angle of its view, in degrees, around the way it faces (it faces the
    /// way it walks). 360 sees all around.
    #[field(range = 1.0..=360.0, step = 1.0)]
    pub angle: f32,
    /// Seconds without the player in view before it goes back.
    #[field(range = 0.0..=60.0, step = 0.1)]
    pub give_up: f32,
    #[serde(skip)]
    #[field(skip)]
    state: State,
    /// The place at the start.
    #[serde(skip)]
    #[field(skip)]
    start: Vec3,
    /// The last place where it saw the player, and the end of the last walk to it.
    #[serde(skip)]
    #[field(skip)]
    seen_at: Vec3,
    /// Seconds since it saw the player.
    #[serde(skip)]
    #[field(skip)]
    lost: f32,
}

impl Default for ScriptName {
    fn default() -> Self {
        Self {
            distance: 8.0,
            angle: 120.0,
            give_up: 4.0,
            state: State::Rest,
            start: Vec3::ZERO,
            seen_at: Vec3::ZERO,
            lost: 0.0,
        }
    }
}

impl Script for ScriptName {
    fn start(&mut self, context: &mut ScriptContext) {
        self.start = context.pose().translation;
    }

    fn step(&mut self, context: &mut ScriptContext) {
        let was = self.state;
        self.decide(context);
        if self.state != was {
            maki_engine::tracing::debug!("{} enters {:?}", context.name(), self.state);
        }
    }
}

impl ScriptName {
    /// One step of the states: what it sees, then where it walks.
    fn decide(&mut self, context: &mut ScriptContext) {
        // The player in view, and its place.
        let player = context
            .world
            .query_filtered::<Entity, With<Player>>()
            .iter(context.world)
            .next();
        let seen = player
            .filter(|player| context.sees(*player, self.distance, self.angle))
            .and_then(|player| context.world.get::<Interpolated>(player))
            .map(|pose| pose.current.translation);
        let cell = context
            .world
            .get_resource::<WalkGrid>()
            .map_or(1.0, |grid| grid.cell());

        if let Some(at) = seen {
            self.lost = 0.0;
            // A new path only when the player left the cell of the last end.
            if self.state != State::Follow || at.distance(self.seen_at) > cell {
                context.walk_to(at);
                self.seen_at = at;
            }
            self.state = State::Follow;
            return;
        }
        match self.state {
            State::Rest => {}
            State::Follow => {
                self.state = State::Search;
                context.walk_to(self.seen_at);
            }
            State::Search => {
                self.lost += context.seconds();
                if self.lost >= self.give_up {
                    self.state = State::GiveUp;
                    context.walk_to(self.start);
                }
            }
            State::GiveUp => {
                if context.arrived() {
                    self.state = State::Rest;
                }
            }
        }
    }
}

pub fn add(app: &mut App) {
    add_script::<ScriptName>(app, "script_name");
}
