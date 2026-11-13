# Stage 65: the gamepad

State: done. Stage file: [stage-65-gamepad.md](stage-65-gamepad.md). Tickets #110 to #113 (pull request #109). Plan: `README.md` 2.15 ("Dependencies": `gilrs`), 3.2 item 10.

Lanes: #110 (A); then #111 (B) and #112 (C) at the same time; then #113 (D). Then one code review (standards and spec) and one lane of fixes.

## 1. What was built

- **Pad inputs as keys** (#110): the pad buttons (`Pad A` to `Pad Select`, `Pad L3`, `Pad R3`, the D-pad) and the directions of the two sticks (`Left stick up` ...) are rows of the key table, with labels for the help panel and the input script. `RawInput::PadAxis` (a whole stick) and `PadLost`; `[gamepad] dead_zone` (0.2, for each axis); `ActionState::value` (0 to 1: a key 1, a stick its value past the dead zone; the largest of the inputs of an action).
- **The gamepad source** (#111): `gilrs` 0.11.2 in `crates/platform/src/gamepad.rs`, with its own filters off: the dead zone is the engine's. Player 1 is the first pad present, or the first that sends an event while there is none; its loss is `PadLost`. A focus loss starts its sticks again. No type of `gilrs` in a public API; no pad: as before.
- **The analog walk and the pad bindings** (#112): `move_direction` from the action values (length at most 1); the free walk scales the speed with the length; the grid walk takes the dominant axis above 0.5. Move: left stick and D-pad; run: Pad X; interact: Pad A; back: Pad B; menu choice: D-pad and left stick; pause: Pad Start. The debug actions stay on the keyboard.
- **The input script and a run by the pad** (#113): the step `stick left|right X Y`; pad buttons by their labels in `press`, `release`, `tap`; a sign with a choice in room D; `tools/editor_scripts/stage65.txt`.
- **Review fixes**: `gilrs` filters off; `PadAxis` carries one axis and the place of a stick is kept once, in the action state, so a stick let go without the focus leaves no axis behind; the hint of the message box names the keyboard keys only (the help panel lists all rows); the key table has keyboard, pointer, and pad parts (`is_pad` from the table); `Axis { X, Y }`; a stick key also held by a script is 1; tests of the help panel, a menu by the pad, the half-tilt speed through `steer_player`, one grid cell from a tilt through the dead zone, a key and a tilt at once.

## 2. Files

- `crates/platform/src/input.rs`, `lib.rs`: the pad rows of the key table, `PadAxis`, `PadLost`, `Stick`, `GamepadSettings`, `ActionState::value`.
- `crates/platform/src/gamepad.rs` (new), `window.rs`, `Cargo.toml`, `Cargo.toml` of the workspace, `Cargo.lock`: the `gilrs` source.
- `crates/gameplay/src/controls.rs`, `player.rs`, `game.rs`; `crates/events/src/lib.rs`: the analog walk, the grid input, the pad bindings of each module.
- `crates/devtools/src/input_script.rs`: the `stick` step.
- `game/assets/rooms/room_d.ron`, `data/text/en.text.ron`, `game/scripts/src/lib.rs`: the sign with a choice.
- `tools/editor_scripts/stage65.txt` (new): the run by hand by the pad.
- Tests: `crates/platform/src/lib.rs`, `gamepad.rs`, `crates/gameplay/src/tests/l65c.rs`, `crates/devtools/src/input_script.rs`, `crates/ui/src/lib.rs`, `game_ui.rs`, `crates/events/tests/menus.rs`.

## 3. Results

| Check | Result |
|---|---|
| Gate in code: a pad button starts and ends its action; a stick under the dead zone starts nothing, past it a value from 0 to 1; half a tilt walks at half the speed; on the grid a diagonal tilt moves one cell along its dominant axis | `crates/platform/src/lib.rs`, `crates/gameplay/src/tests/l65c.rs` |
| Tests and clippy | `tools/check.sh` on the merged tree (Rust 1.100): all pass |
| Validation run | The editor runs by the pad and a game run with no pad: no validation error |
| Trip run (debug) | Rooms A, B, A; no error |
| Picture gate (`bench.py gate`) | `trip_720p`, `walk_720p`, `materials_720p`, `textured_walk_720p` equal (the keyboard walk is the walk of stage 64); `test_1080p` noise |
| Benchmark against `s64` | Run `s65`. GPU total -8.9 % to +1.6 %: no step up; the steps down in each scenario are the machine (no change of the renderer) |
| Symbol count of the engine DLL | 20,126 (stage 64: 19,199; `gilrs` and the `windows` crate) |
| Run by hand in the editor (`stage65.txt`, room D, by the pad only) | Half a tilt walks about half as far as a full tilt; Pad X runs; Pad A at the sign shows the choice, Pad down and Pad A open the door; "Leave it" and its message close with Pad A; the pause menu moves with Pad down and closes with Pad B. On the grid: the stick moves cell by cell; a half tilt (0.375 past the dead zone) does not move |

## 4. Deviations from the stage file

- The label of the select button is "Pad Select".
- The dead zone is for each axis, not radial (a `ponytail:` note names the change if a stick feels square).
- Not tested with a real pad on this machine: the source is tested by its event mapping and a run with no pad.

## 5. Decisions where the stage file was silent

- LT and RT are buttons, down past 0.5 of their travel.
- The run is on Pad X; pause on Pad Start; the debug actions have no pad input.
- Two directions on the grid with a stick: the dominant axis; under 0.5 (after the dead zone) no move.
- A sign with a choice is in room D (no benchmark scenario uses room D): no test room had a choice for the run by the pad.

## 6. Uncertain

- No real pad was used: the hot-plug and the trigger threshold are tested by the mapping only.
- On Windows `gilrs` (Windows Gaming Input) sends events only while a window of the program has the focus.
- With the `gilrs` filters off, a pad that reports its D-pad as axes gives no D-pad buttons.
