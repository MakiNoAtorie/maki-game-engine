# Stage 59: the UI of the game

Plan: [README.md](README.md), 2.15 ("UI of the game", "Menus", the slice items 1, 4, 7), 3.2 item 7 (menus, inventory, HUD). Report: `docs/part6/stage-59.md`. Needs: stage 58 (text in tables).

Goal: the UI of a game is a module of its own on egui, and the menus are data. A menu is a data asset `Menu`: its rows, and for each row the commands that it runs (a command, a routine, a script function). The module draws a stack of menus with focus, the message box, the choices, a list of the items, and a HUD; the player moves the focus with input actions. The title, pause, inventory, and game-over screens are menus of the project; a script can open a menu that it makes. The texts are keys of the text tables, and the game UI has a font of the project (Japanese).
Gate: a test in code: a title menu at the start of a game with `[rules] start = "Title"`: "New game" goes to the start room in `Play`, "Continue" is there only with a save and loads it, "Quit" ends the program; in play the pause action opens the pause menu and its "Back" row closes it and plays again; a menu row `OpenMenu(inventory)` shows each item of the inventory with its name and count, and a pick of an item runs its commands; a HUD menu shows `{coins}` and takes no input; `GameOver` shows the game-over menu; a menu from a script opens and closes; up, down, interact, and back move and take the focus. The game plays as before: the trip run passes, `bench.py gate` shows the same pictures, the GPU time against `s58` is the same.

State: written by the session from the plan and the code at commit `a933137`. Approved by the owner (2026-10-17).

## Now (the code at stage 58)

- The text box of the game (a message, a question with its answers) is an overlay of `EventsUiModule` (`crates/ui/src/events.rs`). It reads `MessageBox` and `ChoiceBox` (`crates/events/src/interpreter.rs`); the events know nothing of egui. It uses the egui default fonts and the theme of the editor.
- The states of the game: `PlayState` (`Title`, `Play`, `Pause`, `Cutscene`, `End`) with `can_change` (`crates/core/src/play_state.rs`); `GameRules` (`[rules] start`, `pause`) and the command `SetPlayState` (`crates/gameplay/src/game.rs`); the pause action `P`. Nothing is drawn in `Title`, `Pause`, or `End`.
- No command starts a new game, loads the save, or ends the program from an event: the save loads at the start and by a debug key (`crates/gameplay/src/save.rs`); `QuitRequested` (`maki_platform`) ends it, on the debug quit action (`Esc`, `crates/gameplay/src/controls.rs`).
- The input actions of the text box: `INTERACT` (`Z`, `Enter`), `CHOICE_UP` (`Up`, `W`), `CHOICE_DOWN` (`Down`, `S`) (`crates/events/src/lib.rs`). No back action.
- Data assets (stage 57) and text tables (stage 58): `DataAsset`, `DataRef<T>`, `Texts`, `TextKey`, `{name}` of a variable. The inventory (`Inventory`, `ItemKind` with a name and a description by key, an icon) has no view.
- A routine file is a command list that the routine panel edits with the rows of the event panel (`crates/editor/src/routine_panel.rs`). A data asset is edited in the inspector, which has no widget for a command list.
- The picture of `bench.py gate` is the image of the frame graph (`dump_picture`, `crates/devtools/src/bench.rs`), not the egui overlay.

## The way

- **The module of the game UI.** `GameUiModule` in `crates/ui` (egui is there; rule 1: `events` and `gameplay` do not see egui). It draws what resources of `crates/events` hold: `MessageBox`, `ChoiceBox`, and the menu stack. The text box moves from `EventsUiModule` into it, the same in look. A game and the editor (the picture of a play) add it; it draws only while the game has the picture.
- **The font and the look of the game.** `[game_ui] font = "<file>.ttf"` (a TTF or OTF of the project, from the directory of the project): the first font of an egui family of its own (`FontFamily::Name("game")`), the default fonts after it for the glyphs it has not. The editor UI keeps its fonts. `[game_ui] scale` for the size of the text. No font file: the default fonts, as now.
- **A menu is a data asset** `Menu` (`NAME = "menu"`, `data/menus/<name>.menu.ron`), in `crates/events` (`menu.rs`): a title (`Option<TextKey>`), a place on the screen (`Center`, `Top`, `Bottom`, `TopLeft`, `TopRight`, `BottomLeft`, `BottomRight`), `focus` (true: it takes the input and blocks the player as a message does; false: a HUD), `back` (true: the back action closes it), and its rows. A row is one of:
  - `Commands(text, conditions, commands)`: shown while its conditions hold (as a page); a pick runs its commands as an event runs a page (`RunningEvent`): a command, `CallRoutine`, `CallScript`.
  - `Items`: a row for each item kind of the inventory with a count above 0: its name, its count, its description under the list for the row of the focus; a pick runs the commands `use` of the item kind (a new field of `ItemKind`, empty by default).
  - `Text(text)`: a text with no pick, with `{name}` of the variables (a HUD line).
