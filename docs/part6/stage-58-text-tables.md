# Stage 58: text in tables

Plan: [README.md](README.md), 2.15 ("Text in tables (item 11)", "Data asset"), 3.2 item 11. Report: `docs/part6/stage-58.md`. Needs: stage 57 (data assets).

Goal: each text that the player reads names a key; the text of the key is in a text table of the language, a data asset. The game shows the text of its language, falls back to a second language, and shows the key itself when no table has it. The editor has a panel of the keys and the languages, and the rows of the events pick a key.
Gate: a test in code: two tables (`en`, `ja`) with the key `door.locked`; a `Message("door.locked")` shows the `en` text, then after a change of the language the `ja` text; a key that only `en` has shows the `en` text in `ja`; a key that no table has shows the key and gives one warning; `{coins}` in a text shows the variable `coins`; a changed table file shows its new text with no restart. The messages of the rooms of `game/` are keys in `data/text/en.text.ron`, and the trip run shows the same messages as before. The game plays as before: `bench.py gate` shows the same pictures, the GPU time against `s57` is the same.

State: written by the session from the plan and the code at commit `f20869a`. Not seen by the owner.

## Now (the code at stage 57)

- The texts that the player reads are in the room file: `Builtin::Message(String)` and `Builtin::Choice(String, Vec<(String, Vec<Command>)>)` (`crates/events/src/builtin.rs`). The rooms of `game/` have 5 messages (`room_a.ron` 2, `room_b.ron` 3) and the beacon script 2 (`game/scripts/src/beacon.rs`, one with a number in it by `format!`).
- `ItemKind.name` and `description` are plain text (stage 57: "stage 58 makes it a key").
- A message goes to `MessageBox(Option<String>)` and the choices to `ChoiceBox` (`crates/events/src/interpreter.rs`); `crates/ui/src/events.rs` draws them. They hold the text, not a key.
- Data assets (stage 57): `DataAsset`, `add_data_type`, `DataTypes::files(directory)` lists the files of a type under `<rooms>/data/`, `DataAssets::get` reads a file and forgets it on `AssetChanged`.
- A field with `#[field(pick = "flags")]` picks from a list of the event panel (`with_picks`, `crates/editor/src/event_panel.rs`), with "New..." for a name that is not there; an unknown list offers nothing.
- The UI uses the egui default fonts: no glyphs of Chinese, Japanese, or Korean.

## The way

- **A text table** is a data asset `TextTable` (`NAME = "text"`): its `language` (`"en"`) and its texts, a list of `(key, text)`. A file for each language, or more than one (by chapter): `data/text/en.text.ron`, `data/text/ja.text.ron`. A list and not a map: the inspector draws a list (`Kind::List`), and the order of the file stays as the writer made it.
- **The texts of the game** are a resource `Texts`: the language (`[text] language`, default `"en"`), the fallback language (`[text] fallback`, default `"en"`), and the merged texts of the tables of those two. It reads the tables through `DataAssets` (so a changed table shows at once) and is made again on `AssetChanged` of a table and on a change of the language. `Texts::get(key)`: the text in the language, else in the fallback, else the key (with one warning for each key). In `crates/events` (the commands, the items, and the scripts read it; the UI of stage 59 too).
- **A key** is a type `TextKey(String)`; in RON a string, as the texts before, so a room file does not change its form. Its field kind is `Kind::Pick { list: "texts", text: true }`.
- **Messages and choices** take a `TextKey`: `Message(TextKey)`, `Choice(TextKey, Vec<(TextKey, Vec<Command>)>)`. The command looks the key up when it runs and puts the text in `MessageBox` and `ChoiceBox`: the UI does not change. A `{name}` in a text is the variable `name` (of the routine call first, as conditions read it, `step.state(world)`); `{{` is a `{`.
- **Items**: `ItemKind.name` and `description` are `TextKey`s.
- **Scripts**: `ScriptContext::text(key)` (with the `{name}` of the variables) and `set_language(language)`.
- **The check**: a key of a row that the fallback language does not have is a warning; a key in two tables of one language is an error; a table with no language is an error.
- **The content of `game/`**: the 5 messages of the rooms and the 2 of the beacon get keys (`room_a.door_locked`, ...) in `data/text/en.text.ron`; the beacon text with a number uses `{beacon_touches}`.
- **The editor**: the key pick of a row lists the keys of the tables with their text in the fallback language, and "New..." adds a key to the first table of that language. A panel "Texts" (by `maki_ui::add_tool_panel`, in the bottom dock): a row for each key, a column for each language, a search field; a cell that a language does not have is marked; "New key" and "New language" (a new table file). A change writes its table file at once, with no undo, as a data asset in the inspector.
- **The language of the editor** is not the language of the game: the UI of the editor stays in English. The panel shows all languages; the rows show the fallback language.

## Tickets

Proposed, not approved.

| Ticket | Blocked by |
|---|---|
| A. Text tables, the texts of the game, the key type | none |
| B. Messages, choices, and items by key; the key pick; the content of `game/` | A |
| C. The panel "Texts" in the editor | A |

B and C can run at the same time after A: B writes `crates/events` (builtin, items, check), `event_panel.rs` (the list `"texts"` of `with_picks`), and `game/`; C writes a new panel file in `crates/editor` and its registration.

## Done when

- The gate.
- Tests: a table reads and writes as a data asset; `Texts::get` in the language, in the fallback, and the key with one warning; a change of the language; a changed table file after `AssetChanged`; `{name}` and `{{`; `Message`, `Choice` by key, and an old room file with a text in place of a key shows that text (the key itself); `ItemKind` by key; `ScriptContext::text` and `set_language`; the check of a key with no text in the fallback, a key in two tables of a language, a table with no language; the key pick lists the keys with their text and "New..." adds one; the panel shows a key in each language, marks a missing cell, and a change writes the table.
- Tests and clippy, static and dynamic; a validation run; the trip run (the same messages as stage 57); `bench.py gate`; the benchmark against `s57`; the symbol count of the engine DLL; the reload test.
- A run by hand in the editor, by the input script: the panel "Texts" with `en` and a new language, a new key from the pick of a `Message` row, play in each language.

## What to watch for

1. **The pictures**: the trip shows messages; their texts must be the same, letter by letter, or the picture of `trip_720p` changes. `bench.py gate`.
2. **Old room files**: a text in place of a key reads as a key and shows itself (no table has it), with a warning: a room of before still plays.
3. **Fonts**: a language with glyphs that the default fonts do not have (Japanese) shows boxes. The fonts of the game are in stage 59 (the UI of the game); the tests of this stage check the text, not the glyphs.
4. **The step**: `Texts` is made again only on a change, never in each frame; a lookup is a map read.
5. **The save**: the language is a setting of the player, not of a save; it is not in `SaveData`.
6. **The script DLL**: `ScriptContext` gets new functions. The reload test passes; the templates of "New script" build.
7. **Not in this stage**: an export and import of the tables for translators (CSV or PO), plural forms, the fonts and the menus of the game (stage 59), a language menu (stage 59: a menu row runs `set_language`), the language of the editor.
