# Stage 39: prefabs and the item buffer

Plan: [README.md](README.md). Report: `docs/part5/stage-39.md`. Item: M12 of [`../part4/MISSING.md`](../part4/MISSING.md).

Goal: items go from one room to another in the editor, and a group of items is one file that many rooms name.
Gate: a table with two chairs is in two rooms; one change of it shows in the two.

Decision of the plan (section 2, M12): a prefab is named by the room, not copied into it.

Written from the code at `07debad`.

## Context in the code

### The room file and the loader

- `RoomDef` (`crates/scene/src/room.rs:46`) has lists of doorways, doors, props, movers, lights, and the spawn points. `PoseDef` is a position, three angles, and a scale; `PoseDef::transform` makes the `Transform`.
- `Spawner::room` spawns each item with a `RoomItem` (the list and the index). `Spawner::prop` takes the pose of the `PropDef`. A mesh has a key from its numbers, not from its pose (stage 36).
- `read_room` checks a file for the game; `check_room` alone is what the editor uses (`Document::parse`): a room with a wrong name must open.
- `patch_room` returns false for each difference that it does not know: the room loads again.

### The editor

- `Item` (`crates/editor/src/document.rs:16`) is what the outliner lists. `Document::place` gives the part that a gizmo changes: a new item with a `PoseDef` moves and turns with no change of the gizmo.
- `Document::duplicate` (`crates/editor/src/ops.rs`) copies an item in its own room, with a name of its own and its notes.
- `Command::Copy` and `Command::Paste` go to the command rows of the event panel (`EventUi::clipboard`); with no click on a row they do nothing (`crates/editor/src/lib.rs:575`).
- `view::pick` and the selection boxes of `overlay.rs` map `RoomItem` to `Item`.
- The notes of a list are by the field name of the list (`Notes`).
- "File > Open" lists the rooms of the game (`EditorModule::rooms`), also when the start room is in another directory.

## Prompt

```text
Stage 39: prefabs and the item buffer.
Read docs/part5/stage-39-prefabs-and-item-buffer.md and item 12 of
docs/part4/MISSING.md first.

The prefab file
- A RON file with props and lights around one origin. A room names it
  with a pose (prefabs: [(file: ..., pose: ...)]). The loader puts
  each item at its pose in the room. A change of the file shows in
  each room that names it, at the next load.
- A prefab file that does not read: the game does not go into the
  room; the editor opens the room and shows it without the prefab.

The editor
- A prefab is an item: the outliner, the properties (name, file,
  pose), the gizmo, a click in the picture, the selection box, "Add".
- "Make prefab": the selected props and lights become a file and one
  prefab in their place. "Unpack prefab": the items of a prefab go
  into the room. Unpack, change, and make again with the same name
  changes the prefab; its pose in the room stays.
- Copy and paste of items: the buffer stays when another room opens.

Out of scope: a prefab in a prefab, movers, doors, and events in a
prefab, a name of a prefab item in an event, a prefab that is edited
in place, a prefab in a room of the game.

Done when: a test loads two rooms that name one prefab file and
changes the file; a test of the editor copies items to another room,
makes a prefab, unpacks it, and makes it again; a table with two
chairs is made, pasted, and changed in the running editor;
trip_720p is byte-equal. Report.
```

## What to watch for

- **The picture and the GPU time.** No room of the game has a prefab: `trip_720p` byte-equal, no step.
- **A turned prefab.** Unpack and make again gives the same file numbers (no 0.99999994) and the same pose in the room.
- **The other rooms.** After "Make prefab" with the name of a file, the items are at the same places in a room that was not open.
- **A file that is replaced.** "Make prefab" writes at once, not with the room; undo does not bring the old file back.
- **Names.** A light that is pasted or unpacked gets a name that the room does not have.
- **A prefab file that is gone.** The editor shows the name in red and the room without the items; the game does not load the room.
- **Paste into a smaller room.** The items keep their places: they can be outside the walls.
- **Text fields in a menu.** The name field of "Make prefab" takes a click and keys.
