# Stage 48: the room file is a list of objects

State: done. Stage file: [stage-48-objects.md](stage-48-objects.md) (written by the session from 2.1 and 2.14; the owner did not see it). Plan: `README.md`, 2.1, 2.14. Tickets: GitHub issues #1 to #5.

## 1. What was built

- **A room file is a list of objects.** An object is a name, a pose, an optional parent, and components; a component is the name of its type and its value as RON text (`ComponentText`, `crates/scene/src/object.rs`). The lists `doorways`, `doors`, `props`, `movers`, `lights`, `prefabs`, and `spawns` are gone from `RoomDef`, from the loader, and from the editor.
- **A component with no code keeps its text.** A room reads with no type for any component; a component that no module registered is written back as it was read, and the inspector shows its text. The text is made compact when it is read (no comments, no white space but in strings).
- **The registry of component types** (`maki_scene::add_component_type::<T>(app, name)`): the table of fields (`FieldTypes` of stage 47) and the way from the text to the tree and back. The scene module registers seven types: `prop` (`Prop`), `light` (`LightSource`), `doorway` (`Doorway`), `door` (`DoorLeaf`), `mover` (`MoverPath`), `prefab` (`PrefabFile`), `spawn` (`SpawnPoint`).
- **The loader spawns each object** from its components, each entity marked `RoomItem(index)` (the index of the object). The events and the audio checks find a light, a door, a mover, and a spawn point among the objects (`RoomDef::each_light`, `spawn`, `spawn_names`).
- **The editor has three item kinds**: the room, an object, an event. The outliner lists "Objects" and "Events"; pick, highlight, the gizmo (with the aim handle of a cone light, the corners of a doorway, the second pose of a mover), add, copy, delete, rename, make and unpack a prefab work on objects. The inspector shows the rows of the object (name, pose, parent) from the table of `ObjectDef`, then a group for each component from its registered table. The rows by hand of a spawn point are gone.
- **The three rooms of the game were migrated by code**, one kind of item for each ticket, with their comments; the migration code went with the old lists (#5).
- `[window] focus` (new): `false` opens the window behind the other windows and does not take the keyboard focus, for runs while the owner works (2026-10-15).

## 2. Results

| Check | Result |
|---|---|
| The gate: the rooms are lists of objects | `room_a`, `room_b`, `room_c`: the shell fields, `objects`, `events`. Migrated by `Document::items_to_objects` (#1 to #4), then deleted (#5) |
| The gate: outliner, pick, gizmo, inspector on any object | Editor tests: a click selects the cube object, the spawn point, the lamp (a sphere of light), a model of a file, a prefab; a gizmo drag of an object is one undo step; the aim handle points a cone light; a doorway's corners change it; a mover moves with its two poses |
| The gate: the same file | `the_editor_writes_each_room_file_with_the_same_content`: each room of the game read and written is the same text (line ends aside: git can give a file CRLF), and the room, each object, and each component go to the tree and back with no change |
| The gate: the same pictures | `bench.py same` of `trip_720p`, `walk_720p`, `materials_720p` against `s47` after #2, #3, #4: byte-equal. After #5: `test_1080p`, `trip_720p`, `walk_720p` byte-equal too |
| Test: a component with no type | `an_object_keeps_each_component_as_its_text` (scene), `an_object_shows_each_component_and_keeps_one_with_no_type` (editor): its text stays through a load and a save; the inspector shows it and does not change it |
| Test: each component type | Its zero tree reads as the type; a value goes to its tree and back (`each_item_kind_goes_to_its_tree_and_back`) |
| Validation run (debug, `--bench-script trip`) | No validation error, no warning. Room A, B, A; the switch, the lamp that dies, the door that opens |
| Reload test (`tools/reload_test.py`) | Passes |
| Tests and clippy | Static: all pass, clippy clean. Dynamic: build and loader test pass |
| Symbols of the engine DLL | 57,962 of 65,535 (stage 47: 57,783). Highest in the stage: 59,161 (#4, with the old and the new types) |
| Benchmark against `s47` | Run `s48`: GPU total 0 to 23 % above `s47` in each scenario. The same rise with the commit before the stage (`526d6da`) built in the scratchpad and run back to back with this stage, two rounds of six scenarios: `test_2160p`, `walk_720p`, `trip_720p`, `heavy_mixed_1080p`, `textured_walk_720p` within 1 % of each other. The machine, not the code (see 6 for `test_1080p`) |
| By hand, in the editor | The editor opens room A: the outliner lists "Objects (11)" and "Events (3)", the view shows the room, the swing of the door, the spawn points. [Picture](pictures/s48_editor.png). Clicks, drags, undo, and save by hand were not done (the owner was at the machine): the editor tests do each of them with no window |

## 3. Deviations from the stage file

- **A door is a component of the object of its doorway**, not a door that names its doorway object (#3). A door is always in one doorway, and a doorway has at most one door: one object holds the two, with no name or index to keep right, and a deleted doorway takes its door with no code. The stage file was changed to say so.
- **"Add" (#1) and "make and unpack a prefab" (#2) moved to objects before #4**: once a kind was migrated, a new item of that kind must not go into the old list.
- **`RoomItem` is the index of the object** (`RoomItem(usize)`), not an enum with one variant.
- **A prefab file keeps its lists** (`props`, `lights` of `PrefabDef`): prefab files as objects are stage 49 (#6). `PropDef`, `LightDef`, `MoverDef`, `PrefabUse` stay as the typed view of a component with its object (name, pose); they lost their table of fields.
- **The events stay a list of the room**: events as components are stage 51 (#12).

## 4. Decisions where the stage file was silent

- **The names of component types**: `prop`, `light`, `doorway`, `door`, `mover`, `prefab`, `spawn`; the Rust types `Prop`, `LightSource`, `Doorway`, `DoorLeaf`, `MoverPath`, `PrefabFile`, `SpawnPoint` (the names `Light`, `Door`, `Mover` are the components of `bevy_ecs`).
- **The place of a light is the position of its object**; the direction of a cone stays a field of the light, not the turn of the object. A turn of a group that has a light writes a rotation to the light's object that the light does not use.
- **The second pose of a mover (`b`) stays in the space of the room.** Pose `a` is the pose of the object. The editor changes `b` through `Document::with_mover_b`.
- **A spawn point is an object with an empty component** (`"spawn": ()`); its name is the name of the object. "The first spawn point" is the first by name, as with the old map.
- **One group of names for all objects**: a light and a door cannot have the same name. A rename of an object corrects each command that names it (light, door, sound), and a `Transfer` of the room to itself for a spawn point.
- **The outliner label of an object with no name**: the name of its first component and the index.
- **`patch_room`** (a change that needs no load) copies the pose of an object but of a file model or a prefab, the values of a light but its sound, and `b` and the time of a mover. Its loop of mesh poses skips doors and movers (they have their own poses) and the meshes of files and prefabs.
- **The order of the migrated objects**: doorways with their doors, movers, props, lights, prefab uses, spawn points; the notes of each item went to its object.

Bugs found and fixed in the stage: `patch_room` set the pose of the object on the leaf of a door and on each mesh of a prefab (after #3 and #4: a slider in the editor moved them); a copy of a light object moved its light two times; the test of the same file failed on a checkout with CRLF line ends.

## 5. Files

| File | Role |
|---|---|
| `crates/scene/src/object.rs` (new) | `ObjectDef`, `ComponentText` (the compact text), the registry `ComponentTypes`, `add_component_type`; tests |
| `crates/scene/src/room.rs` | `RoomDef` with `objects` only; the component types and the views of an object; `RoomItem(usize)`; the loader, `check_room`, `patch_room` on objects; tests |
| `crates/scene/src/lib.rs` | The names of the component types; the exports |
| `crates/events/src/builtin.rs`, `check.rs` | Lights, doors, movers, spawn points among the objects |
| `crates/audio/src/commands.rs` | The lights with a sound among the objects |
| `crates/devtools/src/stages.rs`, `script.rs` | Spawn points of the objects |
| `crates/platform/src/window.rs` | `[window] focus` |
| `crates/editor/src/document.rs` | `Item` (room, object, event); the views of an object (`light`, `doorway`, `mover`, `prefab_use`, `with_mover_b`); `component_tree`, `set_component_tree` |
| `crates/editor/src/ops.rs` | Add, delete, copy, rename, make and unpack a prefab on objects |
| `crates/editor/src/panels.rs` | The outliner sections; the group of each component in the inspector |
| `crates/editor/src/gizmo.rs`, `overlay.rs`, `view.rs`, `lib.rs`, `event_panel.rs`, `events.rs` | Gizmo, marks, pick, rename, the name lists of the rows, on objects |
| `crates/editor/src/tests.rs`, tests in each crate above, `game/scripts/src/lib.rs`, `shake.rs` | Tests; the room texts of the tests as objects |
| `game/assets/rooms/room_a.ron`, `room_b.ron`, `room_c.ron` | Migrated |
| `tools/bench.py` | The generated rooms as objects |
| `docs/part6/stage-48-objects.md`, `docs/CONVENTIONS.md` | The door decision; the test command, parallel branches |

## 6. Uncertain

- **An object with two components that each make an entity** (a prop and a light): two entities with one `RoomItem`. It works for the loader and the pick; a light of a model file in an object that also has a `light` would take that light's values in a patch. No room has that.
- **Raw strings** (`r#"..."#`) in a component text are not known to the compactor: a `"` in one ends the string there (`ponytail` note in `object.rs`).
- **The symbol limit**: 7,573 are left. Stages 49 and 50 add the prefab instance and the script types.
- **The parent of an object** is written and read, and nothing uses it yet (stage 49).
- **`test_1080p`: GPU total 0.14 ms (3 %) above the commit before**, in two rounds back to back (4.96 and 4.96 ms before, 5.10 and 5.12 ms now). Each pass has the same time within 0.01 ms and the picture is byte-equal: the difference is between the passes, not in their work. Not explained; the other five scenarios show none.
