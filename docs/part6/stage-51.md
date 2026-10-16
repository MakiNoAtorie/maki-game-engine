# Stage 51: the events as components, and the general commands

State: done. Stage file: [stage-51-events-as-components.md](stage-51-events-as-components.md) (written by the session; the owner did not see it). Plan: `README.md`, 2.1, 2.8, 2.14. Tickets: GitHub issues #12 and #13.

## 1. What was built

- **An event is a component of an object** (#12): the component `event` (`maki_events::EventPages`) holds the pages. The name of the event is the name of the object; its area is the component `area` of the object (stage 50) at the pose of the object: one kind of area for events and scripts. The list `events` of a room file is gone (`RoomDef` refuses it).
- **The interpreter on the entities**: in each step `run_events` takes the entities with `EventPages`, in the order of the objects in the file (`RoomItem`), with the area at the current pose. A script that moves its object moves the area of its event. What the runner kept of each event is a component (`EventMemory`); a running event knows its entity.
- **The reads**: the type `event` reads its text with the command registry when the loader inserts it (a queued command with the world). `CommandRegistry::parse_events` reads the events from the objects of a room file, with the line of each command for the checks, as before.
- **One store**: the flags and variables were one resource already (`GameState`); a test pins that a script write starts a page.
- **The rooms**: each event of rooms A and B is an object of its own, with the name of the event, the center of its area as its position, its size as `area`.
- **The editor on objects only**: `Item::Event` and the section "Events" of the outliner are gone. An object with `event` shows the event panel; the overlay, the pick, and the corner handles work on the area of any object (events and scripts). "Add > Event area" adds an object with `area` and `event`.
- **The general commands** (#13):
  - `SetField(object, component, field, value)` changes a field of a component of an object. A component type of `bevy_ecs` has a live form now (`maki_scene::Live`, from `add_object_component`: its value in the entity as a tree, and a tree written back).
  - `CallScript(name)` calls a function of the script crate (`add_script_function`) with a `ScriptContext` of the object of the event.
  - The editor: pickers of the object, the component (those with a live form), the field (from the table of fields), and the widget of the field for the value; a pick of the function. Undo and the problems of the row as for each command.
- **The example**: room C has a dial: a row sets `pulses` of the beacon, a row calls `beacon_forget` of `game/scripts/src/beacon.rs`.

## 2. Results

| Check | Result |
|---|---|
| Gate: each event of the rooms works as before | Trip run (debug, validation layers): each event of rooms A and B starts (`to room b`, `switch`, `lamp is dead`, `to room a`, `flicker`, `locked door`); no error, no warning |
| Gate: the same pictures as `s50` | `trip_720p`, `walk_720p`, `test_1080p`, `materials_720p` (room C with the dial): byte-equal (`bench.py same`) |
| Gate: the general rows in the game and the editor | Tests (below); not run by hand (see 6) |
| Test: the events from the objects, with lines | `events_are_read_from_the_objects_of_a_room_file` (events) |
| Test: one area, it moves with the object | `an_event_and_a_script_of_one_object_share_its_area_and_it_moves_with_the_object` |
| Test: one store | `a_script_and_a_row_use_one_store_of_variables`; the save writes `GameState` (save test of stage 38) |
| Test: the migrated rooms | `the_editor_writes_each_room_file_with_the_same_content` (game): 8 events, 10 pages (7 and 9 before, and the dial) |
| Test: Set a field, Call a script function, their checks | `a_row_sets_a_field_of_a_component_of_an_object`, `a_row_calls_a_function_of_the_script_crate_with_the_object_of_the_event`, `the_check_finds_a_wrong_object_component_or_function_of_a_general_command`, `a_row_calls_the_function_that_the_beacon_adds` (game) |
| Test: the editor | `a_general_command_row_has_undo_and_shows_its_problems`, `the_set_field_pickers_offer_the_live_components_and_their_fields`, `a_rename_changes_the_events_and_a_name_stays_unique` (with a `SetField` row), the editor tests of before on objects |
| Tests and clippy | Static and dynamic (`tools/check.sh`): all pass, clippy clean |
| Reload test | `python tools/reload_test.py`: load 0, build 1.7 s, load 1 with the new text |
| Symbols of the engine DLL | 60,011 of 65,535 (stage 50: 58,948) |
| Benchmark against `s50` | Run `s51`: pictures of `trip_720p`, `walk_720p`, `test_1080p`, `materials_720p`, `textured_walk_720p` byte-equal to `s50`. GPU total within 2 % but `trip_720p` +4.1 %; back to back with a build of `53814fc` (two rounds): -0.3 % and +0.5 %. The step is the machine |

## 3. Deviations from the stage file

- **"Set a field" does not change a light.** A light of the file (`LightDef`) is made into another component (`Light`, with the pose apart): the type `light` has no live form, the picker does not offer it, and the game logs a warning. A live form for the light needs a map from `LightDef` to `Light`; not in this stage. The test changes a script and the area.
- **The check of `SetField` does not check the field.** The room check has no table of fields (a resource of the app; the check runs with the registry only). A wrong field is a warning in the game; the picker of the editor offers only fields of the table.
- **The interpreter tests write events in the short form** (`maki_events::short_events`): it makes the objects. The line numbers of their problems are those of the objects.
- **"Test this page"** starts the page before the first step of the room: its own `Autorun` waits (an event blocks the player). Before, the page ran two times.

## 4. Decisions where the stage file was silent

- An event on an object with no name has the name "" (no check). A `Parallel` event is found by its entity, not its name.
- `Problem` has the object of the event (`object`) beside the index in the list.
- `CommandRegistry` holds the script functions too: the check (`CheckContext::functions`) and the row find them with the registry, also in the check of the loader.
- A drag of an area corner moves the object (the center of the area is the position of the object), with its mesh if it has one.
- The value of `SetField` is in the form of the tree of the fields (`serde_json::Value`): a number, `true`, a text, a list. A write only when the value is new.

Code review (`/code-review` since `53814fc`), fixed: a rename did not follow into `SetField` rows; a shown `SetField` row could change with no edit (a dirty document); the two fields of `CommandRegistry` had no names. Not fixed (judgement calls): the three texts of `SetField` as one type, a helper for an object by name, long doc comments.

## 5. Files

| File | Role |
|---|---|
| `crates/events/src/model.rs` | `EventPages`, `EventDef` with its object, the read of the objects of a room file, `short_events` |
| `crates/events/src/interpreter.rs` | The interpreter on the entities, `EventMemory`, `start_event`, the read of `event` with the registry |
| `crates/events/src/command.rs` | `parse_pages`, the script functions of the registry, `Step::entity` |
| `crates/events/src/builtin.rs` | `SetField`, `CallScript`: run, text, help, check |
| `crates/events/src/check.rs` | `Problem::object`, `CheckContext::functions` |
| `crates/events/src/script.rs`, `lib.rs` | `add_script_function`; the type `event`; exports |
| `crates/events/src/tests.rs` | Tests of events on objects, the shared area, the store, the general commands |
| `crates/scene/src/object.rs`, `room.rs`, `lib.rs` | `Live`, `add_component_reader`; `RoomDef` with no `events` |
| `crates/editor/src/document.rs`, `ops.rs` | The event of an object (`event`, `set_event`, `events`, `area`, `set_area`), no `Item::Event` |
| `crates/editor/src/event_panel.rs`, `panels.rs`, `lib.rs` | The event panel on the selected object; the pickers of the general commands |
| `crates/editor/src/gizmo.rs`, `overlay.rs`, `view.rs` | The area of any object: handles, drawing, pick |
| `crates/editor/src/events.rs`, `tests.rs` | The names and the problems from the objects; tests |
| `crates/devtools/src/script.rs`, `bench.rs` | A bench script walks to the object of an event |
| `game/assets/rooms/room_a.ron`, `room_b.ron`, `room_c.ron` | The migrated events; the dial |
| `game/scripts/src/beacon.rs`, `lib.rs`, `shake.rs` | `beacon_forget`; tests |
| `crates/audio/src/tests.rs`, `crates/gameplay/src/save.rs`, `crates/engine/tests/templates.rs`, `programs/editor/...` | Test rooms on objects |

## 6. Uncertain

- **Not run by hand in the editor or the game**: the event panel on an object, the pickers of `SetField`, the dial of room C. The tests do not draw the pickers.
- **A component of a prefab instance**: the check of `SetField` reads the components of the object in the room only, not those of its prefab file.
- **The symbol count** grows: 60,011 of 65,535.
