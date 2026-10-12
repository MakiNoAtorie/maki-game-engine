# Stage 24: engine/game split

State: built. Gate met: `game/` holds content, its module list, and code of this game only; `ShakeCamera` works with no change outside `game/` and the room file. Plan: [stage-24-engine-game-split.md](stage-24-engine-game-split.md).

## 1. What was built

- Two new crates. `gameplay` (between `events` and `gpu`): camera follow, walking controller, start room and reload, move actions and quit, debug control of a light and of the movers. `devtools` (between `ui` and `game`): benchmark runner with its command line options, room list, panel of the gameplay settings. The renderer panel, the message window, and the game state panel are in `ui`.
- Config: one TOML table for each module. A module declares a settings type (`maki_core::Settings`: table name, defaults, limits) and adds it with `App::add_settings`; the type is a resource. `core` knows only `[time]`. `ToneCurve` is in `render`.
- Input actions: `Action` is an open id (`Action("interact")`). Each module defines its actions as constants and adds their keys with `maki_platform::add_bindings`. `platform` has no action.
- Event commands: an open set. A module writes its commands as one serde enum and adds it, with its handler, to `maki_events::CommandRegistry`. The 14 built-in commands are the enum `Builtin`, added the same way.
- `ShakeCamera(seconds, strength)`: a command of the game (`game/src/shake.rs`), in the event "mirror ball" of room B.

Line count of `game/src`: 1824 before, 341 after.

| File of `game/src` | Lines | Why it is of this game |
|---|---|---|
| `main.rs` | 110 | The module list, the log setup, the config path, the default start room (`game/assets/rooms/room_a.ron`, `start`) |
| `scripts.rs` | 96 | `WALK_SCRIPT`, `TRIP_SCRIPT`, `SHAKE_SCRIPT`: frame numbers that fit the room files |
| `shake.rs` | 135 | The `ShakeCamera` command: enum, handler, the system that moves the camera, one test |

Where each old file went:

| Old file | Now |
|---|---|
| `camera.rs` | `crates/gameplay/src/camera.rs`, `CameraModule`, `[camera]` |
| `player.rs` | `crates/gameplay/src/player.rs`, `PlayerModule`, `[player]`, component `Walker` |
| `light.rs` | `crates/gameplay/src/light_control.rs`, `LightControlModule` |
| `controls.rs` | `crates/gameplay/src/controls.rs`: `ControlsModule` (move actions, quit, action log), `MoverToggleModule` (debug) |
| `rooms.rs` | `crates/gameplay/src/rooms.rs`, `RoomsModule`, `[rooms]` |
| `bench.rs` | `crates/devtools/src/bench.rs`, `BenchModule`, `parse_args`. The scripts: `game/src/scripts.rs` |
| `stages.rs` | `crates/devtools/src/stages.rs`, `StagesModule`, resource `RoomDirs` (the benchmark module adds `target/bench`) |
| `debug.rs` | `crates/ui/src/render_panel.rs`, `RenderPanelModule`. Camera and player sections: `crates/devtools/src/gameplay_panel.rs` |
| `events_ui.rs` | `crates/ui/src/events.rs`, `EventsUiModule` |
| `tick_logger.rs` | Deleted. Its header said "remove it when not needed" |

How `If` is registered: as each other command. It is a variant of `Builtin` with two `Vec<Command>`. Its handler calls `Step::open(list)`: when the command ends, the interpreter runs that list, then the command after the `If`. A command of any module can hold command lists and open one (the test command `Twice` in `crates/events/src/tests.rs`).

## 2. Numbers

Checks: `cargo build`, `cargo test --workspace` (88 tests, 80 before), `cargo clippy --workspace --all-targets`, `cargo fmt --check`: clean. Debug build with the validation layer: 300 frames with the moving light, 900 frames of `--bench-script trip`, 200 frames of `--bench-script shake` with `MAKI_RENDER_SELFTEST=1`: no validation error, no warning, no schedule error.

Pictures, `python tools/bench.py same SCENARIO p4base s24` (the dump of the last frame, byte for byte):