- **The menu stack** is a resource `Menus` (`crates/events`): the open menus, each with its focus row. Commands `OpenMenu(DataRef<Menu>)` (on the top of the stack), `CloseMenu` (the top one with focus; with none, the top HUD), `CloseMenus` (all with focus). A menu of a script: `ScriptContext::open_menu(Menu)` and `close_menu()`. The input goes to the top menu with focus; `CHOICE_UP`, `CHOICE_DOWN`, `INTERACT`, and a new action `BACK` (`X`, `Backspace`). While a message shows, the message has the input. A closed menu does not keep its focus row.
- **Title, pause, game over.** `[game_ui] title`, `pause`, `game_over`: menu files. In `Title` the title menu shows (`[rules] start = "Title"`); the pause action opens the pause menu in `Play` (state `Pause`), and its close goes back to `Play`. New commands in `crates/gameplay`: `NewGame` (back to the start of a play: `ResetPlay`, the start room, `Play`), `LoadSave` (the save of the game, `Play`), `QuitGame` (`QuitRequested`), `GameOver` (state `End`, the game-over menu). A condition `SaveExists`, so "Continue" shows only with a save. `SetLanguage(language)` (`crates/events`, `Texts::set_language`) for an options menu.
- **The content of `game/`**: `data/menus/title.menu.ron`, `pause.menu.ron`, `items.menu.ron`, `game_over.menu.ron`, their texts in `en.text.ron`; `game/` keeps `[rules] start = "Play"` in its config, so the benchmark and the trip start in play as now.
- **Menus in the editor.** A menu file opens in a menu panel (as a routine opens in the routine panel): its title, place, `focus`, `back`, and its rows; the commands and conditions of a `Commands` row are rows of the event panel (`command_rows`, `condition_row`). Assets > "New menu" and "Open menu". The game UI of a play in the editor shows the menus.

## Tickets

Approved by the owner (2026-10-17). GitHub issues #69 to #73.

| Ticket | Blocked by |
|---|---|
| #69 The module of the game UI: the text box in it, the font and the look of the game (prefactor) | none |
| #70 Menus as data assets and the menu stack, with focus and input | #69 |
| #71 Title, pause, and game over; new game, continue, quit, language | #70 |
| #72 The list of the items and the HUD | #70 |
| #73 Menus in the editor | #70 |

#71, #72, and #73 can run at the same time after #70: #71 writes `crates/gameplay` and the menus of `game/`; #72 writes the item and text rows of `menu.rs`, `items.rs`, and their drawing; #73 writes a new panel file in `crates/editor`.

## Done when

- The gate.
- Tests: the text box of the module shows a message and a question as before; a font file of the config is the first font of the family `game`, and a missing file logs an error and keeps the default fonts; a menu reads and writes as a data asset; `OpenMenu`, `CloseMenu`, `CloseMenus`; the focus moves with up and down, skips a row whose conditions do not hold, and a pick runs the commands of the row (a command, a routine, a script function); the back action closes a menu with `back` and not one without it; a menu with focus blocks the walk, a HUD does not; a message over a menu takes the input; `ScriptContext::open_menu` and `close_menu`; the title menu in `Title`, "New game", "Continue" with and without a save, "Quit"; the pause menu and its close; `GameOver`; `SetLanguage`; the items row lists the items with a count above 0 with their names and counts, and a pick runs the `use` commands of the item; a `Text` row with `{coins}`; the menu panel shows the rows of a menu and a change writes the file; "New menu" writes a menu file.
- Tests and clippy, static and dynamic; a validation run; the trip run; `bench.py gate`; the benchmark against `s58`; the symbol count of the engine DLL; the reload test.
- A run by hand, by the input script: the editor with "New menu", a row with a command, play with the menus of `game/`: the title, new game, the pause menu, the inventory with an item, a HUD line, the game-over menu; and the game program with `[rules] start = "Title"`. One run with a Japanese font file and the `ja` table, if a font file of the owner is there.

## What to watch for

1. **The pictures**: the gate compares the image of the frame graph, not the egui overlay; the look of the text box is checked by its test and by the run by hand. The menus are not drawn in the benchmark (it starts in `Play` with no HUD).
2. **The input**: the menus read input actions, never keys (rule 4). `BACK` must not take `Esc`, the debug quit key, or `P`, the pause key.
3. **The step**: a menu row runs its commands in the fixed step as an event; the UI only asks for it (a resource). No command runs inside the egui frame (`maki_ui::queue_command` is of the editor, not of the game).
4. **States**: `Pause` stops the fixed step, so the pause menu must take its input outside it; `NewGame` and `LoadSave` from `Title` go along `can_change`.
5. **The font**: the game family only; the editor keeps its fonts and its icons (the symbol fonts of `crates/ui/src/theme.rs`). A font file is read one time, and again on `AssetChanged`.
6. **The script DLL**: `ScriptContext` gets new functions. The reload test passes; the templates of "New script" build.
7. **Not in this stage**: the gamepad (stage 65; the actions are ready for it), animation of the UI, a widget editor (the layout is the place and the rows), menus with pictures beside the item icon, save slots (one save file, as now).
