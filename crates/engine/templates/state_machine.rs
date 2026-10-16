//! ScriptName: a script with states. Each state has an enter, a step, and a leave.
//! Give an object the component `"script_name": ()`.

use bevy_ecs::component::Component;
use maki_engine::core::{App, Fields};
use maki_engine::events::{Script, ScriptContext, add_script};
use serde::{Deserialize, Serialize};

/// The states. Add yours, and a branch for each in `enter`, `step`, and `leave`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum State {
    #[default]
    Rest,
    Alert,
}

#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
#[fields(default)]
pub struct ScriptName {
    /// Seconds in the state `Rest` before `Alert`.
    #[field(range = 0.0..=60.0, step = 0.1)]
    pub rest: f32,
    #[serde(skip)]
    #[field(skip)]
    state: State,
    /// Seconds in the state.
    #[serde(skip)]
    #[field(skip)]
    time: f32,
}

impl Default for ScriptName {
    fn default() -> Self {
        Self {
            rest: 2.0,
            state: State::Rest,
            time: 0.0,
        }
    }
}

impl ScriptName {
    /// Leaves the state, and enters another.
    fn change(&mut self, context: &mut ScriptContext, to: State) {
        self.leave(context);
        self.state = to;
        self.time = 0.0;
        self.enter(context);
    }

    fn enter(&mut self, context: &mut ScriptContext) {
        maki_engine::tracing::debug!("{} enters {:?}", context.name(), self.state);
    }

    fn leave(&mut self, _context: &mut ScriptContext) {
        match self.state {
            State::Rest => {}
            State::Alert => {}
        }
    }
}

impl Script for ScriptName {
    fn start(&mut self, context: &mut ScriptContext) {
        self.enter(context);
    }

    fn step(&mut self, context: &mut ScriptContext) {
        self.time += context.seconds();
        match self.state {
            State::Rest if self.time >= self.rest => self.change(context, State::Alert),
            State::Rest => {}
            State::Alert => {}
        }
    }

    /// The player comes: back to `Rest`.
    fn enter_area(&mut self, context: &mut ScriptContext) {
        self.change(context, State::Rest);
    }
}

pub fn add(app: &mut App) {
    add_script::<ScriptName>(app, "script_name");
}
