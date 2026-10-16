# Stage 57: data assets

Plan: [README.md](README.md), 2.15 ("Data asset", "Menus"), the slice (item 4: items, an inventory, the use of an item). Report: `docs/part6/stage-57.md`. Needs: stage 56 (merged).

Goal: a module or a script crate registers a type with `#[derive(Fields)]` as a data asset type (as the ScriptableObject of Unity, the Resource of Godot). A file of that type is made, edited in the inspector, read by a script and by an event command, and loads again when it changes. Item kinds are the first data asset type of the engine, with an inventory that events and scripts change.
Gate: a test in code: a type of a test module is registered; "New data asset" writes its file with the default values and an id file; a change in the inspector writes the file; a script reads the value; a change of the file on the disk gives the new value at the next read. A type of the script crate of `game/` shows in "New data asset" of the editor after a load of the script DLL. An item kind `key`: `GiveItem(key, 1)`, then a door event with the condition `Item(key, AtLeast, 1)` opens; the count stays over a change of room and over a save and a load; an old save with no inventory loads. The game plays as before: the trip run passes, `bench.py gate` shows the same pictures, the GPU time against `s56` is the same.

State: written by the session from the plan and the code at commit `42e6ecc`. Approved by the owner (2026-10-16).

## Now (the code at stage 56)

- `#[derive(Fields)]` gives a type its table (`Fields::kind`) and its value as a tree (`Fields::tree`, `serde_json::Value`); `maki_core::fields::read` reads a tree back by `serde` (`crates/core/src/fields.rs`).
- `add_field_type::<T>(app, name)` puts a type in `FieldTypes`: its `Kind` and a `check` (tree in, tree of what was read out). Users: the room shell (`"room"`), the object (`"object"`), each component type, the command enums. The names are one namespace: a data type named as a component would replace it.
- The inspector draws a tree from a table (`crate::fields::show`, `crates/editor/src/panels.rs`) for an `Item` of the room document (`Item::Room`, `Item::Object`) and writes it back by `Document::set_tree`, with undo (`ops.rs`). It shows nothing outside the room document.
- A routine file is the model of a data file (`crates/events/src/routine.rs`): `<rooms>/routines/<name>.routine.ron`, read at the first use into a store (`Routines`), forgotten on `AssetChanged`, its paths given to the watch (`AssetWatch::set("routines", ..)` with a `Check`, a plain `fn(&Path)`). The editor has Assets > "Open routine" and "New routine" (`routine_panel.rs`), which writes the file at each change, with no undo.
- `FileRef` is a path and an optional `FileId` (`crates/assets/src/file_ref.rs`); `resolve(directory)` finds a moved file by its `.id` file. Its field kind is `Kind::File("")`. A field with `#[field(file = "routine")]` gets the files of that group: the groups are a fixed `match` in `crates/editor/src/fields.rs` (`"model"`, `"picture"`, `"sound"`, `"prefab"`, `"routine"`). "New routine" writes no id file.
- `GameState` (flags, variables) and `ObjectStates` go into the save (`SaveData`, `crates/gameplay/src/save.rs`), with `#[serde(default)]` for an old save. No item, no inventory anywhere.
- The script crate of `game/` registers a script component (`beacon`), a command (`shake`), and benchmark scripts through `add_modules`; the editor gets them by a load of the script DLL, which is a new app (`maki_platform::NextApp`).
- `ScriptContext` has `state()`, `routine(file, arguments)`, `switch`, `number`; no read of a file of data.

## The way

