# Stage 40: scripts that name places, and the rooms by hand

State: built. The two gates are met.

- The switch of room B (post, button, pilot light, event area) was moved by 1 m in the editor; the trip script pressed it and ran to its end (`pictures/s40_switch_moved.png`).
- Rooms A and B were made from "New room" in the running editor, with each item and each event. Their content is equal to the rooms of the game, and `trip_720p` and `walk_720p` with them in place are byte-equal.

The session was done by the assistant: a script moved the mouse and typed the keys (decision of the owner). No person used the editor.

## 1. What was built

- **Steps** (`crates/devtools/src/script.rs`): a script is a list of `Step`. `Hold(frames, actions)`; `Go(axis, place, pace)`: walk or run along X or Z until the player is at the X (or Z) of a place; `Free`: wait until no event has the control. A place is `Place::Spawn(name)` or `Place::Event(name)` (the center of its area), of the current room.
- **The ends of a `Go` step**: within 0.04 m of the place on the axis; the player did not move for 20 frames (a wall, a prop, an event that has the control); the room changed; 1800 frames (an error line). A place that the room does not have: an error line, and the next step.
- **The scripts of the game** (`game/src/scripts.rs`): the trip and the shake script name spawn points and events. The frames that are left in them are waits (a message, the door that opens). The walk script has the same frames as before, in the new form.
- **A box with no size** (`Mesh::cuboid`): the corners can be in any order, and a side has 1 mm at least. Found by the session: see section 6.

## 2. Numbers

- `cargo test --workspace`: 148 tests, 149 with `--features editor` (3 new, 1 replaced). `cargo clippy --workspace --all-targets` clean with and without `--features editor`. `cargo fmt --check` clean.
- New tests: `a_hold_step_holds_its_actions_for_its_frames` (the frames of the old form), `a_go_step_walks_to_a_place_of_the_room_and_ends_at_a_wall` (to a spawn point, to an event, the end at a wall, the wait, a place that is not there), `a_box_with_no_height_or_with_its_corners_in_the_other_order_is_a_valid_mesh`, `the_walk_script_ends_in_the_frame_that_the_benchmark_dumps`.
- Debug build with the validation layer, the trip script, 900 frames: no error. The trip ends in frame 800 (before: 820): it fits the 800 frames of the benchmark.
- `bench.py same SCENARIO s39 s40`: `walk_720p`, `textured_walk_720p`, `heavy_1080p`, `stress_720p`, `materials_720p`: `ok`, byte-equal. `trip_720p` differs: the way of the player is another one (section 4).
- GPU medians, `s39` against `s40`, `gpu_total`: -0.5 % to +0.6 % in 25 scenarios. `textured_1080p` had +3.7 % in the run; a second run of it had -0.1 % (4.659 ms to 4.654 ms): machine noise. `trip_720p`: 1.920 ms to 1.912 ms.

### The rooms of the session

- `docs/part5/stage-40-rooms/hand_a.ron`, `hand_b.ron`: as the editor saved them.
- Content: the rooms of the game, written by `--editor-resave` and with the comment lines removed, are the same text as the two files (`diff`: no line).
- Pictures: with the two files in place of `room_a.ron` and `room_b.ron`, `bench.py same trip_720p s40 x40` and `same walk_720p s40 x40`: `ok`, byte-equal. The side run `x40` is removed from the history.
- The comments of the rooms were not typed again (out of scope): the files of the game were not replaced.

### The trip with the switch moved

In the editor: the Z of the post, the button, the pilot light, and the center of the event area from -1.5 to -0.5, saved under a new name; then the trip in a copy of the rooms with that file as room B. The player stops at the post at z = -0.05 (before: -1.05), the event `switch` starts, and the trip goes on to the door of room A.

## 3. Files

- `crates/devtools/src/script.rs` (new): the steps, the places, the runner, its tests.
- `crates/devtools/src/bench.rs`, `lib.rs`, `Cargo.toml`: `run_script` uses the runner; the crate depends on `maki-events`.
- `crates/events/src/lib.rs`, `interpreter.rs`: `load_room_events` is public, for the order of a system.
- `game/src/scripts.rs`: the three scripts.
- `crates/assets/src/mesh.rs`: `Mesh::cuboid`.
- `docs/part5/stage-40-rooms/` (the two rooms of the session), `docs/part5/pictures/s40_switch_moved.png`, `docs/part5/stage-40-scripts-and-hand-rooms.md`, `OPEN_ITEMS.md`, `tools/bench_history.csv`, `tools/bench_env.csv`.

