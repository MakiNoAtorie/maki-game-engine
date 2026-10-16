# Stage 52: wires between objects, and "Send a signal"

State: done. Stage file: [stage-52-wires.md](stage-52-wires.md) (written by the session; the owner did not see it). Plan: `README.md`, 2.1, 2.8, 2.14, item 3 of 3.2. Ticket: GitHub issue #14.

Stage 52 came after stage 53 (a parallel lane, merged first): its benchmark base is `s53`.

## 1. What was built

- **Outputs**: a component type names its outputs (`add_outputs(app, "area", &[...])`); they are in the command registry beside the script functions. The area (`TriggerArea`) has `used`, `entered`, `left`, with the rules of the triggers: none in the first step of a room, none while an event blocks the player (`left`: see 3), one press starts one thing (an event first, then a wire, then a script).
- **Wires**: a wire is an output of the object and a command list (`maki_events::Wire`), in the component `event` beside the pages: `"event": (wires: [(output: "used", commands: [OpenDoor("door")])])`. An input is a registered command. The wires of one output run one after the other, in the order of the list, as one running event of the object.
- **Signals**: the row `Signal(object, output)` sends an output of another object; a script sends its own with `ScriptContext::signal`. A signal waits in a resource (`Signals`) and its wires start in the next pass of the interpreter: a wire that sends its own output starts at most one time in each step.
- **The checks**: a wire joins an output of a component of its object; `Signal` names an object with that output. The command lists of the problems are the pages, then the wires (`EventPages::list_mut`).
- **The editor**: the event panel shows the wires as tabs under the pages: the output from a picker of the outputs of the object, "Add a wire", "Delete the wire", the command rows as for a page. `Signal` has pickers of the object and of its outputs. "Add > Button": a small cube with an area and a wire on `used` that opens the first door or mover. A change is one step of the history. A rename follows into `Signal` rows.
- **Room D** (`room_d.ron`, new): a button opens the door of the back wall by a wire; a pad at the left sends `used` of the button with `Signal`.

## 2. Results

| Check | Result |
|---|---|
| Gate: a button opens a door by a wire | `the_button_of_room_d_opens_the_door_by_a_wire` (game); validation run in room D: no error, no warning |
| Gate: "Send a signal" | `a_signal_row_and_a_script_send_outputs_to_wires` (events) |
| Gate: make, change, delete a wire with undo | `a_button_has_a_wire_and_a_change_of_a_wire_is_one_step_of_the_history` (editor) |
| Test: the outputs of the area | `the_area_sends_its_outputs_to_the_wires_of_its_object` |
| Test: a loop of signals, the order on one press | `a_wire_that_sends_its_own_output_starts_once_in_a_step_and_an_event_goes_first` |
| Test: the checks | `the_check_finds_a_wrong_output_of_a_wire_and_of_a_signal` |
| Test: the rooms | `the_editor_writes_each_room_file_with_the_same_content` (game, with room D): 10 events, 11 pages |
| Trip run (debug, validation layers) | The same events as in stages 51 and 53; no error, no warning |
| Tests and clippy | Static and dynamic (`tools/check.sh`): all pass, clippy clean |
| Reload test | `python tools/reload_test.py`: load 0, build 1.5 s, load 1 with the new text |
| Symbols of the engine DLL | 61,315 of 65,535 (stage 53: 60,881) |
| Benchmark against `s53` | Run `s52`: pictures of `trip_720p`, `walk_720p`, `test_1080p`, `materials_720p`, `textured_walk_720p` byte-equal to `s53`. GPU total within 3 % but `trip_720p` +20 %; back to back with a build of `cad730e` (two rounds): -0.8 % and +1.5 %. The step is the machine |

## 3. Deviations from the stage file

- **No component `wires`**: the wires are a field of the component `event` (`EventPages::wires`). One component of the object holds its logic with no code; the document, the undo, the renames, the checks, and the command rows of the editor reach the wires with no new path.
- **The pad of room D sends the signal**, not the lamp: a light object with an event would change the order of the lights for the renderer (stage 50).
- **`left`** is sent while an event blocks the player, as the hook `leave_area` of a script.
- **A signal to an object with outputs and no wire** passes the check and does nothing (a warning in the log when the object has no event).

## 4. Decisions where the stage file was silent

- A running wire has the trigger `Interact`: it blocks the player as an event that the player started.
- `CheckContext` has the registry (the script functions and the outputs), in the place of the list of the functions of stage 51.
- A signal of a room that is left is dropped (`load_room_events`).
- The order on one press: an event, then a wire on `used`, then a script.

Code review (`/code-review` since `cad730e`), fixed: the wires of one output ran at the same time (now one list, in order); `used` could fire in the first step of a room; the comments of the signal timing were wrong. Not fixed (judgement calls): the output names as constants, named structs for the pairs of the lists of the editor, a trigger of its own for a wire, one iterator of the command lists.

## 5. Files

| File | Role |
|---|---|
| `crates/events/src/model.rs` | `Wire`, `EventPages::wires`, `list_mut`, `EventDef::wires` |
| `crates/events/src/interpreter.rs` | The outputs of the area, `Signals`, the start of the wires |
| `crates/events/src/command.rs`, `check.rs` | The outputs of the registry; the checks of a wire and of `Signal`; `CheckContext::registry` |
| `crates/events/src/builtin.rs`, `script.rs`, `lib.rs` | `Signal`; `add_outputs`, `ScriptContext::signal`; the outputs of the area |
| `crates/events/src/tests.rs` | Tests of outputs, wires, signals, checks |
| `crates/editor/src/event_panel.rs`, `lib.rs` | The wire tabs, the pickers of `Signal`, the rows of a page or a wire |
| `crates/editor/src/ops.rs`, `document.rs`, `events.rs` | "Add > Button", renames in `Signal`, the wires in the events of a room and in `spawn_users` |
| `game/assets/rooms/room_d.ron` (new), `game/scripts/src/lib.rs` | The wire room; its tests |

## 6. Uncertain

- **Not run by hand in the editor**: the wire tabs and the pickers. The tests do not draw them.
- **Signals and `entered` start wires outside the state play**; only a press of interact is of play only (stage 53).
- **The problems of an output of a wire** have no index of the wire: the editor marks the object, not the tab.
- **The symbol count** grows: 61,315 of 65,535, 4,220 left.
