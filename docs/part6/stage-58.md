# Stage 58: text in tables

State: done. Stage file: [stage-58-text-tables.md](stage-58-text-tables.md). Tickets #63 to #66 (pull request #62). Plan: `README.md` 2.15, 3.2 item 11.

#63 on the stage branch; then three lanes from it at the same time: #64 (messages by key), #65 (the panel "Texts"), #66 (CSV, added by the owner at the approval). One merge by hand (the `mod` lines of `crates/editor/src/lib.rs`). Then one code review (standards and spec) and its fixes.

## 1. What was built

- **Text tables** (#63): the data asset `TextTable` (`language`, a list of `key` and `text`), files `<rooms>/data/text/<language>.text.ron`. The resource `Texts`: the language and the fallback (`[text] language`, `fallback`, both `"en"`), the merged texts; `get(key)` gives the text of the language, else of the fallback, else the key with one warning. Made again lazily after a room load, a changed table, a change of the language. `TextKey` (a string in RON; field kind `Pick { list: "texts" }`). `fill`: `{name}` is a variable, `{{` is `{`.
- **Messages, choices, items by key** (#64): `Message(TextKey)`, `Choice(TextKey, [(TextKey, commands)])`; the text is looked up when the row runs, with the variables of the step (a routine argument hides a game variable). `ItemKind.name` and `description` are keys. `ScriptContext::text`, `set_language`. The check: a key with no text in the fallback (warning), a key in two tables of a language and a table with no language (errors of the room check). The key pick of a row lists the keys with their text; "New..." adds a key to the first table of the fallback. The 5 messages of the rooms and the texts of the beacon are keys in `game/assets/rooms/data/text/en.text.ron`; the templates of "New script" pass keys.
- **The panel "Texts"** (#65): a tab beside "Events": a row for each key, a column for each language, a search field, "missing" in a cell that a language does not have, "New key", "New language"; a change writes its table at once. Read again only after a change of a file or of the folders.
- **CSV** (#66): Assets > "Export texts" writes `data/text/texts.csv` (a BOM, `\r\n`, RFC 4180 quotes; the fallback first); "Import texts" changes the changed cells, adds new keys and new language tables, changes nothing for an empty cell, removes no key. Own code, no dependency.
- **Review fixes**: one place for the table files and the path of a new table (`table_files`, `table_path`, `TextTable::new`); an empty text keeps the fallback (and counts as no text in the check); a new table file shows in the running game of the editor with no room load (the times of the two folders, with the watch on); the language of the settings again on `ResetPlay`; the CSV skips a row with no key and a table with no language, and does not write over a table of another language.
- **Fix of the run by hand**: the cells of the panel were some letters wide (a grid column takes the width of its first frame); a column is at least 240 points now.

## 2. Results

| Check | Result |
|---|---|
| Gate: `en` and `ja` tables, a change of the language, the fallback, the key itself, `{coins}`, a changed table | `crates/events/src/tests/text.rs`, `tests/messages.rs` |
| Gate: the messages of `game/` are keys and the trip shows the same messages | `the_messages_of_the_game_are_the_texts_of_before`; trip run below |
| Tests of "Done when" | Each line has a test (the files above, `event_panel.rs`, `text_panel.rs`, `text_csv.rs`) |
| Tests and clippy | `tools/check.sh` on the merged tree with the fixes: format, comments, clippy and tests static and dynamic, symbol count, reload test: all pass |
| Validation and trip run (debug) | No validation error, no warning; the messages "Click. The lamp dies. Far away, a lock lets go." and "The lock is dead. The door gives way.", 3 room loads, 8 event starts, as stage 57 |
| Picture gate (`bench.py gate`) | `trip_720p`, `walk_720p`, `materials_720p`, `textured_walk_720p` equal; `test_1080p` noise (278 pixels at 0.0002) |
| Symbols of the engine DLL | 16,719 of 65,535 (stage 57: 16,670) |
| Benchmark against `s57` | Run `s58`. GPU total -3.5 % to +3.8 %; `trip_720p` +3.8 %, then `bench.py confirm` against `main`: +2.1 % and -1.6 % in two rounds. The machine |
| Run by hand in the editor | By the input script, on a check room (room D with a `Message("room_a.door_locked")` on the button, removed after): the panel "Texts" shows the 7 keys in `en`; "New language" `ja` writes `ja.text.ron` and a `ja` column with "missing" cells; a text in a `ja` cell is in the file; Assets > "Export texts" writes the CSV; a text with a comma, quotes, and a line end in the CSV goes into `ja.text.ron` by "Import texts"; play in `en` shows the `en` text, play with `[text] language = "ja"` the `ja` text; in play the panel says that a game runs |

## 3. Deviations from the stage file

- **CSV** (#66): not in the proposal; the owner added it at the approval.
- **The row text** of `Message` and `Choice` shows the key only: the row text has no world to look the text up. The key pick shows the text of the current key under it.
- **The errors of the tables** (a key in two tables, a table with no language) are in the room check only; no row owns them, so the problem list of the event panel does not show them.
- **The beacon function `beacon_forget`** shows a fixed text ("dial turns. ...") in place of the name of its object: a text fills whole-number variables only. Room C only; the trip does not show it.
- **A `TextKey` field outside the event panel** (the inspector of an item kind, a script component) is a typed text: those panels have no key list.

## 4. Decisions where the stage file was silent

- An empty text is a text not yet written: the fallback shows, and the check warns.
- A new table file shows in the running game of the editor by the times of `data/` and `data/text/`; a table in another folder under `data/` shows at the next room load (`ponytail:` note).
- "New..." of the key pick and "New key" of the panel add the key with an empty text.
- The panel shows the languages in the order of the file paths; a key in the order of the first table that has it.
- The CSV has the fallback language first, the others by name; a language column is a file name (letters, digits, `-`, `_`).
- The language of a script goes back to the setting on `ResetPlay`; it is not in the save.

## 5. Uncertain

- The panel and the key pick and the CSV write the same files; the panel compares file times, so a write of another tool in the same time step can be lost.
- The room check stats each table file in each frame of the editor (it reads them only after a change).
- Japanese glyphs: the default fonts have none (stage 59). The run by hand used romaji.
- `tools/comment_lint.py` fails on a file with non-ASCII text on Windows (cp1252): a committed table in Japanese would stop the check. Not fixed here.
- The key pick and "New..." were not clicked by hand; their headless test calls `add_key`.
