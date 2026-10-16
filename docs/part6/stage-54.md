# Stage 54: what the object model left

State: done. Stage file: [stage-54-cleanup.md](stage-54-cleanup.md). Spec: issue #22; tickets #24 to #29. Plan: `README.md` 2.15, `PLANNING-HANDOVER.md` group F.

Six lanes from the stage branch, at most three at a time, each merged when its checks passed; then one code review (standards and spec) and one lane of fixes.

## 1. What was built

- **Commands after the draw** (#24): a menu row, a palette row, and a command button queue their command (`maki_ui::queue_command`); `run_queued` runs the queue after `run_ui`, when `Panels` is back in the world. Command keys still run at once (before `run_ui`).
- **"New script"** (#25): `mod <name>;` goes after the last top-level `mod x;` line (also `pub mod`, also with a comment), else after the last `use`, else after the leading `//!`, `#![..]`, and blank lines. Never in `mod tests {`. After "Create" the code editor opens when a build that started after the request passes; the Build panel takes the focus only on a fail. Templates: 10 (`TEMPLATES` and `crates/engine/templates/`; the report of stage 50 says ten, which is right). The panel showed 9: the list of the combo box had a maximum height, and "Engine module" was below its scroll (found after the stage by the input script of the editor; fixed in the retro).
- **`CallScript` with a command that waits** (#26): in the call, `ScriptContext` queues the commands of the script; the row opens them as a list, as `If` opens a branch. They run in the step of the call while none waits; the next row runs after them. One handler for the editor and the game.
- **`SetField` on any object** (#27): live forms for `light` (intensity, color, radius), `prop` (collider), and `door` (seconds, open). The check reads the table of fields: an unknown field, a field that does not change in play, and a value of the wrong kind are problems; a prefab instance is checked with the components of its prefab. A change in play logs a `debug` line. The object picker lists each object with a live component (`RoomDef::live_objects`); a new row starts with the value of the field now. A write of an equal value marks nothing as changed.
- **Wires** (#28): `Start { Page(Trigger), Wire }` in the event runner; signals and area outputs start wires only in `PlayState::Play` (outside play they are dropped, as an interact outside play). Output names are constants (`TriggerArea::USED`, `ENTERED`, `LEFT`, `OUTPUTS`). A problem of a wire names it with `Wire::label` ("pressed 2").
- **The editor** (#29): F5 is play and stop only (a Startup system removes the binding of `RELOAD_ROOM` in the editor; the game keeps F5). "Add an event" for an object with no event, with undo. Tabs of two wires on one output: "used 1", "used 2" (`Wire::label`). The command rows are a left panel (200 to 600 px), the arguments fill the rest. A click on an outliner row gives up the keyboard focus.

## 2. Results

| Check | Result |
|---|---|
| Gate: a test for each defect, red before the fix | Palette and menu command that reads `Panels` (panicked: "Requested resource maki_ui::Panels does not exist"); `CallScript` with a message (panicked at `script.rs:160`); `mod` line with a tests block; wire start outside play; equal live write |
| Tests: `SetField`, wires, F5 | Light, prefab instance, check of field and kind, picker with light, prop, and door, first value of a new row; wire index, start kind, play-state gate; F5 as raw input starts the play, no frame has the reload action |
| Tests and clippy | `tools/check.sh` on the merged tree: static and dynamic, all pass. Also `cargo test --profile dynamic -p maki-engine --features dynamic --lib`: 10 pass |
| Validation and trip run (debug) | No validation error, no panic; the events and messages of the trip as before |
| Reload test | Passes |
| Symbols of the engine DLL | 61,823 of 65,535 (stage 53: 60,881) |
| Benchmark against `s52` | Run `s54`. The pictures are not byte-equal to `s52`, but `s52` is not the base: a build of `main` (`b411de5`) run back to back gives `trip_720p` and `materials_720p` byte-equal to `s54`, and `test_1080p` within its noise (two runs of the same tree: 205 pixels at 0.0005). GPU total of each scenario 5 % to 40 % lower than `s52`, with the renderer unchanged: the machine |
| Editor opens room C | No panic, no validation error, "no problem in the events" (a capture with no input) |

## 3. Deviations from the stage file

- **Picture base**: the build of `main`, not `s52`; `main` already gave other pictures than `s52` before this stage (stage 53 review or the driver).
- **F5 reload in the editor**: the binding is removed, not moved to another key (#22 allows "skipped or moved"). The room reload has no key in the editor.
- **Live fields**: `prop` has `collider` only (mesh and look need work in the renderer); `light` has no `sound` (another component); `door` has no color (part of the mesh). The check rejects a field that does not change in play.
- **The run by hand** was not done in the stage: the owner worked on the machine, and driving the editor took the desktop's input. After the stage, the input script of the editor (`tools/editor_scripts/stage54.txt`, no input of the desktop) did a part of it: File > New script and Ctrl+P open the panel; F5 plays; the dial of room C shows "dial turns. The lamp forgets."; a click on the outliner row `dial` selects it. Open for the owner: `SetField` on a light in play, two wires, the 4K layout, and a stale outliner mark on an object that is not selected.

## 4. Decisions where the stage file was silent

- Command keys keep running at once: `Panels` is in the world then.
- The component types reach the room check through a copy in `CommandRegistry`, refreshed when `ComponentTypes` changes (the check has no world).
- `SetField` on a prefab instance reaches its objects through their room item.
- A prop with a mesh file has one entity per mesh; a live write changes the first one (`ponytail:` note).
- The picker lists are cached in the event panel until the room or its directory changes.
- `Panels::run_row` is public and `#[doc(hidden)]`: the editor tests start a palette row with it.

## 5. Uncertain

- The outliner mark: no cause found in the code; the fix (give up the focus on a click) is a guess until a run by hand.
- Signals and `entered` outside play are dropped, not kept for later.
- `tools/check.sh` does not run the tests of `crates/engine/src/reload.rs` (they need `--features dynamic`).
