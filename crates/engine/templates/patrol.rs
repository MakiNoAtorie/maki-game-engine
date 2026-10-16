//! ScriptName: the object walks from its place to a point and back, and waits at each
//! end. Give an object with a mesh the component `"script_name": (to: (3.0, 0.0, 0.0))`.

use bevy_ecs::component::Component;
use glam::Vec3;
use maki_engine::core::{App, Fields};
use maki_engine::events::{Script, ScriptContext, add_script};
use serde::{Deserialize, Serialize};

#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
#[fields(default)]
pub struct ScriptName {
    /// The far point, from the place of the object in the room.
    pub to: [f32; 3],
    /// Meters in a second.
    #[field(range = 0.0..=20.0, step = 0.1)]
    pub speed: f32,
    /// Seconds at each end.
    #[field(range = 0.0..=60.0, step = 0.1)]
    pub wait: f32,
    /// The place at the start, and the far point, in the room.
    #[serde(skip)]
    #[field(skip)]
    ends: [[f32; 3]; 2],
    /// The end that the object walks to.
    #[serde(skip)]
    #[field(skip)]
    target: usize,
    /// Seconds left of a wait.
    #[serde(skip)]
    #[field(skip)]
    waits: f32,
}

impl Default for ScriptName {
    fn default() -> Self {
        Self {
            to: [3.0, 0.0, 0.0],
            speed: 1.0,
            wait: 1.0,
            ends: [[0.0; 3]; 2],
            target: 1,
            waits: 0.0,
        }
    }
}

impl Script for ScriptName {
    fn start(&mut self, context: &mut ScriptContext) {
        let at = context.pose().translation;
        self.ends = [at.into(), (at + Vec3::from(self.to)).into()];
    }

    fn step(&mut self, context: &mut ScriptContext) {
        if self.waits > 0.0 {
            self.waits -= context.seconds();
            return;
        }
        let mut pose = context.pose();
        let target = Vec3::from(self.ends[self.target]);
        let way = target - pose.translation;
        let step = self.speed * context.seconds();
        if way.length() <= step {
            // At the end: wait, then back to the other end.
            pose.translation = target;
            self.target = 1 - self.target;
            self.waits = self.wait;
        } else {
            pose.translation += way.normalize() * step;
        }
        context.set_pose(pose);
    }
}

pub fn add(app: &mut App) {
    add_script::<ScriptName>(app, "script_name");
}
