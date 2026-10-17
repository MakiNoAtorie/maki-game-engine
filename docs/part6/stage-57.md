# Stage 57: data assets

State: done. Stage file: [stage-57-data-assets.md](stage-57-data-assets.md). Tickets #58 to #61 (pull request #57). Plan: `README.md` 2.15.

#58 on the stage branch; then two lanes from it at the same time, #59 (the editor) and #60 (items); #61 on the stage branch (its game side during the lanes, its check after #59). Then one code review (standards and spec) and its fixes.

## 1. What was built

- **Data asset types** (#58): the trait `DataAsset` (`const NAME`), `add_data_type::<T>`, the registry `DataTypes` (not `FieldTypes`: a data type and a component type of one name both stay). A file `<rooms>/data/<name>.<type>.ron` holds the RON of one value. `DataRef<T>` is a `FileRef` whose field kind is `Kind::File(T::NAME)`. The store `DataAssets` reads a file at its first use, forgets it on `AssetChanged`, and tells the watch its files. `ScriptContext::data::<T>(file)`.
- **The editor** (#59): Assets > "New data asset" (a submenu of the types, a name; writes the default value and opens the file) and "Open data asset" (a file of an unknown type is listed, not openable). The inspector shows an open data file from the table of its type until "Close" and writes the file at each change. A `DataRef` field picks the files of its type only.
- **Item kinds and the inventory** (#60): `ItemKind` (name, description, icon), `Inventory` (a count by `FileId`), in the save (`SaveData.items`, `#[serde(default)]`), back to empty on `ResetPlay`. Commands `GiveItem`, `TakeItem` (not below 0); condition `Item(item, Compare, n)`; the check reports a missing item file, a file that is not an item, an item file with no id. `ScriptContext::items()` and `item(file)`.
- **A type of the game** (#61): `BeaconStyle` in the script crate of `game/`; the beacon takes its pulses and strength from an optional `style` file at its start. The type shows in "New data asset" of the editor after the load of the script DLL.
- **Review fixes**: references in `GiveItem`, `TakeItem`, and `Item` get the id of their file (`ids::correct_paths`), so a condition reads no id file at each step and a moved item file keeps its count; the watch is told the data files only when their list changes; an open data file that another program changed is read again, and it is locked while a game runs; the beacon test room by `room_text`.
- **Fix of the run by hand**: the type pick of "New data asset" was a combo box; its popup closed the Assets menu. It is a submenu now.

## 2. Results

| Check | Result |
|---|---|
| Gate: a test type, "New data asset", a change in the inspector, a read by a script, a change on the disk | `crates/assets/src/data.rs` tests, `crates/editor/src/data_panel.rs` tests, `crates/events/src/tests/data.rs` |
| Gate: a type of the script crate in "New data asset" after the load of the DLL | Run by hand: the type submenu lists `beacon_style` and `item` |
| Gate: `GiveItem(key, 1)`, then a door with `Item(key, AtLeast, 1)` opens; over a change of room, a save and a load; an old save | `crates/events/src/tests/items.rs`, `save::tests::the_inventory_goes_into_the_save_and_an_old_save_has_none` |
| Tests of "Done when" | Each line has a test (the files above, `ids.rs`, `beacon.rs`) |
| Tests and clippy | `tools/check.sh` on the merged tree with the fixes: format, comments, clippy and tests static and dynamic, symbol count, reload test: all pass |
| Validation and trip run (debug) | No validation error, no warning or error; 3 room loads and 8 event starts, as before |
| Picture gate (`bench.py gate`) | `trip_720p`, `walk_720p`, `materials_720p`, `textured_walk_720p` equal; `test_1080p` noise (545 pixels at 0.0005, the level of stages 55 and 56) |
| Symbols of the engine DLL | 16,670 of 65,535 (stage 56: 16,629) |
| Benchmark against `s56` | Run `s57`. GPU total -3.8 % to +3.9 %; `test_1080p` +3.9 %, then `bench.py confirm 42e6ecc test_1080p`: +0.2 % in two rounds. The machine |
| Run by hand in the editor | By the input script, on a check room (room D with a `GiveItem` pad and a button with the condition `Item`, removed after): Assets > "New data asset", type `item`, name `key`; the inspector shows `key.item.ron` with name, description, icon; the scan wrote its id and the problems of the events went to 0; the pad shows the row "Give 1 data/key.item.ron"; in play an interact with no key starts nothing, the pad gives the key, the next interact opens the door |

## 3. Deviations from the stage file

- **"New data asset" writes no id file**: the scan of the editor (`sources::scan`) writes the id of each new file (`docs/CONVENTIONS.md`: only the scan writes ids). Until its second look (some seconds) a new item file is a problem of the check.
- **The type pick is a submenu**, not a combo box (see section 1).
- **`ScriptContext::item(file)`**: not in the stage file; a script needs the key of the inventory.
- **`FileId` is `Ord`**: the inventory is a `BTreeMap`, so the save has the same order each time.

## 4. Decisions where the stage file was silent

- A file of a type that no module registered is listed in "Open data asset" and cannot be opened.
- `DataAssets` keys a value by the resolved path; a read of a path as another type is an error, not a second value.
- The id of an item: the id in the reference first, else the id file of the resolved path.
- The beacon reads its style at its start only; a changed style file shows at the next start (a new play or a room load).
- `Facts` holds the inventory and the directory of the room file, for the condition `Item`.
- A negative count in `GiveItem` takes items.

## 5. Uncertain

- A component field with a `DataRef` (the beacon style) has no room check; a bad file logs an error at the start of the script.
- `correct_paths` finds item references by the text `Item(`; a data reference of a script crate under another name gets no id (the `ponytail:` note of `ids.rs`).
- The data panel has no undo, as the routine panel.
- The reload of an open data file after a change on the disk has no test: the run by hand did not change the file from outside.
- `DataTypes::files` and the pick list the data files under `data/`; the pick of the file widget looks one folder deep.