- **A data asset type.** A trait `DataAsset: Fields + Serialize + DeserializeOwned + Default + Send + Sync + 'static` with `const NAME: &'static str` (`"item"`). `add_data_type::<T>(app)` puts it in a registry `DataTypes` of its own (the name, the table, a check, and a function that writes a tree as RON text), not in `FieldTypes`: a data type named as a component does not replace it. In `crates/assets` (it has `FileRef` and the watch, and depends on `core` only), so each module above can register one.
- **The file.** `<name>.<type>.ron` (`key.item.ron`), in `<rooms>/data/`, as the routines are in `<rooms>/routines/`. The content is the RON of the type, with no header: the end of the name says the type, as `.routine.ron` does. A file of a type that no module registered stays on the disk and is listed as unknown ("missing script" of 2.14).
- **A reference.** `DataRef<T>`: a `FileRef` with the type. Its field kind is `Kind::File(T::NAME)`; the editor finds the extension of a group that is not a fixed one in `DataTypes`, so a field picks the files of its type only.
- **The store.** `DataAssets`: the files that were read, by path, each as its type (`Box<dyn Any + Send + Sync>`, keyed by path and `TypeId`). `get::<T>(&DataRef<T>, directory) -> Result<Arc<T>, String>` reads at the first use. Forgotten on `AssetChanged`, given to the watch (`AssetWatch::set("data", ..)`, the check `fn(&Path)` of each type, a generic function), as the routines.
- **Scripts and events.** `ScriptContext::data::<T>(file)`. A command or a component of any module holds a `DataRef<T>` and reads through the store. A command with a data reference reports a missing file or a file that does not read as its type in the room check, as `CallRoutine` does.
- **The editor.** Assets > "New data asset": a pick of the registered types, a name; it writes `T::default()` and an id file (`FileId::random`, `write`), and opens the file. Assets > "Open data asset": the files of each type. An open data asset is shown by the inspector from the table of its type, in place of the selection of the room, until "Close"; a change writes the file at once, with no undo (as a routine). The asset browser (editor lane, item 13) lists the same files later by `DataTypes`.
- **A type of the game.** The script crate of `game/` registers a type of its own (`BeaconStyle`: the pulses and the strength of the beacon), and the beacon gets an optional field `style: Option<DataRef<BeaconStyle>>`. No room of the benchmark sets it: the pictures stay. A load of the script DLL adds the type to "New data asset".
- **Item kinds and the inventory.** `ItemKind` (`NAME = "item"`): a name, a description, an icon (`file = "picture"`). The name is plain text now; stage 58 makes it a key of a text table. `Inventory`: a count for each item kind, keyed by its `FileId` (a new name or a move of the file keeps the count); a resource beside `GameState`, in the save (`#[serde(default)]`), back to empty on `ResetPlay`. Commands `GiveItem(item, count)`, `TakeItem(item, count)` (not below 0); condition `Item(item, Compare, i32)`; an item file with no id is a problem of the check. `ScriptContext::items()`. All new commands and conditions are variants with `#[derive(Fields)]` (stage 56): no editor code.

## Tickets

Approved by the owner (2026-10-16). GitHub issues #58 to #61.

| Ticket | Blocked by |
|---|---|
| #58 Data asset types, their files, the store, and the read by a script | none |
| #59 Data assets in the editor: new, open, the inspector, the pick of a reference | #58 |
| #61 A data asset type of the game in its script crate | #59 |
| #60 Item kinds and the inventory | #58 |

#59 and #60 can run at the same time after #58: #59 writes the editor crate, #60 writes `crates/events` and `crates/gameplay`. #61 after #59 (its check is the list of "New data asset" after a load of the DLL).

## Done when

- The gate.
- Tests: a registered type writes its default file and reads it back; a file of a type that no module registered is listed as unknown and kept; a file that does not read as its type gives an error with its line, and the store keeps no value; a changed file gives the new value after `AssetChanged`; a `DataRef` field picks the files of its type only; "New data asset" writes an id file, and a moved data file resolves by its id; a change in the inspector writes the file; `ScriptContext::data` from a script; the `BeaconStyle` of a beacon sets its pulses; `GiveItem`, `TakeItem` (not below 0), the condition `Item`; the inventory over a change of room, a save and a load, an old save; the check reports a missing item file and an item file with no id.
- Tests and clippy, static and dynamic; a validation run; the trip run; `bench.py gate`; the benchmark against `s56`; the symbol count of the engine DLL (`tools/check.sh`); the reload test.
- A run by hand in the editor, by the input script: "New data asset" of an item kind, a change in the inspector, a `GiveItem` row with a pick of the item, a door event with the condition `Item`, play.

## What to watch for

1. **The pictures**: no room of the benchmark gets a data asset or an inventory; the beacon has no `style`. `bench.py gate` shows the same pictures.
2. **The save**: an old save file loads (no inventory); the inventory names an item by its `FileId`, never by its path.
3. **The script DLL**: a type of the script crate is in `DataTypes` of the app of that load; a new load is a new app, so `DataTypes` and `DataAssets` start again from the registrations. A value of the store is never kept over a load (its type can change). The reload test passes; the templates of "New script" build.
4. **Symbols**: each data type instantiates the generic read, check, and write. Count after each ticket (`tools/symbol_count.py`; the stage before: 16,629 of 65,535).
5. **Names**: `DataTypes` is not `FieldTypes`; a data type and a component type of the same name both stay. Two data types of one name: the later replaces the earlier, as `add_field_type` does, with a warning.
6. **No undo**: a data asset in the inspector writes its file at each change, as a routine does. Undo of an asset file comes with the asset browser, if the owner wants it.
7. **Two lanes**: #59 owns `crates/editor`; #60 adds no editor code (its rows and conditions come from their field tables, its item pick from the group `"item"` of #59). The tests of its pick wait for #59.
8. **Not in this stage**: the asset browser (item 13), text tables (stage 58: `ItemKind.name` becomes a key then), menus as data assets (stage 59: `Menu` is a data asset type there), the values of enemies, the use of an item from a menu (stage 59).
