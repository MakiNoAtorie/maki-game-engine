# Stage 49: prefabs with changes, and the player as data

State: done. Stage file: [stage-49-prefabs.md](stage-49-prefabs.md) (written by the session; the owner did not see it). Plan: `README.md`, 2.1, 2.14, item 2 of 3.2. Tickets: GitHub issues #6 and #7.

## 1. What was built

- **A prefab file is a list of objects** (`PrefabDef { objects }`, `crates/scene/src/prefab.rs`): the form of a room file, any component. "Make prefab" takes each selected object but an instance.
- **An instance stores only its changes.** The component `prefab` is `file` and `changes`: by `object/component` of the file (the name of the object, or its index), the changed fields as a tree. `instance_objects` gives the objects that an instance shows: each object of the file at the pose of the instance, with the changes merged into its tree through the registry. The loader spawns them with the `RoomItem` of the instance; unpack puts them into the room. A changed prefab file loads the room again (the asset watch has the prefab files of the room).
- **The inspector of an instance**: a group for each object and component of the file, from the merged tree; a changed field has the mark "changed: field" and a button "reset". An edit writes the difference to the file's tree (`difference`): only the fields that differ.
- **The registry keeps a field that is not in the tree** (`#[field(skip)]`): the text of a tree is made over the old value of the component. Before, an edit of the file of an instance in the inspector lost its changes.
- **The player is a prefab file**: `game/assets/rooms/prefabs/player.prefab`, named by the project file (`player = "prefabs/player.prefab"`). Its object has the component `player` (`PlayerDef`: walk speed, run speed, radius; registered by the player module) and `prop` (the mesh: the new `MeshDef::Capsule`, or a model file). The `[player]` table of the config is gone. A change of the file (or of its model) gives the player its new values and mesh.
- **The room loader has no code for the player**: it writes the place of the spawn point into `CurrentRoom::spawn`; the player module (`place_player`) moves the player in the same frame, before the interpolation.

## 2. Results

| Check | Result |
|---|---|
| The gate: a change of the file shows where an instance did not change the value | `an_instance_shows_the_file_at_its_pose_with_its_changes` (scene): the changed intensity stays, the radius follows the new file; a change of a type with no module is left out |
| The gate: the inspector marks a change and resets it | `an_instance_shows_its_prefab_with_its_changes` (editor): the world shows the changed light, a look in the inspector changes nothing; `a_reset_takes_the_value_of_the_file_again` |
| The gate: the game plays as before | Trip run (debug, validation layers): no error, no warning; the player starts at `start`, goes to room B at `west`, room A again. Pictures of `trip_720p`, `walk_720p`, `test_1080p`, `materials_720p` byte-equal to `s48` |
| Test: the file form | `a_prefab_file_is_a_list_of_objects_that_reads_back`; `a_difference_merged_into_its_base_gives_the_new_tree` (also a variant of another name, and a change through RON and back) |
| Test: the player | `the_player_is_made_from_its_prefab_and_follows_its_files` (gameplay): the values and the model of the file; a changed model, then a changed prefab file (new speed, a capsule in place of the model), no mesh left in the store |
| Test: undo | `an_instance_shows_its_prefab_with_its_changes`: a change of the instance is one step of undo |
| Test: no player in the loader | `a_room_change_removes_the_old_room_and_gives_the_spawn_point` (scene), `commands_reach_the_door_...` (events): the loader gives `CurrentRoom::spawn` |
| Tests and clippy | Static: all pass, clippy clean. Dynamic: build and loader test pass |
| Symbols of the engine DLL | 58,506 of 65,535 (stage 48: 57,962) |
| Benchmark against `s48` | Run `s49`: GPU total of each scenario from -14 % to +0.4 %; no step up. `test_1080p` is at 4.72 ms, as in `s47`: the 3 % of stage 48 (its report, 6) was the machine |
| By hand | Not done in the editor (the owner was at the machine): the editor tests draw the inspector of an instance |

## 3. Deviations from the stage file

- None in what was built. The stage file said the asset watch "loads the room again (stage 45)"; the watch did not have the prefab files: it has them now.

## 4. Decisions where the stage file was silent

- **The key of a change**: `name/component`, or `index/component` for an object with no name. A rename of an object in the prefab file drops the changes of it in each instance (they stay in the room file, and do nothing).
- **The merge of trees**: a map whose keys are all in the old map goes field by field; any other value (a number, a variant of another name) takes the place of the old one. `difference` gives the changed fields of a map with the same keys, else the whole new value.
- **Changes are RON maps** (`{"lamp/light": {"intensity": 3.0}}`), the form of `serde_json::Value` written by RON. `maki-scene` names `serde_json` (it is in the tree by `maki-core`).
- **A file name in the player prefab starts at the folder of the rooms**, as in a prefab that a room names.
- **A player prefab that does not read**: an error in the log, and the player is the capsule with the default values. A project with no `player` key: the same with no error.
- **The gameplay panel** changes the `PlayerDef` of the player entity, not a resource: nothing is written to the prefab file.
- **The inspector reads the prefab file in each frame** while an instance is selected (`ponytail` note: a cache by the time of the file if it shows in a profile).

Code review (`/code-review` since `172987a`), fixed: the `[player]` table was still in `README.md`, `OPEN_ITEMS.md`, and `tools/convert_config.py`; the checks of the old player settings were gone (negative speeds, a capsule lower than two radii): `PlayerDef::check`, at each read of the file; the watch of the player kept the model file of the start (it follows the prefab file now); a test of undo; two doc comments on one function.

## 5. Files

| File | Role |
|---|---|
| `crates/scene/src/prefab.rs` (new) | `PrefabDef`, `PrefabFile`, `PrefabUse`, `instance_objects`, `merge`, `difference`, `change_key`; tests |
| `crates/scene/src/object.rs` | The text of a tree over the old value |
| `crates/scene/src/room.rs` | The spawner: one function for an object, also of a prefab; the watch of prefab files; `CurrentRoom::spawn`; `MeshDef::Capsule`; no player |
| `crates/scene/src/lib.rs`, `Cargo.toml` | Exports; `serde_json` |
| `crates/gameplay/src/player.rs` | `PlayerDef`, the player from its prefab file, its reload, `place_player`; test |
| `crates/engine/src/project.rs`, `lib.rs`; `game/project.toml` | The key `player` |
| `game/assets/rooms/prefabs/player.prefab` (new) | The player |
| `crates/editor/src/ops.rs`, `panels.rs`, `lib.rs`, `document.rs` | Make and unpack of any objects; the inspector of an instance; tests |
| `crates/devtools/src/gameplay_panel.rs` | The speeds of the player entity |
| `crates/events/src/tests.rs`, `crates/gameplay/src/save.rs`, `crates/editor/src/tests.rs` | Tests |

## 6. Uncertain

- **The marks of a change are rows under the group**, not on the field: `fields::show` draws a tree with no idea of a change. A mark on the row itself needs a hint for each path in `fields::show`.
- **A prefab in a prefab** is left out with a warning.
- **A prefab file of the old form** (`props`, `lights`) does not read; no file of the game had one.
- **A change key is a name**: an unpack gives each object a name that the room does not have ("lamp" can become "lamp_2"), and "make prefab" again writes that name; the changes of other instances to "lamp" then do nothing. An object with no name has its index, which depends on the order of the selection. A name "1" and the index 1 are the same key.
- **A field that is a map** cannot lose a key by a change (`merge` takes the key of the file again). No component has such a field (`ponytail` note in `prefab.rs`).
- **The mesh of the player has no look** (`prop.look` of the prefab is not used): the capsule of before had none.
