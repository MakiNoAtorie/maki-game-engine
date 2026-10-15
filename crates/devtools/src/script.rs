//! Benchmark scripts: a list of steps that play the game with input actions, as a
//! player does with keys. A step holds actions for some frames, or walks to a place
//! that the room file names (a spawn point, the area of an event): a room that
//! changes does not break the script, as long as the way along the two axes is free.

use glam::Vec3;
use maki_events::EventDef;
use maki_gameplay::{MOVE_BACK, MOVE_FORWARD, MOVE_LEFT, MOVE_RIGHT, RUN};
use maki_platform::Action;
use maki_scene::RoomDef;

/// One step of a script. The next step starts in the frame where a step ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Step {
    /// Holds these actions for this many frames. The actions of frame N move the
    /// player in the fixed step of frame N + 1, and the picture shows that step in
    /// frame N + 2 (see `maki_core::interpolation`).
    Hold(u64, &'static [Action]),
    /// Walks along one axis of the floor until the player is at the X (or the Z) of
    /// a place of the current room. It ends also when the player does not move (a
    /// wall, a prop, an event that has the control), and when the room changes.
    Go(Axis, Place, Pace),
    /// Waits until no event has the control: the end of a fade, of a room change.
    Free,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X,
    Z,
}

/// A place that the room file names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// The center of the area of an event, by the name of the event.
    Event(&'static str),
    /// A spawn point, by its name.
    Spawn(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pace {
    Walk,
    /// With the run action.
    Run,
}

/// A script: its steps, one after the other. After the last step nothing is held.
pub type Script = &'static [Step];

/// The player is at a place when it is this near on the axis, in meters. More than
/// half of the way of one fixed step at the default run speed (0.06 m).
const NEAR: f32 = 0.04;
/// A player that did not move for this many frames does not get nearer.
// ponytail: frames, not fixed steps. A benchmark has one step in each frame; a run
// by hand at more than 60 frames in a second can end a `Go` step a little early.
const STILL_FRAMES: u32 = 20;
/// A `Go` step ends after this many frames, with an error in the log.
const GO_LIMIT: u64 = 1800;

/// Where a script is.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Progress {
    /// The index of the step that runs.
    pub step: usize,
    /// The frame in which that step started.
    started: u64,
    /// `World::loads` when that step started. `None`: before the first frame.
    loads: Option<u64>,
    /// The place of the player in the frame before, and for how many frames it was
    /// there.
    last: Vec3,
    still: u32,
}

/// What a script reads of the game in a frame.
pub(crate) struct World<'a> {
    pub player: Vec3,
    pub room: &'a RoomDef,
    pub events: &'a [EventDef],
    /// `CurrentRoom::loads`.
    pub loads: u64,
    /// An event has the control (`EventRunner::blocks_player`).
    pub blocked: bool,
}

impl Place {
    fn find(&self, world: &World) -> Option<Vec3> {
        match self {
            Place::Event(name) => {
                let event = world.events.iter().find(|event| event.name == *name)?;
                let [x, z] = event.area?.center;
                Some(Vec3::new(x, 0.0, z))
            }
            Place::Spawn(name) => world.room.spawn(name).map(Vec3::from),
        }
    }
}

/// The actions that walk along an axis. `positive`: to larger X, or to larger Z.
// The camera of the game looks along -Z: "forward" is -Z, and "right" is +X.
// ponytail: a game whose camera turns needs the camera here (`move_direction`).
fn walk_actions(axis: Axis, positive: bool, pace: Pace) -> &'static [Action] {
    match (axis, positive, pace) {
        (Axis::X, true, Pace::Walk) => &[MOVE_RIGHT],
        (Axis::X, true, Pace::Run) => &[MOVE_RIGHT, RUN],
        (Axis::X, false, Pace::Walk) => &[MOVE_LEFT],
        (Axis::X, false, Pace::Run) => &[MOVE_LEFT, RUN],
        (Axis::Z, true, Pace::Walk) => &[MOVE_BACK],
        (Axis::Z, true, Pace::Run) => &[MOVE_BACK, RUN],
        (Axis::Z, false, Pace::Walk) => &[MOVE_FORWARD],
        (Axis::Z, false, Pace::Run) => &[MOVE_FORWARD, RUN],
    }
}

