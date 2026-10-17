# Stage 59: the UI of the game

State: done. Stage file: [stage-59-game-ui.md](stage-59-game-ui.md). Tickets #69 to #73 (pull request #67). Plan: `README.md` 2.15, 3.2 item 7.

#69 on the stage branch; #70 as one lane; then three lanes from it at the same time: #71, #72, #73. Two fixes between the lanes by hand (the menu panel for the rows of #72; the pause menu opens the items menu). Then one code review (standards and spec) and one lane of fixes.

## 1. What was built

- **The module of the game UI** (#69): `GameUiModule` in `crates/ui` (`game_ui.rs`): the text box of the game moved into it, the same in look. `[game_ui] font` (a TTF or OTF file) is the first font of the family `game`, the default fonts after it; `[game_ui] scale`; the editor keeps its fonts. A changed font file installs again (the watch has it).
- **Menus** (#70): the data asset `Menu` (`data/menus/<name>.menu.ron`): a title, a place, `focus`, `back`, rows. Read through the command registry (`read_menu`, the store `MenuFiles`, in the watch and the room check). The stack `Menus`; `OpenMenu`, `CloseMenu`, `CloseMenus`; `ScriptContext::open_menu`, `close_menu`. Input by actions: up, down, interact, and the new `BACK` (`X`, `Backspace`); the focus skips a row that does not hold; a pick runs the commands of the row as an event (`Start::Menu`). A menu with focus blocks the walk; a message over a menu takes the input; in a pause the menus take their input in `Update`. The UI draws a view (`ShownMenus`) that the events build.
- **Screens** (#71): `[screens] title`, `pause`, `game_over` (in `project.toml`, and the config over it); the title menu in `Title`, the pause menu on the pause action, the game-over menu on `GameOver`. Commands `NewGame`, `LoadSave`, `QuitGame`, `GameOver`; condition `SaveExists`; `SetLanguage`. The menus of `game/` (title, pause, game over); `game/` starts in play.
- **Items and HUD** (#72): the row `Items` (each item kind with a count above 0, its name and count, the description of the item of the focus; a pick runs the routine `use_routine` of the item kind) and the row `Text` (a line with `{name}`); a menu with no focus and `Text` rows is a HUD. The items menu of `game/`, opened from the pause menu.
- **Menus in the editor** (#73): Assets > "New menu" and "Open menu"; an open menu in the event panel: its fields and rows at the left, the text key, the conditions, and the command rows of a row in the middle, the arguments at the right; a change writes the file. "Add" makes a row with commands, the items, or a text.
- **Review fixes**: the UI only reads the view of the menus; the focus is always on a place of focus (the first at open, the nearest when its row stops to hold), and a menu with none has no mark; `CloseMenu` closes the top menu with focus, a HUD only when no menu has focus; the font in the watch.
- **Fixes of the run by hand**: with `[rules] start = "Title"`, the title menu showed in the view of the editor while it edits (no play): no screen opens while a tool stops the game (`Time::paused`). A menu box is at least 240 points wide (the rows wrapped after a word).

## 2. Results

| Check | Result |
|---|---|
| Gate: the title menu, new game, continue with and without a save, quit, the pause menu and its close, the items row, a HUD, game over, a menu of a script, the focus and the input | `crates/events/src/tests/menus.rs`, `tests/menu_items.rs`, `crates/gameplay/src/screens.rs`, `crates/ui/src/game_ui.rs` |
| Tests of "Done when" | Each line has a test (the files above, `menu_panel.rs`, `tests/text.rs`) |
| Tests and clippy | `tools/check.sh` on the merged tree with the fixes: format, comments, clippy and tests static and dynamic, symbol count, reload test: all pass |
| Validation and trip run (debug) | No validation error, no warning; the two messages of the trip, 3 room loads, 8 event starts, as stage 58; no menu opens |
| Picture gate (`bench.py gate`) | `trip_720p`, `walk_720p`, `materials_720p`, `textured_walk_720p` equal; `test_1080p` noise (689 pixels at 0.0005) |
| Symbols of the engine DLL | 16,800 of 65,535 (stage 58: 16,719) |
| Benchmark against `s58` | Run `s59`. GPU total -5.6 % to +4.5 %; `heavy_mixed_lights8_1080p_bounces2` +4.5 %, then `bench.py confirm` against `main`: +1.4 % and +2.3 % in two rounds, with the base itself 6.6 % apart between rounds. The owner used the GPU for other work in the same time: the machine |
| Run by hand in the editor | By the input script, with `[rules] start = "Title"`: play shows the title menu ("New game", "Quit"; no "Continue" with no save); "New game" plays; the pause action opens the pause menu (Back, Items, Quit to title); "Items" opens the items menu over it; back and back again play. Assets > "New menu" writes `data/menus/check.menu.ron` and opens it; "Add" offers the three kinds; a row with commands is in the file, with its text key pick, conditions, and command rows. Two fixes came from it (section 1). Not by hand: game over (no room starts it; its test covers it), a font file of the owner (none here) |

## 3. Deviations from the stage file

- **`[screens]` in place of `[game_ui] title, pause, game_over`**: `crates/gameplay` opens the screens and cannot see `GameUiSettings` of `crates/ui`. The table is in `project.toml` (the project names its menus, as its start room) and the config can give it.
- **The use of an item is a routine** (`ItemKind.use_routine`), not a command list: an item kind is a data asset that serde reads with no command registry, and the routine panel edits the routine.
- **`Menu` is not registered with `add_data_type`**: its commands need the registry. The data panel lists a menu file as an unknown type; the menu panel opens it.
- **The font path** is from the working directory, as the other paths of the config, not from the project folder.
- **`CloseMenu`** closes the top menu with focus (as `BACK`), not the top menu (from the review: a HUD over the pause menu took the close).

## 4. Decisions where the stage file was silent

- The focus stops at the first and the last place; it does not wrap. A menu that opens again starts at its first place of focus.
- A menu keeps the object that opened it; a pick runs with it (so `CallScript` has an object). A menu of the screens has none.
- `NewGame` and `LoadSave` ask, and an `Update` system does the work: `ResetPlay` needs the event runner, which a running command has out of the world. `NewGame` clears the flags and variables.
- The title and game-over menus open again while their state lasts and no menu has focus.
- In a pause, a press does not reach the first fixed step after the pause (`FixedActionState::forget_changes`).
- The item rows are in the order of the paths of the item files.
- `SaveExists` reads a flag `SavedGame` that the save module keeps, not the disk at each step.

## 5. Uncertain

- A menu under the menu of the focus shows through it at the same place (the items menu over the pause menu): the look of a stack is the default one.
- `menu.rs` has many jobs (600 lines: format, store, stack, focus, input, view); the focus is a pair of numbers; `new_menu`, `new_routine`, and `new_data_asset` share their name check and their write. Left as they are.
- An item file that is new while the game runs is not found by its id until the table of ids is made again.
- The left column of the menu panel is narrow ("Close" is cut at the default layout).
- The text box over a menu in a real window was seen by its test, not by hand.
