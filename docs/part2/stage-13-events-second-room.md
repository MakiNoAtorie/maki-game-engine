# Stage 13: rooms as data, event system, second room

Do the steps in order: room files, then events, then content. Needs stage 11 (the `scene` crate with `MeshInstance`, `Light`, `Camera`, `Door`, `Mover`, `Collider`, `Player`).

State: done.

Checks that ran:

- Step 1: `bench.py same` of `test_720p` and `walk_720p`, run `s13a` (scene in code) against `s13b` (scene from `room_a.ron`): `ok`.
- Step 2: 20 reloads with F5. 21 `room loaded` lines, 140 `meshes removed from GPU` lines, the same table indices each time, GPU memory 35.3 MB before and after (stats panel), the same picture, no validation message.
- Step 3: a flag that was added and set in the panel is still set after a reload. Text typed in the panel field (with W and D held) does not move the player.
- Steps 4 to 6: unit tests in `crates/events` (page selection, wait, branch, the four triggers, the commands, a transfer into a bad room).
- Step 7: `--bench-script trip` plays the full loop; the log has each event in order. A copy of the two room files with another flag name and another message text gives the same loop with the new text, with no build.
- A start room with a bad command: `bad.ron:4:16-4:23: Unexpected variant named Explode ...`, and the game does not start.
- `cargo tree`: `maki-scene` uses `maki-assets`, `maki-core`; `maki-events` adds `maki-platform`, `maki-scene`.

Changes against this plan:

- The config has no room, light, or model keys any more: `start_room` and `start_spawn`. The scenarios of `tools/bench.py` with a generated scene write a room file.
- `Fade` is `maki_scene::ScreenFade`, so `render` does not depend on `events`. The tonemap pass gets a lower exposure; its shader did not change.
- `Light` has an `on` field.
- A room file is checked by each crate that reads a part of it before the room changes (`RoomValidators`): a transfer into a room with a bad event does not happen.
- `Autorun` runs one time for each time its page becomes active, not in a loop.
- The events that the player or the game starts (`Interact`, `Touch`, `Autorun`) take the control from the player, not only `Autorun`.
- The message shows in a debug panel window.
- Room A has a second doorway (east) to room B. The door in the back wall is the locked one.

Bugs found by this stage, in code of stage 12:

- A lamp that goes off stayed in the picture as a ghost: the history was short for one frame only, then the old light faded as `32 / count`. Now the history stays short until 1/1024 of the old light is left (`fade_frames` in `crates/render/src/accumulation.rs`).
- The variance in the filter passes could pass the range of a 16-bit float: infinity, then "not a number" in the next pass, which showed as thin black lines. The value has a limit now.

## Steps

### 1. Room file

- [x] Format in `crates/scene`, RON through serde: room geometry (procedural room parameters or mesh files), static props with transforms, lights, doors and movers, spawn points, event definitions (as data that `scene` does not read; see step 4).
- [x] The hardcoded scene of `game/src/scene.rs` becomes `game/assets/rooms/room_a.ron`. Remove the file `scene.rs` when it is empty.
- [x] A bad room file gives an error with the file name and the line, and the game does not start (or the transfer does not happen).
- Check: unit test, a room written by code reads back equal. `bench.py same` against the run before: `ok`.

### 2. Load and unload

- [x] Load spawns the entities of the room, each with a `RoomEntity` marker. Unload despawns all entities with the marker.
- [x] The player, the game state, and the camera are not room entities (rule 7).
- [x] Assets stay in the asset store across room changes: a path that was loaded gives the same handle.
- [x] A debug action reloads the current room.
- Check: reload 20 times. Validation is clean, GPU memory does not grow, the picture is equal. This runs the paths that never ran: mesh removal, `Gpu::destroy_blas`, empty TLAS, reset on removal (`OPEN_ITEMS.md`).

### 3. Game state

- [x] New crate `crates/events` (`maki-events`), after `scene`.
- [x] `GameState` resource: boolean flags and integer variables by name. Not reset by a room change.
- [x] Debug panel (in `game` or `ui`): flags and variables, editable.
- Check: a flag set in the panel is still set after a room reload. Typing in the panel moves no player (`OPEN_ITEMS.md`, "Keyboard capture by the UI").

### 4. Event model and page selection

- [x] Data types: an event belongs to a room and has pages. A page has conditions (flags, variables), a trigger (interact, touch, autorun, parallel), and a command list.
- [x] The active page is the highest-numbered page whose conditions hold. No page: the event does nothing.
- [x] The room file holds the events; `events` reads them from the room that `scene` loaded.
- Check: unit tests of the page selection.

### 5. Interpreter

- [x] A running event is state: the command list, the index, a wait timer. One step for each fixed tick; a command that waits returns "not done".
- [x] Commands: set flag, set variable, wait N seconds, conditional branch on a flag.
- [x] Triggers: interact (the player faces the event and presses interact), touch (the player enters its box), autorun, parallel.
- [x] Autorun events pause player control. Parallel events do not.
- [x] The debug panel lists the running events.
- Check: unit tests with a fixed tick: a wait takes N seconds of ticks and never blocks; a branch takes the correct side.

### 6. Commands that reach the world

Each command writes to a component or a resource of `scene` or `events`. No command calls `render` or `ui`.

- [x] Open/close door, move a mover: sets the state of the `Mover` (stage 11). The command waits until the motion ends.
- [x] Light on/off, intensity: writes the `Light` component of a named light.
- [x] Show message: writes a `Message` resource in `events`; waits until interact clears it. The `game` crate draws it as an egui window. That resource is the interface for a styled text box later.
- [x] Fade out/in: writes a `Fade` resource (0 to 1) in `events`; `render` extracts it and the tonemap pass multiplies by it.
- [x] Transfer: asks for a room and a spawn point; a system unloads, loads, and moves the player.
- Check: each command from a test room file. The window does not freeze during a message or a fade.

### 7. Content (data only)

- [x] Room A: the current room, the player, a closed door. Interact with the door: message "locked".
- [x] Room B: reached from room A through a doorway or transfer point. A switch: interact sets a flag, shows a message, turns the light of room B off.
- [x] Back in room A: the door is unlocked (page conditioned on the flag), and the light flickers through a parallel event.
- [x] Transfers use fade out/in.
- Check: the full loop. Then change a message text and a flag name in the RON files and run with no recompile.

## Repo state that this stage meets

- New dependencies: `ron`, `serde`.
- `render` depends on `events` for the `Fade` resource. The order permits it. If that reads wrong, put `Fade` in `scene`.
- A room change with a synchronous glTF load and a BLAS build with a GPU wait stops the frame (`OPEN_ITEMS.md`, "BLAS build stops the game", "Asynchronous loading"). The fade hides it. Stage 14 measures it.
- The temporal history of stage 12 is wrong after a transfer. The transfer asks for the global reset.

## What to watch for

- Events as code: a story beat in Rust breaks rule 8. Test: change a message text and a flag name in the RON file, run with no recompile.
- Blocking waits: the window freezes during a message or a fade. The interpreter is then not resumable.
- State lost on transfer: flags survive A, B, A. Check in the debug panel.
- `cargo tree -p maki-scene` and `-p maki-events` show no `gpu`, `render`, or `ui` crate.

## Done when

- [x] The full loop works: room A, room B, flag, back to room A with changed state.
- [x] Rooms and events change by file edit with no recompile.
- [x] Validation is clean, through room changes too.
- [x] Stop and report.