| Result | Scenarios |
|---|---|
| Equal | 19 of 23, with `walk_720p` and `trip_720p` |
| 61 pixels, largest difference 0.00006 | `test_1080p_bounces2`. Two runs of one binary differ this much in some scenes |
| About half of the pixels, same mean (0.09162 and 0.09160) | `heavy_lights8_1080p`, `heavy_mixed_lights8_1080p`, `heavy_mixed_lights8_1080p_bounces2` |

The three scenes with eight lights of a model file: the cause is the order of the lights in the list of the renderer, not this stage's code (section 4, "Order of the lights"). With the same order in both, the baseline and this stage give the same picture, byte for byte.

GPU time, `python tools/bench.py compare p4base s24`, median of `gpu_total` in ms:

| Scenario | p4base | s24 | Change |
|---|---|---|---|
| `test_720p` | 2.609 | 2.623 | +0.5 % |
| `test_1080p` | 4.848 | 5.188 | +7.0 % |
| `test_1080p_bounces2` (the same config as `test_1080p`) | 5.188 | 5.189 | 0.0 % |
| `heavy_mixed_lights8_1080p` | 10.098 | 10.094 | 0.0 % |
| `heavy_lights8_1080p` | 5.816 | 6.192 | +6.5 % |
| `walk_720p` | 2.733 | 2.750 | +0.6 % |
| `trip_720p` | 2.229 | 2.620 | +17.5 % |
| `materials_720p` | 2.410 | 2.410 | 0.0 % |

The changes of `test_1080p`, `heavy_lights8_1080p`, and `trip_720p` are the machine, not the code. `p4base` ran 34 minutes before `s24`; two more runs of `s24` gave the same numbers as the first. What changed on the machine in that time is not known. Proof: the baseline commit, built from an exported copy (`git archive 3d4e09e`), run back to back with this stage:

| Scenario, 610 frames | Baseline binary | Stage 24 binary |
|---|---|---|
| `test_1080p`, three runs each | 5.199, 5.050, 5.194 | 5.198, 5.192, 5.192 |
| `trip_720p`, two runs each | 2.538, 2.578 | 2.577, 2.577 |
| `trip_720p`, by phase (room A, fade, room B, room B dark, room A again) | 2.71, 2.15, 2.08, 2.04, 2.95 | 2.71, 2.22, 2.08, 2.04, 2.95 |

In `p4base`, `test_1080p` and `test_1080p_bounces2` have the same config and differ by 7 %: that is the noise of one run on this machine today. The medians of each pass of `test_1080p` are equal in the two runs (trace 2.106 and 2.116 ms).

## 3. Files

New:
- `crates/gameplay/` (`Cargo.toml`, `lib.rs`, `camera.rs`, `controls.rs`, `light_control.rs`, `player.rs`, `rooms.rs`): the generic gameplay modules, each with its settings and actions.
- `crates/devtools/` (`Cargo.toml`, `lib.rs`, `bench.rs`, `stages.rs`, `gameplay_panel.rs`): tools that a game opts into.
- `crates/events/src/command.rs`: `Command`, `CommandRegistry`, `Step`, and the serde code that finds the enum of a command name.
- `crates/events/src/builtin.rs`: the enum `Builtin` and its handler (the old `match` of the interpreter).
- `crates/render/src/settings.rs`: `RenderSettings`, `ToneCurve`, the limits that were in `App::new`.
- `game/src/scripts.rs`, `game/src/shake.rs`: see section 1.
- `tools/convert_config.py`: the old config format to the new one.

Changed:
- `crates/core/src/config.rs`: `ConfigFile`, the trait `Settings`, `TimeSettings`. `app.rs`: `App::new(ConfigFile)`, `add_settings`, `check_config`, `resource_mut`.
- `crates/platform/src/input.rs`: `Action` as an id, `add_bindings`. `window.rs`: `WindowSettings`.
- `crates/scene/src/room.rs`: a room check is a closure (`RoomValidator`).
- `crates/events/src/interpreter.rs`, `model.rs`, `lib.rs`, `tests.rs`: `run_events` has the full world; the action `INTERACT`; tests of commands of another module and of write-then-read.
- `crates/render/src/lib.rs`, `accumulation.rs`: read `RenderSettings` and `WindowSettings`; the action `RESET_ACCUMULATION`. `light.rs`: the order of the lights.
- `crates/ui/src/lib.rs`, `help.rs`, `render_panel.rs`, `events.rs`: the action `TOGGLE_HELP`; the moved panels.
- `game/src/main.rs`, `game/Cargo.toml`, `game/assets/rooms/room_b.ron` (the event "mirror ball").
- `tools/bench.py`, `tools/shot.py`: config keys are `table.key`.
- `README.md` (config tables, panels, commands, crates), `CLAUDE.md` (crate list, rules 1 and 5), `OPEN_ITEMS.md`, `Cargo.toml`, `.gitignore`.

