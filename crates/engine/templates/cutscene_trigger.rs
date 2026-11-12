//! ScriptName: a cutscene when the player walks into the area of the object. The
//! player loses the control, a timeline plays to its end, then the control comes
//! back. The action `back` skips to the end. Give the object the components
//! `"script_name": (timeline: 3)`, `"area": (width: 2.0, depth: 2.0)`, and give the
//! object 3 a `"timeline"` (0: the timeline of this object).

use bevy_ecs::component::Component;
use maki_engine::core::{App, Fields, PlayState};
use maki_engine::events::{BACK, Builtin, Script, ScriptContext, TimelineCommand, add_script};
use maki_engine::gameplay::GameCommand;
use maki_engine::scene::{ObjectId, Timeline};
use serde::{Deserialize, Serialize};

#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
#[fields(default)]
pub struct ScriptName {
    /// The object with the timeline. 0: this object.
    pub timeline: ObjectId,
    /// True: only the first time. The flag `flag` keeps that it happened.
    #[field(label = "one time only")]
    pub once: bool,
    pub flag: String,
    /// True from the enter until the timeline stops after it played.
    #[serde(skip)]
    #[field(skip)]
    started: bool,
    /// True once the timeline plays: the event starts it some steps after the enter.
    #[serde(skip)]
    #[field(skip)]
    seen_playing: bool,
}

impl Default for ScriptName {
    fn default() -> Self {
        Self {
            timeline: ObjectId::default(),
            once: true,
            flag: "script_name_seen".to_owned(),
            started: false,
            seen_playing: false,
        }
    }
}

impl Script for ScriptName {
    fn enter_area(&mut self, context: &mut ScriptContext) {
        if self.started || (self.once && context.state().flag(&self.flag)) {
            return;
        }
        context.call(Builtin::SetFlag(self.flag.clone(), true));
        // One list: the rows run as an event, which waits for the end of the timeline.
        let rows = [
            context.command(GameCommand::SetPlayState(PlayState::Cutscene)),
            context.command(TimelineCommand::PlayTimeline(self.timeline, true)),
            context.command(GameCommand::SetPlayState(PlayState::Play)),
        ];
        context.run(rows.into_iter().flatten().collect());
        self.started = true;
    }

    fn step(&mut self, context: &mut ScriptContext) {
        if !self.started {
            return;
        }
        let timeline = if self.timeline == ObjectId::default() {
            context.get_mut::<Timeline>()
        } else {
            context.object_mut::<Timeline>(self.timeline)
        };
        let Some(playing) = timeline.map(|timeline| timeline.playing) else {
            // No timeline: the event row warns and ends at once.
            self.started = false;
            return;
        };
        if playing {
            self.seen_playing = true;
            // The skip: the end values of each track, then the wait above ends.
            if context.pressed(BACK) {
                context.call(TimelineCommand::StopTimeline(self.timeline, true));
            }
        } else if self.seen_playing {
            self.started = false;
            self.seen_playing = false;
        }
    }
}

pub fn add(app: &mut App) {
    add_script::<ScriptName>(app, "script_name");
}
