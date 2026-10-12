# Stage 38: save and load

State: built. Gate: met. The trip script ran to the switch of room B and the game ended (460 frames); the next start was in room B, the player at the switch, the lamp off (`pictures/s38_next_start.png`).

## 1. What was built

- **The save file** (`crates/gameplay/src/save.rs`, `SaveData`): a RON file with the room file, the place and the turn of the player, the flags, and the variables. `write` puts the text into `<file>.new` and renames it. The game names the file (`save.ron` in the working directory); `[save] file` and `[save] enabled` in the config.
- **`SavePoint`**, an event command of `SaveModule`. It waits for nothing and notes the name of its event. A system after `run_events` writes the file in the first step where that event does not run, with the state of that step. With no save file the command does nothing (a debug line).
- **Load**: `apply` checks the room file of the save, runs `ResetPlay`, sets `GameState`, asks for the room with no spawn point, and puts the player at its pose with no blend. It runs in `Startup` after the start room request and the player, and on the debug key F9 (`LOAD_GAME`).
- **A bad save**: a file that does not read, or a room that does not read: one error line, and no change. At the start that is the start room.
- **Content**: the switch of room B ends with `SavePoint`.
- **Who does not save**: `SaveModule { active: false }` with the editor (`game/src/main.rs`); `save.enabled = false` in each scenario of `tools/bench.py`.

## 2. Numbers

- `cargo test --workspace`: 144 tests, 145 with `--features editor` (1 new). `cargo clippy --workspace --all-targets` clean with and without `--features editor`. `cargo fmt --check` clean.
- New test `a_save_point_writes_at_the_end_of_its_event_and_the_next_start_is_there` (gameplay): an event goes to room B, saves, and sets a variable after the save point. A run with no save file writes nothing. A run with the file writes room B, the pose, and the variable. A second app starts in room B with the flag, the variable, and the pose, with a start room that does not read. A save with a bad room, and a file that is no save: the start room, no flag.
- The test of the game (`the_editor_writes_each_room_file_with_the_same_content`) reads and writes `room_b.ron` with `SavePoint`.
- Debug build with the validation layer: the trip script with the save off (900 frames), a run to the switch with the save on, a start from the save, F9 in the running game: no error. The `script` lines of the trip are the same as in stage 37.
- `bench.py same SCENARIO s37 s38`: `trip_720p`, `walk_720p`, `textured_walk_720p`, `heavy_1080p`, `stress_720p`: `ok`, byte-equal.
- GPU medians, `s37` against `s38`, `gpu_total`: -0.4 % to +0.9 % in all 26 scenarios (`trip_720p` 1.913 ms to 1.931 ms, `heavy_mixed_1080p` 7.127 ms to 7.124 ms).
- No `save.ron` in the repository root or in `target/bench` after the benchmark.

The save file of the gate run:

```text
(
    room: "game/assets/rooms/room_b.ron",
    position: (1.75, 0.0, -1.4999995),
    rotation: (0.0, 0.70710677, 0.0, 0.70710677),
    flags: {
        "power_off": true,
    },
    variables: {},
)
```

(The run of the gate wrote the room with a `\`; the code now writes `/`.)

## 3. Files

- `crates/gameplay/src/save.rs` (new): the module, the command, the file, load, the test.
- `crates/gameplay/src/lib.rs`: the module and its names. `rooms.rs`, `player.rs`: `request_start_room` and `spawn_player` are `pub(crate)`, for the order in `Startup`. `Cargo.toml`: `ron` (a crate of the workspace).
- `game/src/main.rs`: `SaveModule` in the module list and in the list of the test.
- `game/assets/rooms/room_b.ron`: `SavePoint`.
- `tools/bench.py`: `save.enabled = false`. `.gitignore`: `/save.ron`.
- `OPEN_ITEMS.md`, `tools/bench_history.csv`, `tools/bench_env.csv`, `docs/part5/stage-38-save-and-load.md`, `docs/part5/pictures/s38_next_start.png`.

## 4. Deviations

- **`GameState` has no serde derive.** The save has the two maps as fields of its own: the events crate does not change.
- **The pose of the player, not a spawn point.** The plan permits the two ("spawn point or player pose"). A save point is where the player stands.

## 5. Decisions

- **The module is in `gameplay`**: it needs the events (the command), the room, and the player. It is one more module of the list of a game.
- **"The event ends"** is: no running event has the name of the event of the save point. The request has the name, not an index.
- **The game says who does not save** (`active` of the module), not the editor: the editor names no resource of the save module. The command is added in each case, so a tool reads each room file.
- **Load checks the room first.** The room loader changes nothing for a bad file; without the check the game would have the flags of the save in the room before.
- **The room state is not saved.** An `Autorun` page with a flag condition sets it right, as after a room change ("lamp is dead").
- **The music is not saved**: no room of the game has music, and `gameplay` does not depend on `audio`. A module that saves its own state needs a seam (a list of parts of the save file); not built.
- **No version number** in the file (a `ponytail` comment says when).
- **F9** is the debug load key. The two systems with the world (`load_at_start`, `load_on_action`) declare the order that counts and are `ambiguous_with_all`, as `run_ui`.
- **Forward slashes** in the room path of the file.

## 6. Uncertain

- A load in the running game while the picture is faded out, or while a sound of an event plays: `ResetPlay` sets these back in their modules; tried by hand only with F9 in a room at rest.
- The editor was not run in this stage. Its "Add" list gets "SavePoint" by `add_defaults`, and its row by `add_texts`, as the commands of the audio module; play in the editor has `active: false` (the test covers a run with no save file).
- The save file is in the working directory. A game that is installed needs a directory of the user.
- A save from another version of a room: the player can stand in a wall. Not checked.

## 7. What to watch for: results

- **The benchmark**: `save.enabled = false`; no file after the run; `trip_720p` byte-equal.
- **The editor**: `active: false` by the game; the room file with `SavePoint` reads and writes with the same content (test of the game). Not run by hand.
- **The state at the write**: the variable that is set after `SavePoint` is in the file (test).
- **An event that changes the room and saves**: the file names the new room (test).
- **A crash in the write**: the text goes to `save.new`, then a rename.
- **Load in the running game**: F9: `game loaded`, the room loads again (5 meshes kept), "lamp is dead" runs again.
- **The player**: at the saved place, with `Interpolated::snap` (test, and the picture).
- **A flag of a debug panel**: the save is a copy of `GameState`.

## 8. Open items

- "Game state on disk" is closed. New row "Save: what stage 38 left": the music, room state with no flag, one file, no menu, no version.
- "Memory of a waiting command" stays: no save in the middle of an event.