Pictures: `pictures/stage-24-window.png` (the game with the panel column), `pictures/stage-24-panels.png` (the help panel with the keys of eight modules; the Gameplay panel), `pictures/stage-24-room-b.png` (room B).

## 4. Deviations

- **Order of the lights (a change in the renderer; the stage has renderer changes out of scope).** `extract_lights` now sorts the lights by position. Before, the list had the order of an ECS query: that of the tables, which are made in an order that depends on which systems exist. The shader selects a light by its index, so another order gives another noise pattern for the same scene. This stage added and removed systems, and the three scenes with lights in two tables (a room light and model lights) changed their pictures. A stage that compares pictures cannot have that. 12 lines and one test. Check: the baseline with the same sort and this stage give byte-equal pictures (`heavy_lights8_1080p`, frame 610; `trip_720p`, frame 420, room B with two lights and the player in motion).
- **`core` keeps the text of the config file, not parsed tables.** Each module reads its table from the text, so an error has its line. The check for the old format and for tables with no reader parses the text one more time.
- **`App::resource_mut`.** Before, `App` gave no write access to a resource, and a module added a panel in a `Startup` system. Bindings, room checks, and room directories are tables that many modules add to while the app is built; the order of the bindings is the order of the help panel. The panel registry now uses it too (8 lines less).
- **The camera and player sections of the debug panel are not in `ui`.** They are in `devtools` ("Gameplay" panel): `ui` then has no dependency on `gameplay`, and a game without those modules has no empty sections.
- **New dependency: `erased-serde` 0.4.10.** The stage file does not name it. The registry holds enums of types that `events` does not know; serde's `Serialize` and `Deserialize` cannot be called through a trait object. Nothing in the workspace does that.
- **The message window shows the keys of `INTERACT` from the binding table**, not the fixed text "Z or Enter".
- **`CLAUDE.md`, "Current target".** The work tree had the block already, with the paths of the part 2 and part 3 docs. It was kept in that form.

Not done:
- No menu for the panels ("List of panels" stays open; the column has 7 sections).
- No action of `game/`: the game needs none. The proof that an action needs no edit of `platform` is that `platform` has none: `events`, `gameplay`, `render`, `ui`, and `devtools` each define theirs, and the tests of `platform` define their own.

## 5. Decisions

