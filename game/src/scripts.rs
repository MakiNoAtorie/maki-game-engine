//! The benchmark scripts of this game: each plays the game with input actions, as a
//! player does with keys. The benchmark module of the engine runs them
//! (`maki_devtools::BenchModule`, `--bench-script NAME`).
//!
//! The frame numbers come from the speeds of the default config (walk 1.6 m/s, run
//! 3.6 m/s) at 60 steps in a second, and from the room files in `game/assets/rooms`:
//! a change there needs a change here.

use maki_devtools::{Script, Scripts};
use maki_events::INTERACT;
use maki_gameplay::{MOVE_BACK, MOVE_FORWARD, MOVE_LEFT, MOVE_RIGHT, RUN, TOGGLE_MOVERS};

pub const SCRIPTS: Scripts = &[
    ("walk", WALK_SCRIPT),
    ("trip", TRIP_SCRIPT),
    ("shake", SHAKE_SCRIPT),
];

/// The walk script, in room A. The player walks to the left, then to the back between
/// the cube and the left mirror, and the door opens. The last motion shows in frame
/// `WALK_FRAMES`: `tools/bench.py` dumps the picture of that frame (`WALK_FRAMES`
/// there) to measure the picture quality with motion.
const WALK_SCRIPT: Script = &[
    (40, &[MOVE_LEFT]),
    (95, &[MOVE_FORWARD]),
    // One frame: the door starts to open. It takes 1.2 s, 72 frames.
    (100, &[MOVE_FORWARD, TOGGLE_MOVERS]),
    (101, &[MOVE_FORWARD]),
    (WALK_FRAMES - 1, &[]),
];
const WALK_FRAMES: u64 = 180;

/// The trip script: the full loop of the game. Each line is one leg.
const TRIP_SCRIPT: Script = &[
    // Room A: to the right past the glass ball, to the back, into the east passage.
    (30, &[MOVE_RIGHT, RUN]),
    (92, &[MOVE_FORWARD, RUN]),
    (155, &[MOVE_RIGHT, RUN]),
    // The transfer event has the control: fade out, room B, fade in.
    (185, &[]),
    // Room B: along the room, to the back, and to the switch.
    (240, &[MOVE_RIGHT, RUN]),
    (298, &[MOVE_FORWARD, RUN]),
    (323, &[MOVE_RIGHT]),
    (345, &[]),
    // The switch, and its message.
    (350, &[INTERACT]),
    (351, &[]),
    (400, &[INTERACT]),
    (401, &[]),
    // Back to the west passage.
    (410, &[MOVE_BACK, RUN]),
    (435, &[MOVE_LEFT, RUN]),
    (530, &[]),
    // Room A again: to the door that was locked.
    (590, &[MOVE_FORWARD, RUN]),
    (640, &[]),
    (645, &[INTERACT]),
    (646, &[]),
    (700, &[INTERACT]),
    (701, &[]),
    // The door opens. Through it.
    (780, &[MOVE_FORWARD]),
    (820, &[]),
];

/// The shake script: the mirror ball of room B, whose event has the command of this
/// game (`ShakeCamera`). Not a benchmark: a check that the command runs in the game.
/// It needs room B as the start room: `[rooms] start = ".../room_b.ron"` and
/// `start_spawn = "west"` in the config.
const SHAKE_SCRIPT: Script = &[
    // From the west spawn point to the front of the ball.
    (10, &[MOVE_RIGHT]),
    (48, &[MOVE_FORWARD]),
    (58, &[]),
    // The event: the shake, and its message.
    (62, &[INTERACT]),
    (63, &[]),
    (130, &[INTERACT]),
    (131, &[]),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_script_is_in_frame_order_and_ends_with_no_action_held() {
        for (name, script) in SCRIPTS {
            assert!(script.is_sorted_by_key(|(first, _)| *first), "{name}");
            assert_eq!(script.last().unwrap().1, &[], "{name}");
        }
        // `tools/bench.py` has the same number.
        assert_eq!(WALK_SCRIPT.last().unwrap().0, WALK_FRAMES - 1);
    }
}
