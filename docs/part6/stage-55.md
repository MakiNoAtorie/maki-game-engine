# Stage 55: object ids, and files with an identity

State: done. Stage file: [stage-55-ids.md](stage-55-ids.md). Tickets #40 to #47 (pull request #39). Plan: `README.md` 2.15, 3.2 item 12; ADR 0001.

Seven lanes from the stage branch, at most three at a time, each merged when its checks passed: #40 and #41 first, then #42, #44, #46, then #43, #45, and #47 last. Then one code review (standards and spec) and one lane of fixes.

## 1. What was built

- **Object ids** (#40): `maki_scene::ObjectId(u32)`, the first field of an object (`id: 3`), `next_id` in the room file, the component `ObjectId` on the entity beside `Name`. The editor gives ids to a room with none when it opens it, and a new id to a new object, a copy, a duplicate, a paste. A rename, a move, an undo, a cut and paste in the same room, and "unpack prefab" (its first object) keep the id.
- **References by id** (#42, #47): the commands that name an object (`OpenDoor`, `CloseDoor`, `LightOn`, `LightOff`, `LightIntensity`, `Signal`, `SetField`, `SoundOn`, `SoundOff`, `GiveControl`), the spawn of `Transfer`, the parent, and `start_spawn` of `project.toml` are `ObjectId`. #42 read an id or a name (expand); #47 migrated the content and removed the name form (contract). The room check reports an id with no object.
- **The editor** (#43): the field kind `Kind::Object`; the inspector and the argument column show a pick of names and store the id; an id with no object shows "missing (id N)" in red. A rename changes the name only: `Document::renamed` and `rename_in_events` are gone.
- **Events and prefabs** (#44): an event is named by the id of its object (the check of two events, the save point, `start_event`). The objects of a prefab file have ids (`next_id` in the file); the key of an instance change is `id/component`.
- **Scripts** (#45): `ScriptContext::object_mut(ObjectId)` beside `named_mut`. A command enum of a script crate with `#[derive(Fields)]` and `registry.add_fields::<T>()` gets the widgets of its arguments from its field table; an object field gets the pick of names.
- **File ids** (#41, #46): `maki_assets::FileId` in `<file>.id` beside the asset; `FileRef { path, id }`, a bare path in RON when it has no id. The resolve: the path if its file has the id, else the table of id files (built at the first look by id, then kept), else the error of a missing file. The scan thread of the editor writes an id file for each asset with none (on the second scan that sees the file with no id), keeps the id of a converter output, and the editor corrects a moved path as an undo step.
- **Review fixes**: a Cut command (Ctrl+X); `History` keeps the highest `next_id`; the editor adds the id to a bare path whose file has an id file; prefab files get ids when the editor opens them; unnamed objects show as "object N" in the pick; the test rooms of the stage use `room_text`.

## 2. Results

| Check | Result |
|---|---|
| Gate: rename of the door of room A, the button of room D, the spawn `west` of room B | Test `a_rename_changes_only_the_line_of_the_name`: one line changes; `the_button_of_room_d_opens_the_door_by_a_wire` after a rename |
| Gate: `cabinet.glb` moved with its id file | Test `the_cabinet_moved_with_its_id_file_still_shows`: the game resolves the new path; `correct_paths` writes it and marks the room changed |
| Tests of "Done when" | Ids through rename, move, undo, cut and paste; new ids for copy and paste into another room; no reuse of a deleted id (also through undo); a room with no ids; command, wire, parent, `Transfer` by id after a rename; `Kind::Object` in the inspector and the argument column; missing id; prefab change after a rename in the prefab file; a moved file resolves; a path alone loads as before; a converter keeps its id |
| Tests and clippy | `tools/check.sh` on the merged tree (`f4d33f4`): format, comments, clippy and tests static and dynamic, reload test: all pass |
| Validation and trip run (debug) | No validation error, no panic; the events, messages, and room loads of the trip as before |
| Picture gate (`bench.py gate`, on `c3c389e`) | `trip_720p`, `walk_720p`, `materials_720p`, `textured_walk_720p` equal; `test_1080p` noise (414 pixels at 0.0005). The fixes after it change no renderer code and no room file |
| Symbols of the engine DLL | 16,509 of 65,535 (stage 54 after the retro: 16,460) |
| Benchmark against `s54` | Run `s55`. GPU total of each scenario -0.3 % to +3.4 % (`test_2160p` +3.4 %, `heavy_mixed_1080p` +3.3 %, `trip_720p` +2.8 %); no renderer code changed and the pictures are equal: the machine |

## 3. Deviations from the stage file

- **The rooms of `bench.py`** get `id: 1` on their spawn, and the config of a run gets `rooms.start_spawn` from the room text (`bench.start_spawn`): `start_spawn` of the project is an id now (11), which is no spawn of a generated room. "What to watch for" 1 said these rooms have no ids. `tools/shot.py --spawn <name>` turns the name into an id.
- **Paths that stay paths**: `Transfer`'s room, `PlaySound`, `PlayMusic`, and `start_room` of `project.toml` are plain paths, not `FileRef`. The player prefab of `project.toml` is a `FileRef`.
- **The run by hand** was done after the merge, by the input script of the editor (no input of the desktop; room D: `tools/editor_scripts/stage55.txt`):
  - Rename of the door of room A, the spawn `west` of room B, and the button of room D, then Ctrl+S: one line of each room file changes. In play, the renamed button of room D opens the door. A trip run of the game with the renamed rooms: the transfer of room A arrives at the renamed spawn (-2.2, 0, 0), and `OpenDoor(8)` finds the renamed door of room A (no warning). The trip script itself names the spawn `west` by name (`Place::Spawn` of `maki_devtools::script`), so it stops after a rename of that spawn: a limit of the test script, not of the game.
  - Cut and paste of the pad of room D keeps id 5. Copy and paste of the button gives id 6 (`next_id` 7) with its wire `OpenDoor(2)`; after a delete of the original, the copy opens the door in play, and the room check reports `Signal(4, ...)` of the pad (an id with no object, the pad in red).
  - `cabinet.glb` and its id file moved to another folder while the editor has room A open: the cabinet still shows, the tab shows `room_a.ron *`, and a save writes `moved/cabinet.glb` with the same id. The game with the old path loads the cabinet and logs "a file has moved".
  - Found: `quit` of the input script did not end the editor when the room had changes (the path correction had marked it): the editor took the request as its own Quit command and asked a question. Fixed after the stage: `take_quit_request` leaves a request with no `QuitHold` alone (test `a_quit_with_no_hold_ends_the_app_with_changes_not_saved`).

## 4. Decisions where the stage file was silent

- Ids start at 1; 0 is "no object" (`GiveControl(0)` gives the control back to the player).
- A room opened with no ids gets them but is not marked as changed (New and Open would stop on the question of unsaved changes).
- The first object of an unpacked prefab keeps the id of the instance; the others get new ids. The instance that `make_prefab` makes again gets a new id.
- The id table is kept for each folder that a reference resolves from, not for the asset folder as a whole (the same folder for `game/` today).
- The id of a new file is written on the second scan that sees it with no id file, so a move in Explorer whose `.glb` lands before its `.id` does not get a new id.
- `correct_paths` finds the file references of a document in its RON text after `path:`, `file:`, and `File(` (components are text in the document). A field of a script type with another name is not reached (`ponytail:` note).
- Events of objects with no id are told apart by name, as before.

## 5. Uncertain

- `ops::prefab_to_edit` writes the ids into a prefab file with none when the editor opens it to edit an instance; the write drops the comments of that file (`ponytail:` note). The player prefab already has ids.
- `SaveRequest` holds an optional id and a name; `GiveControl` uses 0 for the player. The review called both a primitive in place of a type (`Option<ObjectId>`); not changed.
- The scan thread writes id files every 500 ms while the editor is open: a file that a program writes in two steps can get an id between them.
- `tools/check.sh` does not run the tests of `crates/engine/src/reload.rs` (they need `--features dynamic`), as in stage 54.
