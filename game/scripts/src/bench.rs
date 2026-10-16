//! The benchmark scripts of this game: each plays the game with input actions, as a
//! player does with keys. The benchmark module of the engine runs them
//! (`maki_engine::devtools::BenchModule`, `--bench-script NAME`); this crate adds
//! them to its list.
//!
//! A script walks to places that the room files name (a spawn point, the area of an
//! event): a room that changes does not break it, as long as the way along X and
//! then along Z is free. The frame numbers that are left are waits (a message, a
//! door that opens), and the walk script, whose picture must be the same in each
//! version of the game.

use maki_engine::core::App;
use maki_engine::devtools::{Axis, Pace, Place, Script, Step, add_bench_script};
use maki_engine::events::INTERACT;
use maki_engine::gameplay::{MOVE_FORWARD, MOVE_LEFT, TOGGLE_MOVERS};

pub fn add_scripts(app: &mut App) {
    for (name, script) in [
        ("walk", WALK_SCRIPT),
        ("trip", TRIP_SCRIPT),
        ("shake", SHAKE_SCRIPT),
    ] {
        add_bench_script(app, name, script);
    }
}

/// The walk script, in room A. The player walks to the left, then to the back between
/// the cube and the left mirror, and the door opens. The last motion shows in frame
/// `WALK_FRAMES`: `tools/bench.py` dumps the picture of that frame (`WALK_FRAMES`
/// there) to measure the picture quality with motion.
///
/// Frames, not places: the numbers come from the walk speed of the default config
/// (1.6 m/s) at 60 steps in a second, and from room A. A change there needs a change
/// here.
const WALK_SCRIPT: Script = &[
    Step::Hold(40, &[]),
    Step::Hold(55, &[MOVE_LEFT]),
    Step::Hold(5, &[MOVE_FORWARD]),
    // One frame: the door starts to open. It takes 1.2 s, 72 frames.
    Step::Hold(1, &[MOVE_FORWARD, TOGGLE_MOVERS]),
    Step::Hold(78, &[MOVE_FORWARD]),
];
#[cfg(test)]
const WALK_FRAMES: u64 = 180;

/// A press of the interact action, and a wait for what it starts.
const fn interact(wait: u64) -> [Step; 2] {
    [Step::Hold(1, &[INTERACT]), Step::Hold(wait, &[])]
}

/// The trip script: the full loop of the game.
const TRIP_SCRIPT: Script = &[
    Step::Hold(30, &[]),
    // Room A: to the right past the glass ball, to the back, into the east passage.
    Step::Go(Axis::X, Place::Spawn("east"), Pace::Run),
    Step::Go(Axis::Z, Place::Spawn("east"), Pace::Run),
    Step::Go(Axis::X, Place::Event("to room b"), Pace::Run),
    // The transfer event has the control: fade out, room B, fade in.
    Step::Free,
    // Room B: along the room, then to the switch. Its post stops the player.
    Step::Go(Axis::X, Place::Event("switch"), Pace::Run),
    Step::Go(Axis::Z, Place::Event("switch"), Pace::Walk),
    // The switch, and its message.
    interact(49)[0],
    interact(49)[1],
    interact(9)[0],
    interact(9)[1],
    // Back to the west passage.
    Step::Go(Axis::Z, Place::Spawn("west"), Pace::Run),
    Step::Go(Axis::X, Place::Event("to room a"), Pace::Run),
    Step::Free,
    // Room A again: to the door that was locked.
    Step::Go(Axis::Z, Place::Event("locked door"), Pace::Run),
    interact(54)[0],
    interact(54)[1],
    interact(79)[0],
    interact(79)[1],
    // The door opens. Through it.
    Step::Hold(40, &[MOVE_FORWARD]),
];

/// The shake script: the mirror ball of room B, whose event has the command of this
/// game (`ShakeCamera`). Not a benchmark: a check that the command runs in the game.
/// It needs room B as the start room: `[rooms] start = ".../room_b.ron"` and
/// `start_spawn = 9` (the spawn point `west`) in the config.
const SHAKE_SCRIPT: Script = &[
    Step::Hold(10, &[]),
    // From the west spawn point to the front of the ball: the ball stops the player.
    Step::Go(Axis::X, Place::Event("mirror ball"), Pace::Walk),
    Step::Go(Axis::Z, Place::Event("mirror ball"), Pace::Walk),
    // The event: the shake, and its message.
    interact(67)[0],
    interact(67)[1],
    interact(1)[0],
    interact(1)[1],
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_walk_script_ends_in_the_frame_that_the_benchmark_dumps() {
        let frames = |step: &Step| match step {
            Step::Hold(frames, _) => *frames,
            _ => panic!("the walk script has frames only"),
        };
        // `tools/bench.py` has the same number.
        assert_eq!(WALK_SCRIPT.iter().map(frames).sum::<u64>(), WALK_FRAMES - 1);
    }
}
