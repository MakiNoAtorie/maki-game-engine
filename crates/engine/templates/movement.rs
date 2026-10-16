//! ScriptName: moves the object by the move actions (the keys of the walk). Give an
//! object with a mesh the component `"script_name": (speed: 2.0)`.

use bevy_ecs::component::Component;
use maki_engine::core::{App, Fields};
use maki_engine::events::{Script, ScriptContext, add_script};
use maki_engine::gameplay::{MOVE_BACK, MOVE_FORWARD, MOVE_LEFT, MOVE_RIGHT};
use maki_engine::platform::Action;
use serde::{Deserialize, Serialize};

#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
#[fields(default)]
pub struct ScriptName {
    /// Meters in a second.
    #[field(range = 0.0..=20.0, step = 0.1)]
    pub speed: f32,
}

impl Default for ScriptName {
    fn default() -> Self {
        Self { speed: 2.0 }
    }
}

impl Script for ScriptName {
    fn step(&mut self, context: &mut ScriptContext) {
        // -1, 0, or 1 for an axis. Forward is -Z: away from the camera.
        let axis = |plus: Action, minus: Action| {
            let held = |action| if context.held(action) { 1.0 } else { 0.0 };
            held(plus) - held(minus)
        };
        let (x, z) = (axis(MOVE_RIGHT, MOVE_LEFT), axis(MOVE_BACK, MOVE_FORWARD));
        if x == 0.0 && z == 0.0 {
            return;
        }
        // The same speed along a diagonal.
        let step = self.speed * context.seconds() / f32::hypot(x, z);
        let mut pose = context.pose();
        pose.translation.x += x * step;
        pose.translation.z += z * step;
        context.set_pose(pose);
    }
}

pub fn add(app: &mut App) {
    add_script::<ScriptName>(app, "script_name");
}
