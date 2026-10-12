# Stage 40: scripts that name places, and the rooms by hand

Plan: [README.md](README.md). Report: `docs/part5/stage-40.md`. Items: M13 and M14 of [`../part4/MISSING.md`](../part4/MISSING.md).

Goal: a benchmark script does not break when a room changes, and the editor makes the two rooms of the game from "New room" with no text editor.
Gate: the switch of room B is moved by 1 m and the trip script still presses it. `trip_720p` is byte-equal with the rooms of the session, or each difference has its reason.

Decisions of the owner (2026-10-12): the session is done by the assistant, with a mouse and a keyboard that a script drives; no room of the game gets the table prefab.

Written from the code at `16942f8`.

## Context in the code

- `Script` (`crates/devtools/src/bench.rs:96`) is a list of (frame, actions): from that frame on, the actions are held. `run_script` asks `script_actions` in each frame of a run.
- `TRIP_SCRIPT`, `WALK_SCRIPT`, `SHAKE_SCRIPT` (`game/src/scripts.rs`) hold frame numbers that fit rooms A and B and the speeds of the default config. `tools/bench.py` has `WALK_FRAMES` a second time: the picture of frame 180 of the walk script is the measure of the picture quality in motion.
- The player moves by the actions `MOVE_*` and `RUN` (`maki_gameplay`); "forward" is the view direction of the camera on the floor, -Z in this game. An event that runs has the control (`EventRunner::blocks_player`).
- An event has a name and an area; a room has spawn points by name (`EventRunner::events`, `RoomDef::spawns`).
- `devtools` is right of `gameplay` and `events` in the crate order.
- "File > Open", "New room", and "Save as" of the editor use the room directory of the game. `--editor-resave ROOM` writes a room file in the form of the editor.

## Prompt

```text
Stage 40: scripts that name places, and the rooms by hand.
Read docs/part5/stage-40-scripts-and-hand-rooms.md and items 13 and
14 of docs/part4/MISSING.md first.

Scripts
- A script is a list of steps: hold actions for some frames; walk
  along X or Z to a spawn point or to the area of an event of the
  current room; wait until no event has the control.
- A walk step ends at the place, at an obstacle, and at a room change.
- The trip script and the shake script name places. The walk script
  keeps its frames: its picture must not change.

The session
- Rooms A and B from "New room" in the running editor, each item and
  each event, saved under new names. Compare their content with the
  rooms of the game, and the picture of trip_720p with them in place.
- The switch of room B, moved by 1 m in the editor: the trip script
  presses it.
- List each place where a text editor would have been faster, and
  each fault that the session finds. Fix a fault that stops the work.

Out of scope: a path around obstacles, a script from a file, the
editor shows a script, comments of the rooms typed again.

Done when: tests for a hold step, a walk step, and its ends; the
trip runs with the validation layer; walk_720p is byte-equal with
stage 39; the two gates. Update OPEN_ITEMS.md ("Benchmark scripts and
room files"). Report.
```

## What to watch for

- **The walk script.** `walk_720p` byte-equal with `s39`: the same frames in the new form.
- **The trip.** Its way changes: the pictures and the times of `trip_720p` change one time. The other scenarios do not move.
- **The length of the trip.** It ends in the frames of the benchmark (700 and 100).
- **A step that ends at once.** The first step of a script, and the step after a room change.
- **A place that the room does not have.** An error line, and the script goes on.
- **A fault of the session against a fault of the driver.** A click of the script at a wrong place is not a fault of the editor.
- **Numbers typed digit by digit.** A field changes its value with each key.
