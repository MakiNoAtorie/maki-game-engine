# Stage 55: object ids, and files with an identity

Plan: [README.md](README.md), 2.15 ("Object id"), 3.2 item 12; [ADR 0001](../adr/0001-object-ids.md); [GLOSSARY.md](../../GLOSSARY.md) ("Object id"). Report: `docs/part6/stage-55.md`. Needs: stage 54 (merged).

Goal: a rename or a move of an object or of a file breaks nothing. Each object of a room has an id that the editor writes; each reference to an object (a command, a wire, the parent, the start spawn) names the id. Each asset file has an id; each reference to a file names its id and its path. This is the key of object state (stage 56) and of the file operations of the asset browser (2.11, part A).
Gate: in the editor, a rename of the door of room A, of the button of room D, and of the spawn point `west` of room B changes no other line of a room file, and the door opens, the wire starts, and the transfer arrives as before. A model file that is moved to another folder (with its id file) still shows in its room, in the game and in the editor. The game plays as before: the trip run passes, `bench.py gate` shows the same pictures, the GPU time against `s54` is the same.

State: written by the session from the plan and the code at commit `1590424`. Not seen by the owner. The form of the file id (a file beside each asset) is a proposal of the session: the plan says "an id for each file", not where it is.

## Now (the code at stage 54)

- `ObjectDef` (`crates/scene/src/object.rs:35`): an optional `name`, a pose, an optional `parent` (a name), the components. The entity of an object has `maki_scene::Name`.
- The commands name objects by their name, as `String`: `OpenDoor`, `CloseDoor`, `LightOn`, `LightOff`, `LightIntensity`, `Signal`, `SetField` (`crates/events/src/builtin.rs:29`), `SoundOn`, `SoundOff` (audio), `GiveControl` (gameplay). `Transfer(room, spawn)` names a room file and a spawn point by name. The systems find the entity by `Name` (`builtin.rs:200`, `:241`, `:467`; `interpreter.rs:397`).
- A rename in the editor rewrites each command that named the old name (`Document::renamed`, `crates/editor/src/ops.rs:137`), one match for each command of each module. A command of a module that this list does not know (of a script crate) keeps the old name, with no error.
- An event is named by the name of its object (`EventDef::name`, `crates/events/src/model.rs:240`): the check of two events with one name (`check.rs:147`), the event of the save point (`crates/gameplay/src/save.rs:172`).
- A change of a prefab instance is keyed by `name/component` of the object of the prefab file, or `index/component` if it has none (`change_key`, `crates/scene/src/prefab.rs:156`): a rename in the prefab file loses the changes of each instance.
- A script finds another object by its name (`ScriptContext::named_mut`, `crates/events/src/script.rs:140`).
- `project.toml` names the start spawn by name (`start_spawn = "start"`); `maki_devtools::stages` reads the spawn names of a room (`spawn_names`).
- A file is a path: from the directory of the room file (`Kind::File` of `maki_core::fields`; `mesh: File("...")`, `floor: (path: "...")`, `PrefabFile::file`, the sounds, `Transfer`'s room, `project.toml`'s `player` and `start_room`). The asset stores load by path and give handles (`crates/assets`).

## The way

### Object ids

- **The id.** `maki_scene::ObjectId(u32)`, `#[serde(transparent)]`: the RON text is a number (`OpenDoor(3)`). Unique in its room. The room file has `next_id`; the editor gives a new object `next_id` and adds one. An id of a deleted object is not given again: its saved state (stage 56) cannot go to a new object.
- **The object.** `ObjectDef` gets `id: Option<ObjectId>`, as the first field. A room file with an object that has no id is valid (the rooms of `bench.py`, of the tests): that object has no id, and no command can name it. The editor gives an id to each object with none when it opens a room; a save writes them.
- **What does not change the id**: a new name, a move, a change of a component, a cut and a paste in the same room, the undo, "unpack prefab". **What gives a new id**: a new object, a copy, a duplicate, a paste into another room.
- **The entity** of an object gets the component `ObjectId` beside `Name`.
- **The parent** is an `ObjectId`.
- **The commands** name objects by `ObjectId`: each command of the list in "Now". `Transfer(room, spawn)` names the spawn by id. The systems find the entity by `ObjectId`. The rename code of the editor (`Document::renamed`, its `rename_in_events`) goes: a rename changes the name only. The editor keeps names unique in a room, as now, so that a pick by name is clear; a name is not a key.
- **The field kind** `Kind::Object` in `maki_core::fields`: a field that names an object of the room. The inspector and the argument column of the event panel show the names in a pick and store the id; an id with no object shows as "missing (id 7)", in red. The pick of objects of the event panel (`Picked::Name`, `crates/editor/src/event_panel.rs:263`) picks ids. A command of a script crate gets the kind from its field type, with no code of the editor.
- **Events** are named by the id of their object: `EventDef` gets the id; the check, the save point, and `start_event` use it. The log and the panels show the name.
- **The objects of a prefab file** have ids of the file (`next_id` in the prefab file). The key of a change of an instance is `id/component` (`change_key`). A command names a room object; on a prefab instance it reaches the object of the instance that has the component, as a name does now.
- **A script** finds an object by id (`ScriptContext::object_mut(ObjectId)`) and by name (`named_mut`, as now). A script that names an object of a room by id reads it from a field of its component (`Kind::Object`), not from a number in its code.
- **The start spawn** of `project.toml` is an id: `start_spawn = 2`, with a comment that names the spawn. `maki_devtools::stages` and `StartRoom` read ids.
- **The migration**, by code, as in stage 48: the rooms A to D, the player prefab, and `project.toml` get ids in the order of their objects; each command, wire, and parent that names an object by name gets its id. The comments of the files stay. The migration code goes in the last ticket, with the name form of the commands. The rooms of the tests in code are changed to ids.

