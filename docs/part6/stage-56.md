# Stage 56: the gaps of the events

State: done. Stage file: [stage-56-event-gaps.md](stage-56-event-gaps.md). Tickets #50 to #56 (pull request #49). Plan: `README.md` 2.8, 2.15.

Seven lanes from the stage branch, at most three at a time: #50, #52, #56 first; then #53, #55, #51; #54 after #53. Two merges by hand (#52 after #56, #51 after #54). Then one code review (standards and spec) and one lane of fixes.

## 1. What was built

- **Conditions from the field table** (#50): `Condition` derives `Fields`; the event panel draws a condition with `condition_row` from its table, in pages and in `If`. A new condition form is one variant with no editor code. Flag and variable fields pick from the known names, with "New...".
- **Object state** (#51): the component `state` (named switches and numbers with first values) on any object with an id. The values are in `ObjectStates`, keyed by the room file and the `ObjectId`; they stay over a change of room and go into the save (`SaveData.objects`, `#[serde(default)]`). Conditions `Switch`, `NotSwitch`, `Number`; commands `SetSwitch`, `SetNumber`, `AddNumber`; object 0 is the object of the event. A condition reads through `Facts` (the game state, the variables of a routine call, the object states). `ScriptContext` has `switch`, `number`, `set_switch`, `set_number`.
- **Routines** (#52): `<rooms>/routines/<name>.routine.ron` (argument names and a command list). `CallRoutine(file, arguments)` opens the list with the arguments as variables of the call; a command reads and writes variables through `Step`, so an argument hides a game variable of the same name. Read at the first call, forgotten on `AssetChanged`. The check reports a missing file and a wrong count of arguments. `ScriptContext::routine`. The editor: Assets > "New routine" and "Open routine", and a routine panel with the rows of the event panel.
- **Flow** (#53): `Condition::Any` ("any of"); an `If` alone in the else list of an `If` shows as one "Else if" row (no change of the format); `Loop` with `Break` (`Step::jump`); `WaitUntil`. A field of `Vec<T: Fields>` is `Kind::List`, drawn by the generic field code.
- **Numbers** (#54): `RandomVariable(name, min, max)` from `Random` (xorshift64*, own code), seeded by `[events] seed` (no key: the clock; `bench.py` and the tests: 1); condition `Variables(a, Compare, b)`.
- **Autorun again** (#55): `Page.repeat`; an autorun page with it starts again while it is the active page, at most once in a step.
- **Show, hide, move** (#56): the live component `hidden`; the extract of meshes and lights, the collision of the walk, and the areas skip a hidden object. `Show`, `Hide`, `MoveObject(object, place, seconds)` (waits until the object is there). Not saved; reset at each room load.
- **Review fixes**: `ObjectStates` and `Random` go back on `ResetPlay` (the editor no longer keeps its own copy); the check reports a state condition whose object is missing or has no `state`; routine calls nest at most 64 deep; a `Break` stops at the edge of its routine; the active-page mark of the event panel reads object state; show, hide, and move reach the children of an object (by the `parent` chain); the save at the start loads in the first frame, after the `ResetPlay` of the start; a gate test in one room.

## 2. Results

| Check | Result |
|---|---|
| Gate: chest by interact, over a transfer and back, over a save and a load | Test `a_chest_opened_by_an_interact_stays_open_over_a_transfer_and_a_save` |
| Gate: a routine with an argument from two events and a script | Routine tests of `crates/events/src/routine.rs` |
| Gate: loop with a break, "wait until"; a hidden object out of the picture and the collision | `tests/flow.rs`, `tests/objects.rs`, the extract tests of `crates/render` |
| Tests of "Done when" | Each line has a test (`crates/events/src/tests/{state,flow,numbers,repeat,objects,review}.rs`, `event_panel.rs`, `routine_panel.rs`, `events.rs`, `save.rs`) |
| Tests and clippy | `tools/check.sh` on the merged tree (`e4042a0`): format, comments, clippy and tests static and dynamic, reload test: all pass |
| Validation and trip run (debug) | No validation error, no warning; the events, messages, and room loads of the trip the same as stage 55 |
| Save at the start | A save written by a trip run loads at the next start (room B) |
| Picture gate (`bench.py gate`, on `e4042a0`) | `trip_720p`, `walk_720p`, `materials_720p`, `textured_walk_720p` equal; `test_1080p` noise (1,569 pixels at 0.0005, the noise level of stage 55). The base build reads the config with the new `[events] seed` |
| Symbols of the engine DLL | 16,629 of 65,535 (stage 55: 16,509) |
| Benchmark against `s55` | Run `s56`. GPU total of each scenario -1.6 % to +3.2 % (`heavy_lights8_1080p` +3.2 %, `trip_720p` +2.4 %, `test_2160p` -1.4 %); the pictures are equal: the machine |
| Run by hand in the editor | By the input script (no input of the desktop), on a check room (room D with a chest, removed after): the inspector shows the `state` component with its switch list; the event panel shows an `If` on `this.open`, an "Else if" row, and the `CallRoutine` row; in play the first interact sets the switch and the routine hides the door, the second gives "Already open.". "New routine" from the menu was not clicked: a headless test covers it |

## 3. Deviations from the stage file

- **The check of state conditions** (from the review): not in the stage file; the commands had it, the conditions did not.
- **Children of an object** follow show, hide, and move (from the review): poses are in room space, so the children move by the same offset.
- **The save at the start** loads in the first `Update`, not in `Startup`: the `ResetPlay` of the start ran after it and cleared the object state.

## 4. Decisions where the stage file was silent

- Object state is keyed by the room path (as the save names the room); a rename of a room file loses its state (`ponytail:` note: the file id when needed).
- A script's object 0 is its own object.
- `Random` with no seed in the config takes the clock.
- A `Loop` holds a command list, so it has no field table: an enum of its own with `add_lists`; `Break` and `WaitUntil` derive `Fields`.
- A routine call with more than 64 nested calls logs a warning and does not run.
- A `Break` with no loop in its routine logs a warning; the routine goes on.
- An empty "any of" does not hold.
- A routine panel writes its file at each change, with no undo, and stays open until "Close".
- `MoveObject` moves the position only; a door or a mover keeps its own poses (`ponytail:` note).

## 5. Uncertain

- Switch and number names are typed, not picked from the `state` component of the object.
- An object hidden in its room file is not drawn in the editor view either (the extract skips it in each app).
- The change of a condition from a flag to a variable starts empty (the old widget took the first known variable).
- A flag or variable field of a command of a script crate gets no pick of names yet.
- `Hidden` on light and mesh entities changes their archetype when it is added; the gate shows no change in the benchmark rooms, which have no hidden object.
