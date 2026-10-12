# Stage 39: prefabs and the item buffer

State: built. Gate: met in the running editor. A table with two chairs was made a prefab in room A, pasted into room B, changed in room B (one chair 0.8 m up), and room A showed the change (`pictures/s39_prefab_two_rooms.png`, `s39_prefab_changed.png`). The rooms of the game were set back after the session: no room of the game has a prefab.

## 1. What was built

- **The prefab file** (`crates/scene/src/room.rs`): `PrefabDef`, a RON file with `props` and `lights` in the format of a room file, around one origin. `read_prefab`, `prefab_text`.
- **A room names it**: `RoomDef::prefabs`, a list of `PrefabUse` (name, file, pose). `Spawner::prefab` reads the file at each load and spawns each prop and each light at its pose in the room (`pose_in`), with `RoomItem::Prefab(index)`. A direction of a cone light turns with the prefab.
- **A file that does not read**: `read_room` (the game: the start room, a transfer) gives an error with the file and the line, and the room is not loaded. `check_room` (the editor) does not look at the files; the loader then logs the error and shows the room without the prefab.
- **The editor: a prefab is an item** (`Item::Prefab`): the outliner list "Prefabs", the properties (name, file from a list of the `.prefab` files, pose), the gizmo, a click in the picture, the selection boxes, "Add > Prefab file", duplicate, delete, notes.
- **"Make prefab" and "Unpack prefab"** in the "Edit" menu (`Document::make_prefab`, `unpack_prefab`). Make: the selected props and lights go into `prefabs/<name>.prefab` and one prefab takes their place. Unpack: the items go into the room at their places. The editor keeps the file and the pose of the last unpack: "Make prefab" with that name puts the prefab back at that pose, so the items of the file keep their places in each other room.
- **The item buffer**: "Copy" keeps the document and the selected items; "Paste" copies them into the room that is open then (`Document::copy_from`; `duplicate` is the same code with the same room). In the room that they came from, the copies are 0.5 m beside the items; in another room, at the same places.
- **Menus with a text field stay open** on a click inside ("File", "Edit"). Before, a click on the "Save as" field closed the menu: the field took no text by the mouse.

## 2. Numbers

- `cargo test --workspace`: 146 tests, 147 with `--features editor` (2 new). `cargo clippy --workspace --all-targets` clean with and without `--features editor`. `cargo fmt --check` clean.
- New tests: `a_prefab_file_shows_in_each_room_that_names_it_and_a_change_of_it_in_all` (scene: one prefab in room A, two in room B with a quarter turn, a change of the file shows in the two, a bad file stops the room change, a pose is the same after `pose_in` and `pose_out`); `items_go_to_another_room_and_a_prefab_is_made_unpacked_and_changed` (editor: copy in room A, paste in room B with a new light name; make a prefab; the world shows it and a click selects it; turn it, unpack, change, make again: the file has the change in its own space and the prefab has its pose; the saved room reads as the document).
- Debug build with the validation layer, the trip script, 900 frames: no error; the log is the same as in stage 38. The editor session of the gate: no error in the log.
- `bench.py same SCENARIO s38 s39`: `trip_720p`, `walk_720p`, `textured_walk_720p`, `heavy_1080p`, `stress_720p`: `ok`, byte-equal.
- GPU medians, `s38` against `s39`, `gpu_total`: -1.0 % to +0.3 % in all 26 scenarios (`trip_720p` 1.931 ms to 1.920 ms, `heavy_mixed_1080p` 7.124 ms to 7.127 ms).

The prefab file of the session, after the change:

```text
(
    props: [
        (name: "table", mesh: Cuboid((-0.6, 0.0, -0.4), (0.6, 0.75, 0.4)), look: (texture: (path: "textures/wood.png", filter: Nearest))),
        (name: "chair_l", mesh: Cuboid((-0.2, 0.0, -0.2), (0.2, 0.45, 0.2)), pose: (position: (-0.9, 0.0, 0.0)), look: (color: (0.5, 0.3, 0.15))),
        (name: "chair_r", mesh: Cuboid((-0.2, 0.0, -0.2), (0.2, 0.45, 0.2)), pose: (position: (0.9, 0.8, 0.0)), look: (color: (0.5, 0.3, 0.15))),
    ],
)
```