- **Crates.** `gameplay` between `events` and `gpu`, as the stage file proposes. `devtools` between `ui` and `game`: the runner needs `render` and `ui`; one crate for all tools that a release build leaves out. Rule 1 of `CLAUDE.md` has the new order.
- **Old flat config: a converter, not a second reader.** `tools/convert_config.py` has the table of the 19 keys that moved; the rest goes to `[render]`. The game stops on a flat file and prints the command. The engine has no code for the old format.
- **Strict tables.** A key that the table does not have is an error (`deny_unknown_fields`): `bench.py` cannot set a key that the loader ignores. A table that no module reads is a warning only: a build without a module reads the same file.
- **`[rooms]` has no default in the engine.** The keys are optional; `RoomsModule { start_room, start_spawn }` gets the defaults from the game, and the resource `StartRoom` has the result. The config file that the game writes has an empty `[rooms]`.
- **Table names:** `[time]`, `[window]`, `[render]` (26 keys, one resource: a change of any key starts the picture again, as before), `[camera]`, `[player]`, `[rooms]`. In `[render]` only `render_scale`, `render_width`, `render_height` lost their prefix.
- **Controller.** The component `Walker { radius, direction, speed }` and two systems: `steer_player` writes the walkers of `Player` entities from the move actions; `walk` moves each walker. An NPC module writes `direction` and runs `.before(walk)`. `radius` and `height` are keys of `[player]`; `radius` is read at spawn.
- **`MoveCapture(bool)`** in place of the enum `MoveTarget { Player, Light }`: the controller does not know who took the move actions.
- **The camera keeps its target in the system** (`Local`), not in the `Camera` component: an offset of one frame (the shake) does not become a part of the follow path.
- **`run_events` has the full world.** A handler can write anything. So each system of `FixedUpdate` must declare its order to `run_events`; the schedule stops with the names of the two systems if one does not.
- **A handler is `Fn(&T, &mut Step, &mut World) -> bool`.** `Step::start`, `Step::event`, `Step::memory::<M>()` (the seconds that are left of a `Wait`), `Step::open(list)`.
- **The registry is shared (`Arc<RwLock>`)**: the room check holds a clone and sees commands that later modules add. While a room file is parsed, a thread-local holds the registry: serde's derive cannot hand a value to the `Deserialize` of a nested `Command`.
- **Unknown command at load:** `room_b.ron:3:12: no command `Explode`. The commands are: SetFlag, ...`.
- **`ShakeCamera` is in an event that the trip script does not run** (the mirror ball, 3 m from the switch). `trip_720p` is not changed: its picture is byte-equal to the baseline. The script `shake` runs the event in the real game.
- **Help panel and panel column are in module order** (`main.rs`).

## 6. Uncertain

- GPU time on this machine today: medians moved up to 18 % in half an hour with the same binary (cause not known), and each run has a burst of about 9 times the trace time some 2.6 s after the process starts (in the baseline binary too). A stage that changes GPU work must compare back to back, not against a run of hours before.
- Lights sorted by position: a light that moves changes its place in the list when it passes another light. The noise pattern of that frame is another one; the light is the same. Not looked at in motion with many lights.
- The Debug, Stages, Bench, and Game state panels were not used by hand after the move. Seen: the column with all seven sections, the unfolded Help and Gameplay panels, a click that unfolds a section.
- `Step::memory` is not saved: a save in the middle of an event is not possible yet (open item).
- `Key` is still a closed enum in `platform` (open item for stage 26).

## 7. What to watch for: results

| Check | Result |
|---|---|
| Moved but not generalized | Searched `crates/gameplay`, `crates/devtools`, `crates/ui` for room names, spawn names, script names, sizes: none. `PLAYER_RADIUS` and `PLAYER_HEIGHT` are `[player]` keys. `core` names no room. The start room is named in `game/src/main.rs` only |
| The config split becomes a dynamic bag | No. Six settings types, each a resource. No system reads a key by a string. The debug panels write the typed resources |
| ShakeCamera is the real test | Met. Its files: `game/src/shake.rs`, `game/src/main.rs` (one line), `room_b.ron`. Run in the game (`--bench-script shake`, room B as start room): the event starts, the camera changes in 36 frames (0.6 s), then rests. Test: `an_event_of_a_room_file_shakes_the_camera_and_the_camera_comes_back`. Actions: see "Not done" |
| Benchmark drift | Section 2. Pictures byte-equal but for the light order, which is fixed. `bench.py` sets its keys: a wrong key stops the game (strict tables); `walk_720p` (`camera.follow_time = 0`) and `materials_720p` (`rooms.start`) give the baseline pictures |
| System order | Each order of the moved systems is kept: `follow_player.after(interpolate_transforms)`; `(steer_player, walk).chain().after(run_events)`; `control_lights.after(run_events)`; `toggle_movers.after(run_events).before(move_movers)`; `reload_on_action` and `step_stage` `.before(load_requested_room)`; `move_lights.before(interpolate_transforms)`, with `run_script` `.after(load_requested_room)`; `run_events.before(move_movers)`. One was lost and found by the first run: `allow_ambiguous_resource::<Panels>()`. No lag of one frame: the pictures of `walk_720p` (frame 610) and `trip_720p` (frames 420 and 610) are byte-equal to the baseline |