### Files with an identity (item 12)

- **The id of a file**: `maki_assets::FileId`, a random 64-bit number as 16 hexadecimal digits. It is in a file beside the asset, `<file>.id` (`cabinet.glb.id`), as the `.meta` of Unity and the `.uid` of Godot 4. One form for each kind of file: models, pictures, sounds, prefabs, room files, routines and data assets later. A move in Explorer keeps the identity if the `.id` file moves with it (as in Unity).
- **Who writes it**: the editor, for each file of the asset folder that has none, when it opens a project and when the watch (stage 45) sees a new file. A converter of stage 46 keeps the id of its output when it converts again. The game program never writes an id file.
- **A reference to a file**: `maki_assets::FileRef { path, id: Option<FileId> }`. In RON a path only (`"models/cabinet.glb"`) is valid, as now (the generated rooms of `bench.py` and of the tests); the editor writes `(path: "models/cabinet.glb", id: "9f3a2c...")`. Each field of `Kind::File` is a `FileRef`; the inspector shows the path.
- **The resolve**, in `maki_assets`, before a store loads: the path, if its file is there and has the id. Else the first look for an id builds a table of the id files of the asset folder (one walk, then kept; the watch keeps it right in the editor), and the path of the id is used; the log has a warning with the old and the new path. Neither: the error of a missing file, as now, with the id. The stores and the handles do not change: they get a path.
- **The editor corrects the path**: a document whose reference resolves by id gets the new path, and the room is marked as changed. No file operation comes in this stage: the move and the new name in the editor are the asset browser (2.11, part A), which uses this.
- **The migration**: an id file for each asset of `game/assets`, and each reference of the rooms, the prefab, and `project.toml` gets its id.

## Done when

- The gate.
- Tests: an id stays through a rename, a move, an undo, a cut and paste; a copy and a paste into another room get new ids; a deleted id is not given again; a room with no ids loads and gets ids in the editor; a command, a wire, a parent, and a `Transfer` by id after a rename; a field of `Kind::Object` in the inspector and in the argument column; an id with no object shows as missing and the check reports it; a change of a prefab instance stays after a rename in the prefab file; a file that was moved with its id file resolves, in the game and in the editor, and the editor corrects the path; a reference with a path only loads as before; a converter keeps the id of its output.
- Tests and clippy, static and dynamic; a validation run; the trip run; `bench.py gate`; the benchmark against `s54`; the symbol count of the engine DLL (`tools/check.sh`).
- A run by hand in the editor: rename the door of room A, the button of room D, and the spawn `west` of room B, then play the three rooms; move `cabinet.glb` with its id file in Explorer while the editor is open; copy an object with a wire and play it.

## What to watch for

1. **The pictures**: no room of the benchmark changes. The rooms of `bench.py` have no ids and paths only: they must load as before.
2. **The same text**: a room file that the editor reads and saves is the same text (the gate of stage 48), with the ids. The comments of the migrated files stay.
3. **The script DLL**: `ScriptContext` and the field kinds change the API that scripts use. The reload test (`python tools/reload_test.py`) passes; the templates of "New script" build.
4. **The start time**: the walk of the asset folder for the table of ids runs only when a path does not resolve, not at each start.
5. **Two writers of an id file**: the editor writes; the converter thread of stage 46 writes for its outputs. One place writes an id file, with a write to a new file and a rename (as `save.rs:204`).
6. **Git**: the id files are in the repository. A `.glb` with a `.bin` beside it: the id is of the `.glb` only.
7. **Not in this stage**: object state (stage 56), the file operations and the asset browser (2.11), a search by id in the panels.