## 4. Deviations

- **The pictures and the times of `trip_720p` changed.** The player now goes to the X of the spawn point `east` (3.19, before 3.01), walks to the switch from the front (before: from the left), and stands at other places. From `s40` on, `trip_720p` compares with `s40`.
- **The walk script keeps frame numbers.** Its last frame is the picture that `bench.py` measures; a step that ends "at a place" ends in another frame when a speed changes.
- **The session was scripted**, not done by a person: it shows that each value can be set in the editor, not how long a person needs.
- **The stage file was written with the stage.**

## 5. Decisions

- **A walk step goes along one axis.** The trip is: along X, then along Z. No path around a prop. A step that ends at an obstacle gives no error: the post of the switch is such an obstacle, and it is the place to stand.
- **"Stopped" is 20 frames with no move.** In frames, not fixed steps (a `ponytail` comment): a benchmark has one step in each frame.
- **The camera looks along -Z** in the runner (a `ponytail` comment): "forward" is the view direction of the camera in the game.
- **The waits for a message are frames**, not "until the message shows": they do not depend on the room.
- **The runner is in `devtools`**, with the benchmark: it reads `EventRunner` and `CurrentRoom`, and writes `ScriptedActions`.
- **`Mesh::cuboid` makes a thin plate** of a box with no size, in place of an error: a room file from a text editor with such a box loads too.

## 6. What the session found

Faults of the editor and the engine:

1. **A crash.** A number field changes its value with each key. "0.9" in the "to Y" field of a cuboid is "0" for a moment: a box with no height, normals that are not numbers, and the mesh generator ended the game. Fixed (`Mesh::cuboid`), with a test.
2. **A text field in a menu** (stage 39): fixed there. "Save as" by the mouse worked in this session.
3. **The "Save as" field keeps its last text.** A second "Save as" with a click and a name gives `hand_bmoved_b.ron`: the text must be selected first. Not fixed.
4. **`ShakeCamera` has no widgets.** A command of the game shows as one text field with the text of the room file (`ShakeCamera(0.6,0.06)`). A module can add widgets (`add_argument_editor`); the game did not.
5. **A new light gets the name of nothing**, but a name of a light that is equal to the name of an event is accepted. Seen by a fault of the driver (below); it made no error.

Where a text editor would have been faster:

- Nine commands of one loop (`flicker`): 9 times "Add", a name, and a number, against one copied line.
- A second mirror: "Duplicate" helped, but each of its 8 numbers was typed again.
- The two messages with their texts: equal.
- The comments of the file (not typed).

Faults of the driver, not of the editor:

- A click of the script in the picture selected the lamp, and the next typed values went into its name and its radius. The saved file showed it; the rename back to `lamp` also changed the two commands that named it.

Where the editor was faster or safer: the defaults of a new command (the flag `power_off`, the first light, the first door, 0.4 s for a fade), the lists for a room file and a spawn point of `Transfer`, and the sound file list.

## 7. Uncertain

- A script on a display with more than 60 frames in a second, run by hand: a `Go` step can end some frames early ("stopped" counts frames).
- A `Go` step that ends at a prop that is not the place gives no word. The `script` line of the debug log has the place of the player at each change of the held actions.
- The event `mirror ball` of the shake script was not run in this stage (not a benchmark).
- The session typed no comment and used no gizmo: each place was typed as a number.

## 8. What to watch for: results

- **The walk script**: `walk_720p` byte-equal with `s39`.
- **The trip**: new pictures, with the reason; 25 other scenarios with no step.
- **The length of the trip**: the last step ends in frame 800.
- **A step that ends at once**: found by the test: the first step of a script took the room of the start for a room change. Fixed before the first run.
- **A place that the room does not have**: an error line, and the next step (test).
- **A fault of the session against a fault of the driver**: apart in section 6.
- **Numbers typed digit by digit**: the crash of section 6.

## 9. Open items

- "Benchmark scripts and room files": the row has the new state: places for the trip, frames for the walk, no way around a prop, no word in the editor.
