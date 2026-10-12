# UX pass of the editor

State: commit `baf8b7a` (stage 28). Method: a read of `crates/editor/src` and `crates/ui/src`, and the pictures of stages 26 to 28. The editor was not run for this pass: stage 32 starts with a hand session that confirms each finding.

Each finding has the stage that fixes it ([README.md](README.md)).

## Traps: a key or a click does something that the user did not ask for

| # | Finding | Where | Fix | Stage |
|---|---|---|---|---|
| U1 | The Delete key deletes the event while a command row is selected. The key knows the item selection only; the row is in `EventUi::row`. | `Editor::keys` (`lib.rs:648`), `Command::Delete` (`lib.rs:432`) | The editor knows which list had the last click (items or command rows). Delete, copy, paste, and duplicate go to that list. | 32 |
| U2 | "Save as" with the name of a room that exists writes over it, with no question. | `Command::SaveAs` (`lib.rs:403`), `file_menu` (`panels.rs:212`) | The question of `CONFIRM`, with "Replace". | 32 |
| U3 | Play asks "Save / Do not save / Cancel" each time the room has a change. The game plays the text of the document, not the file. | `asks` (`lib.rs:378`), `play` (`lib.rs:544`) | No question for `Play` and `Test`. The reason for it today: a `Transfer` away and back reads the file. So the loader gets the text of the open document for its path while the editor plays. | 32 |
| U4 | A name typed in "new:" with one letter wrong is a new flag. No mark shows. | `pick_or_new` (`event_panel.rs:115`) | The mark "no write" of M1, and a picker where "New..." is an entry of the list and not a field that is always there. | 29, 31 |
| U5 | The question for changes that are not saved is a window at the bottom of the picture. The room can be changed while it waits. | `add_window(CONFIRM)` (`panels.rs:48`), `Place::Window` (`ui/lib.rs:373`) | A modal question in the middle of the window. Enter is the first answer, Esc is "Cancel". | 32 |

## Layout

| # | Finding | Where | Fix | Stage |
|---|---|---|---|---|
| U6 | The window is 240 + 1280 + 440 = 1960 pixels. The two widths are constants and the panels are `resizable(false)`. | `ui/lib.rs:81-83`, `317`, `331` | M3. | 31 |
| U7 | The right column lists six debug panels (Help, Stats, Debug, Gameplay, Game state, Bench) before "Properties". The properties are one fold of a scroll column. | `run_ui` (`ui/lib.rs:326-350`), module order in `game/src/main.rs` | In the editor: the properties are a panel of their own. The debug panels are one window, closed at the start, opened from the menu. | 31 |
| U8 | The arguments of a command are below the full command list. With a long list they are off the screen when a row is selected. A row is one line of RON text (a `Message` with its full text) in a column of 440 pixels. | `commands` (`event_panel.rs:623`) | The event panel under the picture, in three parts (README, section 2). | 31 |
| U9 | During play the left panel shows the tools and the outliner, disabled, and the property panel shows one line. | `outliner` (`panels.rs:292`), `properties` (`panels.rs:424`), `stage-28-step.png` | During play: the running events and the game state take the place of the tools and the properties. | 31 |
| U10 | The window title is the title of the config. It does not show the room file or the mark of a change. | `window.rs:197` | Title: `room_a.ron * - maki editor`. A seam in `platform` to set the title. | 31 |
| U11 | The status line is one weak grey line. "Added" and a save that failed look the same, and the next command replaces the error. | `panels.rs:279`, `Editor::fail` (`lib.rs:507`) | A status bar at the bottom of the window. An error is red and stays until the user closes it or the same command works. | 31 |

## Picture

| # | Finding | Where | Fix | Stage |
|---|---|---|---|---|
| U12 | No key puts the selection in the middle of the view. A double click in the outliner does nothing. | `camera.rs:25-32` | `F` frames the selection: target at its center, distance from its box. A double click on an outliner row does the same. | 32 |
| U13 | A handle or an item under the pointer looks the same as the others. The user learns what a click takes only from the click. | `overlay.rs` (no hover state), `viewport` (`lib.rs:345-371`) | The handle under the pointer is brighter. The item under the pointer has a thin box. The pick runs for the pointer place in each frame: measure its cost in a room with a model file. | 32 |
| U14 | Esc does nothing while the room is edited. A drag of a handle ends only with the release, at the new value. | `viewport` (`lib.rs:332-343`) | Esc in a drag puts the values of the drag start back. Esc with no drag clears the selection. | 32 |
| U15 | F5 plays. No key of the editor stops: the quit key of the game does. | `BINDINGS` (`lib.rs:101`), `panels.rs:251` | F5 plays and stops (a binding in the game mode that the editor adds). The quit key stays. | 32 |
| U16 | Undo of a delete puts the item back, not selected. | `run_commands` (`lib.rs:839`) | A history step holds the selection. | 32 |