(The file has one value in each line; shortened here.)

## 3. Files

- `crates/scene/src/room.rs`, `lib.rs`: the prefab types, `PoseDef::of`, `pose_in`, `pose_out`, the loader, the check in `read_room`, the test.
- `crates/editor/src/document.rs`: `Item::Prefab`, its place and its notes.
- `crates/editor/src/ops.rs`: `Kind::Prefab`, `copy_from`, `make_prefab`, `unpack_prefab`, `tidy`.
- `crates/editor/src/lib.rs`: `Command::MakePrefab`, `UnpackPrefab`, copy and paste of items, the buffer.
- `crates/editor/src/panels.rs`: the outliner list, the properties, the two menus, "Add > Prefab file". `view.rs`, `overlay.rs`: the pick and the boxes. `widgets.rs`: `PREFABS`.
- `crates/editor/src/tests.rs`: the test.
- `OPEN_ITEMS.md`, `tools/bench_history.csv`, `tools/bench_env.csv`, `docs/part5/stage-39-prefabs-and-item-buffer.md`, `docs/part5/pictures/s39_*.png`.

## 4. Deviations

- **No room of the game has a prefab.** The gate was shown in the editor with rooms A and B and then set back. A prefab in rooms A and B changes the pictures of the trip again, and stage 40 makes these rooms again by hand. The owner decides if the game gets the table.
- **The stage file was written with the stage**, not before it.

## 5. Decisions

- **A prefab has props and lights.** A mover, a door, and an event have names that events use, and a doorway is a part of the room shell.
- **The extension is `.prefab`**, the text is RON: a list of `.ron` files would have the rooms in it.
- **A prefab is changed by unpack, change, make again.** No mode that edits a prefab in place, no second kind of document.
- **The origin of a new prefab** is the floor point of the first selected item, with no turn.
- **"Make prefab" writes the file at once** and replaces a file of that name with no question: this is how a prefab is changed. Undo brings the document back, not the file.
- **The pose of a prefab can have a scale.** An item in it gets the scale for each axis and keeps its shape (no shear): the `ponytail` comment at `pose_in` names the limit.
- **The file is read at each load of the room** (a `ponytail` comment). A drag of a prefab in the editor reads it in each frame of the drag.
- **Poses that the editor writes into a prefab file or back** are at a step of 0.0001.
- **Paste keeps the places** in another room.
- **The buffer is the document of the copy** and the list of the items: no second format for copied items.
- **The game checks the prefab files in `read_room`**, the editor does not: it must open a room whose file is gone.

## 6. Uncertain

- A model file that only a prefab names is read again at each room change and at each reload in the editor (`RoomModels` keeps the models of the props of the room). A `ponytail` comment; not measured.
- A name of an item of a prefab: two prefabs of one file give two lights with one name. `LightOff` then finds the two; the check of the events does not know these names.
- A drag of a prefab in the editor loads the room in each frame (`patch_room` does not know prefabs). Not measured; the session moved no prefab with the gizmo.
- The prefab test of the editor selects by code; in the session the selection was by clicks in the outliner and one click in the picture was not tried on a prefab. The test clicks one.
- "Save as" by the mouse was not tried after the menu fix; the name field of "Make prefab" in the same kind of menu was.

## 7. What to watch for: results

- **The picture and the GPU time**: byte-equal, no step.
- **A turned prefab**: the test turns the prefab by 90 degrees, unpacks, and makes it again: the file has `(1.0, 1.5, 0.0)` and the pose has `(0.0, 90.0, 0.0)`, exact.
- **The other rooms**: in the session, room A showed the raised chair at its place after the change in room B.
- **A file that is replaced**: at once, with no question; in "Decisions".
- **Names**: the pasted lamp is `lamp_2` (test).
- **A prefab file that is gone**: seen in the session (a room of the game with a prefab of a scratch directory): the name is red, the room shows, an error line in the log. The game: the test.
- **Paste into a smaller room**: seen: the table was outside room B until its place was in the room.
- **Text fields in a menu**: a click on the field closed the menu. Fixed for "File" and "Edit".

## 8. Open items

- New row "Prefabs: what stage 39 left".
