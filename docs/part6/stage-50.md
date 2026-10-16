# Stage 50: script components with hooks, commands as functions, "New script", the log panel

State: done. Stage file: [stage-50-scripts.md](stage-50-scripts.md) (written by the session; the owner did not see it). Plan: `README.md`, 2.1, 2.4, 2.7, 2.12 (item 29), 2.14. Tickets: GitHub issues #8 to #11.

## 1. What was built

- **Scripts** (`maki_events::Script`, `crates/events/src/script.rs`): a component of `bevy_ecs` with fields and the hooks `start`, `step`, `interact`, `enter_area`, `leave_area`. `add_script::<T>(app, "name")` registers it. The hooks have `&mut self` and a `ScriptContext` (the world, the entity; pose, step length, input actions, flags and variables, the name, a component of the object).
- **The object entity**: a component type can now insert itself into an entity (`ComponentType::insert`, `add_object_component`). The components of `bevy_ecs` of an object go to one entity: that of its prop with a generated mesh, its light, door, or mover, else a new one, with `Interpolated` at the pose of the object. A script that moves its object moves the mesh; a script on a light changes that light.
- **The area** of an object: the component `area` (`TriggerArea`, width and depth), for `interact`, `enter_area`, `leave_area`, with the reach and the rules of the events (none in the first step, none while an event blocks the player, one press starts an event or a script, not both).
- **Commands as functions** (#9): `context.call(value)` runs a command of the registry with the handler of the rows; `context.command(value)` and `context.run(list)` for a list (a talk, a question). The first step runs at once; a command that waits goes on as a running event with the name of the object.
- **The example script**: `Beacon` (`game/scripts/src/beacon.rs`), an object of room C with an area under the spot light: the light (found by name, `ScriptContext::named_mut`) pulses while the player stands there, and a press of interact counts with `AddVariable` and shows a `Message`. It uses each hook.
- **"New script"** (#10): the command File > New script opens a panel: a name, a template, "Create". The file goes to `scripts/src/<name>.rs` of the project, `lib.rs` gets `mod <name>;` and `<name>::add(app);`, and the file opens in the code editor. Ten templates in `crates/engine/templates/`; a folder `templates/` of the project adds its own or takes the place of one. A test builds and runs each template.
- **The log panel** (#11): a `tracing` layer of the log setup writes each line into `maki_devtools::LOG` (a store of the process, 5000 lines, an equal line after another is one line with a count). The panel "Log" of the editor: a button for each level, a source filter, search, "Clear"; a click on the place of a line opens the file at the line. The store is cleared at "Play".

## 2. Results

| Check | Result |
|---|---|
| Gate: an example script uses each hook, in the game and in the editor | `the_lamp_pulses_while_the_player_is_under_it_and_counts_the_touches` (game/scripts); validation run in room C with the beacon: no error, no warning. In the editor: not run by hand (see 6) |
| Gate: a change of a script loads with no restart | `python tools/reload_test.py`: load 0, a change, build 1.5 s, load 1 with the new text |
| Gate: a script DLL that does not build keeps the values | A component that no module registered stays text and is written back (`a_script_moves_its_object_and_an_object_with_no_code_keeps_its_text`); the editor keeps the old app while a build fails (stage 44) |
| Gate: the game plays as before | Trip run (debug, validation layers): no error, no warning |
| Test: each hook at its time | `each_hook_of_a_script_runs_at_its_time` (events): start one time, step, interact only facing the area, the message of `call`, enter, leave |
| Test: one press, one thing | `one_press_starts_an_event_or_a_script_not_both` |
| Test: the object entity | `the_components_of_bevy_ecs_of_an_object_go_to_one_entity_with_its_pose` (scene) |
| Test: templates and "New script" | `each_template_builds_registers_and_runs` (engine, `tests/templates.rs`); `a_new_script_is_written_and_registered_and_not_twice`; `a_template_of_the_project_is_added_or_takes_the_place_of_the_engines`; `a_name_gives_a_module_and_a_type` |
| Test: the log store | `equal_lines_are_one_with_a_count_and_the_filters_choose_the_lines`, `the_store_keeps_the_last_lines_only` |
| Tests and clippy | Static and dynamic: all pass, clippy clean |
| Symbols of the engine DLL | 58,948 of 65,535 (stage 49: 58,506) |
| Benchmark against `s49` | Run `s50`: pictures of `trip_720p`, `walk_720p`, `test_1080p`, `materials_720p`, `textured_walk_720p` byte-equal to `s49`. GPU total from -16 % to +9 %, most +4 to +8 %, also in scenarios with no script. Back to back with a build of `e58928a` (two rounds, `test_720p`, `materials_720p`, `walk_720p`, `heavy_1080p`): equal within 0.3 %. The step is the machine |

## 3. Deviations from the stage file

- **No system for each script type**: `run_events` calls the runner of each type (`ScriptRunners`) after its own step. A new exclusive system in `FixedUpdate` needs an order against each other system there (bevy refuses an ambiguous order); `run_events` has that order already.
- **The templates are in `crates/engine/templates/`**, not in the editor crate: the command needs the project folder, which the engine has (`reload.rs`), and the test of the templates needs `maki_engine` paths.
- **The log store is `maki_devtools::LOG`**, not `maki_ui::LogLines`: the panel is in `maki-devtools`, and the store is of the process (it stays when the editor makes a new app for new code).
- **`leave_area`** is a hook beside the four of 2.14: the template "Trigger area" (2.7) needs it.
- **The area is `width` and `depth`**, not a size array: the table of fields has no kind for two numbers.
- **Patrol walks to one point and back, Talker splits its text at `|`**: the table of fields has no list kind.

## 4. Decisions where the stage file was silent

- A script value is out of its entity while a hook runs (`std::mem::take`): the trait needs `Default`. A hook that despawns its entity loses the value, with no panic.
- A command list of a script runs with the trigger `Interact`: a message from any hook blocks the player, as an event does.
- The step mode of the editor does not apply to script calls (`ponytail` note).
- `maki-engine` re-exports `tracing`: a script crate names `maki-engine` only (rule 8).
- A new script must not take the name of a registered component type (`area`, `light`): the panel refuses it.
- The script crate of a project is its folder `scripts/` (the layout of stage 43); `lib.rs` must have `pub fn add_modules(app: &mut App) {` and a `mod` line, else the status says what to add by hand.
- The log panel shows each line with a file as a link, also a line of the engine: the owner works on both.
- **The beacon is not on the entity of a light.** The first run of the benchmark showed two causes of a changed picture of `materials_720p`: the benchmark moves the first light of a room (the area of a beacon on it passed over the player), and a component more on a light entity changes the order of the lights for the renderer (a different noise, mean +1.6 %). A script that is on a light changes the picture of a room at rest; the beacon is an object of its own.

Code review (`/code-review` since `e58928a`), fixed: `interact` could fire in the first step; a new script could take the name of a built-in component; the search text was made small once for each line; the list of the script runners was copied in each step; the levels of the log panel were two arrays. "Empty at Play" runs after the commands of the editor and before the loader: the lines of the new room stay.

## 5. Files

| File | Role |
|---|---|
| `crates/events/src/script.rs` (new) | `Script`, `ScriptContext`, `TriggerArea`, `add_script`, the runner of the hooks |
| `crates/events/src/interpreter.rs`, `lib.rs` | `faces` on an area, the free interact press, the runners after the events; exports |
| `crates/events/src/tests.rs` | Tests of the hooks |
| `crates/scene/src/object.rs`, `room.rs`, `lib.rs` | `ComponentType::insert`, `add_object_component`, the object entity; the patch of a scripted prop |
| `game/scripts/src/beacon.rs` (new), `lib.rs`; `game/assets/rooms/room_c.ron` | The example script on the lamp of room C |
| `crates/engine/src/new_script.rs` (new), `templates/*.rs` (new), `tests/templates.rs` (new), `Cargo.toml` | "New script", the ten templates, their test |
| `crates/engine/src/lib.rs`, `editor_program.rs`, `reload.rs` | The log layer, `pub use tracing`, the modules of the editor, "empty at Play" |
| `crates/devtools/src/log_panel.rs` (new), `lib.rs` | The log store and the panel |
| `crates/editor/src/lib.rs` | `run_commands` is public, for an order against it |

## 6. Uncertain

- **Not run by hand in the editor**: the panels "New script" and "Log", and the beacon in a play of the editor. The tests draw neither panel.
- **A player who walks into an area while an event blocks** gets no `enter_area` until the next time, as with the events.
- **The log panel copies the lines** that pass the filter in each frame that it shows (`ponytail` note).
- **The order of the lights for the renderer follows the archetypes of the entities**: a script on a light (the template "Flickering light") changes the noise of the picture of that room, with no change of a value. A stable order (by `RoomItem`) in the extract of the lights would end it; not in this stage (renderer).
- **The engine templates are a list by hand** (`TEMPLATES`): a new template file needs a line there and in its test.