/// The actions that a script holds in a frame. Call one time for each frame, with
/// frame numbers that go up by one. The first step starts in frame 0.
pub(crate) fn actions(
    script: Script,
    progress: &mut Progress,
    frame: u64,
    world: &World,
) -> &'static [Action] {
    progress.loads.get_or_insert(world.loads);
    if world.player == progress.last {
        progress.still += 1;
    } else {
        (progress.last, progress.still) = (world.player, 0);
    }
    // A step that ends gives the frame to the next one.
    while let Some(step) = script.get(progress.step) {
        let since = frame - progress.started;
        let (done, held): (bool, &'static [Action]) = match *step {
            Step::Hold(frames, held) => (since >= frames, held),
            Step::Free => (since >= 1 && !world.blocked, &[]),
            Step::Go(axis, place, pace) => match place.find(world) {
                Some(target) => {
                    let delta = match axis {
                        Axis::X => target.x - world.player.x,
                        Axis::Z => target.z - world.player.z,
                    };
                    let stopped =
                        since >= u64::from(STILL_FRAMES) && progress.still >= STILL_FRAMES;
                    if since >= GO_LIMIT {
                        tracing::error!(?place, "script: the player did not get to the place");
                    }
                    let done = delta.abs() <= NEAR
                        || stopped
                        || Some(world.loads) != progress.loads
                        || since >= GO_LIMIT;
                    (done, walk_actions(axis, delta > 0.0, pace))
                }
                None => {
                    tracing::error!(?place, "script: the room has no such place");
                    (true, &[])
                }
            },
        };
        if !done {
            return held;
        }
        progress.step += 1;
        progress.started = frame;
        progress.loads = Some(world.loads);
        progress.still = 0;
    }
    &[]
}

#[cfg(test)]
mod tests {
    use maki_events::Area;

    use super::*;

    const LEFT: Action = Action("left");
    const UP: Action = Action("up");

    fn world<'a>(player: [f32; 3], room: &'a RoomDef, events: &'a [EventDef]) -> World<'a> {
        World {
            player: player.into(),
            room,
            events,
            loads: 1,
            blocked: false,
        }
    }

    #[test]
    fn a_hold_step_holds_its_actions_for_its_frames() {
        const SCRIPT: Script = &[
            Step::Hold(40, &[]),
            Step::Hold(55, &[LEFT]),
            Step::Hold(25, &[UP, LEFT]),
        ];
        let room = RoomDef::default();
        let mut progress = Progress::default();
        let held: Vec<&[Action]> = (0..=620)
            .map(|frame| actions(SCRIPT, &mut progress, frame, &world([0.0; 3], &room, &[])))
            .collect();
        assert_eq!(held[1], &[]);
        assert_eq!(held[40], &[LEFT]);
        assert_eq!(held[94], &[LEFT]);
        assert_eq!(held[95], &[UP, LEFT]);
        // Nothing is held after the script.
        assert_eq!(held[120], &[]);
        assert_eq!(held[620], &[]);
    }

    #[test]
    fn a_go_step_walks_to_a_place_of_the_room_and_ends_at_a_wall() {
        const SCRIPT: Script = &[
            Step::Go(Axis::X, Place::Spawn("east"), Pace::Run),
            Step::Go(Axis::Z, Place::Event("switch"), Pace::Walk),
            Step::Free,
            Step::Go(Axis::X, Place::Spawn("nowhere"), Pace::Walk),
            Step::Hold(1, &[UP]),
        ];
        let mut room = RoomDef::default();
        room.spawns.insert("east".to_owned(), [3.0, 0.0, 0.0]);
        let events = [EventDef {
            name: "switch".to_owned(),
            area: Some(Area {
                center: [5.0, -2.0],
                size: [1.0, 1.0],
            }),
            pages: Vec::new(),
        }];
        let mut progress = Progress::default();
        let mut frame = 0;
        let mut at = |player: [f32; 3], blocked: bool, progress: &mut Progress| {
            let mut world = world(player, &room, &events);
            world.blocked = blocked;
            frame += 1;
            actions(SCRIPT, progress, frame, &world)
        };
        // To the X of the spawn point, with the run action. Near it: the next step,
        // to the Z of the event.
        assert_eq!(at([0.0; 3], false, &mut progress), &[MOVE_RIGHT, RUN]);
        assert_eq!(at([4.0, 0.0, 0.0], false, &mut progress), &[MOVE_LEFT, RUN]);
        assert_eq!(at([3.03, 0.0, 0.0], false, &mut progress), &[MOVE_FORWARD]);
        // A wall: the player does not move, and the step ends. Then the script waits
        // while an event has the control.
        for _ in 0..STILL_FRAMES {
            assert_eq!(at([3.03, 0.0, -1.0], false, &mut progress), &[MOVE_FORWARD]);
        }
        assert_eq!(at([3.03, 0.0, -1.0], true, &mut progress), &[]);
        assert_eq!(at([3.03, 0.0, -1.0], true, &mut progress), &[]);
        // A place that the room does not have: the step is left out.
        assert_eq!(at([3.03, 0.0, -1.0], false, &mut progress), &[UP]);
        assert_eq!(at([3.03, 0.0, -1.0], false, &mut progress), &[]);
    }
}