## Outliner

| # | Finding | Where | Fix | Stage |
|---|---|---|---|---|
| U17 | No menu on a row, no rename in place, no filter. An item with no name is "cube 0", by its index: the label changes when an item before it is deleted. | `sections` (`panels.rs:338-394`), `rows` (`panels.rs:317`) | A context menu (Rename, Duplicate, Delete, Frame). F2 or a double click on the text renames in the row. A filter field above the list. "Add" gives each new item a name that the room does not have. | 32 |
| U18 | An event with a problem has " (!)" in the color of the others, with no hover text. The count of problems shows only in the status of a save. | `panels.rs:377`, `Editor::save` (`lib.rs:498`) | The mark is red, with the problem as hover text. The status bar has the count; a click opens a list; a click on a row goes to the event, page, and command. | 32 |
| U19 | "Where used" lists each place as text. A click does nothing. | `where_used` (`panels.rs:413`) | A click on a place of the open room selects the event, page, and row. A place of another room names the file. | 32 |

## Properties

| # | Finding | Where | Fix | Stage |
|---|---|---|---|---|
| U20 | A label and its values are a horizontal row: the values of two rows are not in a column. A vector has no X, Y, Z mark. A number has no unit (the snap fields have one). | `number`, `vector` (`widgets.rs:18-42`), `stage-27-layout.png` | A grid of two columns. A colored X, Y, Z prefix (the colors of the gizmo). Suffix `m`, `deg`, `s`. | 32 |
| U21 | The path of a texture and of a glTF file is typed. "Add > Model file" has the list of the files. | `texture` (`widgets.rs:171`), `mesh` (`widgets.rs:217`), `model_files` (`panels.rs:73`) | One file picker (the list, with the search field of stage 31) for the three places. The pictures come in stage 37. | 32 |
| U22 | The doorway of a door is picked as "0", "1". The outliner names it "doorway 0 (NegZ)". | `panels.rs:521` | The label of the outliner. | 32 |
| U23 | A cone light has two checkboxes, "cone" and "direction", one for each optional field. | `panels.rs:569-578` | One group "cone" with the angle and the direction. Check first if a direction with no cone has a use. | 32 |
| U24 | "Copy" has three meanings: a second item (`Command::Duplicate`), a second page, a command into the buffer. | `panels.rs:161`, `event_panel.rs:383`, `event_panel.rs:536` | "Duplicate" for the first two. "Copy" only for the buffer. | 32 |
| U25 | With more than one item selected, the panel changes the last one. | `properties` (`panels.rs:429`) | Not planned (see below). The line that says so stays. | - |

## Event panel

| # | Finding | Where | Fix | Stage |
|---|---|---|---|---|
| U26 | Two lines of explanation of the star are always under the page tabs. | `event_panel.rs:374` | Hover text of the star. | 32 |
| U27 | The place of a drop shows as the text "< here" beside the row. | `event_panel.rs:577` | A line between the rows, at the indent of the list that gets the command. | 32 |
| U28 | The buffer holds one command. No selection of a range of rows. | `event_panel.rs:539` | Shift+click selects a range in one list; copy, delete, and drag take the range. | 32 |
| U29 | "Add" is a flat menu of each name of the registry, with no description. | `event_panel.rs:507` | The search field of stage 31. A command set can give one line of help for each command (hover text). | 32 |
| U30 | A row is the command as RON: `Message("The lock is dead.")`. | `rows` (`events.rs`) | A built-in command has a row text for a reader: `Message: The lock is dead.`, `If power_off`. A command of another module keeps the RON text, or gives its own. | 32 |
| U31 | No button shows its key. The keys are a table in a fold of the debug column. | `ui/help.rs`, `tools` (`panels.rs:159`) | After the chords of M4: the hover text of a button has the key of its action, from the binding table. | 32 |

## Not planned

- A change of many selected items at one time (U25), and a box selection in the picture. Shift+click and the gizmo cover a room of this size.
- Panels that dock and float (a docking crate). Stage 31 gives fixed places with a width that the user sets.
- A file dialog of the OS. The room files are in one directory, and the editor lists it.
- A graph view of events, themes, a change of the key bindings in the editor.

## Order by cost and effect

1. U1, U2, U5: a loss of work. Small.
2. U6 to U8: stage 31, the largest effect on daily work.
3. U3, U12, U14, U15, U20: felt in each minute. Small each.
4. U17, U18, U28, U30: medium.
5. The rest.
