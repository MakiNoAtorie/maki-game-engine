# Stage 54: what the object model left

Plan: [README.md](README.md), 2.15 ("First"), and [PLANNING-HANDOVER.md](PLANNING-HANDOVER.md), group F with its part "To fix: found by the run by hand". Report: `docs/part6/stage-54.md`. Needs: stage 52 and stage 53 (merged).

Goal: the object model of stages 47 to 53 works by hand in the editor and in the game, with no panic, before the stages that build on it (object ids, the gaps of the events).
Gate: each defect of the run by hand has a test that failed before the fix; "New script" from the File menu, Ctrl+P, and the panel makes a script that builds; the dial of room C shows its message in the game and in the editor with no panic. The game plays as before: the trip run passes, the benchmark against `s52` shows the same pictures.

State: written by the session from the run by hand of 2026-10-16 and the reports of stages 50 to 53. Not seen by the owner.

Beside this stage, outside the stages (a branch and a pull request of its own): the measure of the symbol limit (2.15). `opt-level = 2` for the engine crates in the profile `dynamic`; the count of the engine DLL, the build time, and the reload test before and after. A check in CI if the gain is large. It changes `Cargo.toml` only: no shared file with this stage.

## Tickets

| Ticket | Content |
|---|---|
| (by `/to-tickets`) | The three defects |
| (by `/to-tickets`) | `SetField`: a live form for `light`, the check of the field, the components of a prefab instance in the check |
| (by `/to-tickets`) | Wires: the index of the wire in a problem, the state of the play for signals and `entered`, `Trigger::Interact`, the output names in one place |
| (by `/to-tickets`) | The rough edges of the editor |

## The way

### The defects

- **A command that reads `Panels` from a menu or the palette.** `run_ui` holds `Panels` out of the world (`crates/ui/src/lib.rs:543`). A command of a menu or the palette is queued and runs after `run_ui`, when `Panels` is back; or a command gets the panels as an argument. The simplest that fixes each command, not only "New script". A test: a command that reads `Panels` runs from the palette.
- **"New script" and `mod tests`** (`crates/engine/src/new_script.rs:136`). The line `mod <name>;` goes after the last line `mod <x>;` at the top level (a line that ends with `;`), or after the `use` lines if there is none. The unit test gets a `lib.rs` with `#[cfg(test)] mod tests { .. }`.
- **A script function that waits, from `CallScript`** (`crates/events/src/script.rs:160`, `interpreter.rs:466`). The commands that the script pushes go to a queue that the runner reads after the call; the runner is not in the world in the call. A test: a `CallScript` row whose function shows a message, then a row after it; the message shows, then the row after it runs.

### `SetField`

- `light` gets a live form: `SetField` changes the strength, the color, and the range of a light while the game runs.
- The check of a `SetField` row checks the field: a field of that component, a value of its kind.
- The check of a prefab instance sees the components of its prefab.
- A `SetField` change logs a line at the level `debug` (object, component, field, value).
- A new row starts with the value of the field now, not 1.0.
- The object picker lists each object with a component that has a live form (props, lights, doors too).

### Wires

- A problem of an output names the index of the wire.
- Signals and `entered` start wires only in `PlayState::Play`, as events.
- The output names are in one place (constants of the component that has them); the editor and the check read them there. `Trigger::Interact` is not used to stand for a running wire: a wire has its own kind of start.
- Two wires on one output: the tab shows the index ("used 1", "used 2").

### The editor

- F5 is the play of the editor only: `reload_room` of the gameplay gets another key in the editor (the game keeps F5).
- An object with no event: the event panel has "Add an event".
- The column of the arguments is next to the command rows at any width of the panel.
- "New script": the count of the templates in the report of stage 50 is corrected (9), or the tenth is added if one is missing; the panel keeps the focus after "Create" if the build fails, else the code editor opens (2.7).
- The outliner: the mark of a row that is not selected goes.

## Done when

- The gate.
- Tests: each defect, each `SetField` point, each wire point, the F5 bindings of the editor.
- Tests and clippy, static and dynamic; a validation run; the trip run; the benchmark against `s52`; the symbol count of the engine DLL.
- A run by hand in the editor of the fixed points: "New script" from the three places, the dial of room C, `SetField` on a light, two wires on one output.

## What to watch for

1. **The pictures**: no room of the benchmark changes.
2. **The order in a step**: the commands of a script function run after the call, in the same step if they do not wait; an event with a `CallScript` row keeps its order of rows.
3. **The script DLL**: a change of `ScriptContext` changes the API that scripts use; the reload test (`python tools/reload_test.py`) passes.
4. **Not in this stage**: the title as a state with a picture (stage 59, menus), rooms that use `SetPlayState` and `GiveControl` (the slice of the owner).
