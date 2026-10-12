# Stage 42: the shell of the editor

State: done. Stage file: [stage-42-editor-shell.md](stage-42-editor-shell.md). Plan: `README.md`, 2.13.

## 1. What was built

- **Docks** (`maki_ui`, on `egui_dock` 0.21.1 for egui 0.36): the tool layout of `Panels` is a tree of docks with tabs. A panel of a module is one tab; its `ToolPlace` is its dock in the default layout only. The picture is the tab in the middle: the place of its body is the `WindowMargin` of the platform crate, so `Picture`, the renderer, the marks, and the pick did not change. A tab is moved, split (the five drop targets of a dock), grouped, closed, opened again from "Window", and it floats in the window of the editor.
- **The layout** is written to a file beside the config file (`config.layout.ron`) at the end of each change, and read at the start. "Window > Reset layout" gives the default. Named layouts: Build, Logic, Look, Animate, Debug, in "Window > Layout", in the list at the right of the tool row, and in the command search.
- **Two registries** (`maki_ui`): a panel (`add_tool_panel`: id, title, icon, default dock, draw function) and a command (`add_command`: id, name, menu, the action of its key, "can run now", what it does). The menus, the tool buttons, the keys, the help panel, and the command search (Ctrl+P) are made from the command list. "Window" and the command search list each panel.
- **The panels of today in the shell**: Outliner (left), Inspector (right), Events (bottom); Help, Stats, Debug, Gameplay, Game state, Bench as bottom tabs that the default layout closes. The tool row: tools and snap at the left, play and stop in the middle, the layout list at the right. The status row: the last message, the jobs, the count of the problems, the frame time.
- **Widgets** (`maki_ui::widgets`): one property grid (a label column and a value column, also for a group in a group; X, Y, Z marks; "back to default"), one tree (filter, rename in place with F2, the menu of the right mouse button), one tool button (icon, hover text with the name and the key of its command), one notice that stops no work.
- **One theme table** (`maki_ui::theme`): distances, text sizes, the colors of an error, a warning, the selection, a game that runs, and the axes; and the icons.
- **One mode is marked**: while a game runs, the frame of the picture and the stop button have the play color.

Pictures: [the default layout](pictures/s42_default.png), [the layout "Debug"](pictures/s42_debug.png), [the default layout at `[ui] scale = 1.5`](pictures/s42_scale15.png). The display was 3840 x 2160 at 100 %; the files have half the size.

## 2. Results

| Check | Result |
|---|---|
| `cargo test`, without and with `editor` | 151 and 41 passed, 0 failed (stage 41: 148 and 40) |
| `cargo clippy --all-targets`, without and with `editor` | Clean |
| Test: a module adds a panel and a command | `a_module_adds_a_panel_and_a_command_to_the_shell_of_a_tool` (`crates/ui`): in "Window", in the command search with its key, in its default dock; its key runs it |
| Test: the layout is written and read | `the_layout_is_written_and_read` (`crates/ui`): a file with a panel that no module has, and a panel that the file does not know |
| Test: the editor in the shell | `the_shell_has_the_commands_and_the_panels_of_the_editor` (`crates/editor`); the 35 tests of before pass with no change (keys, clicks in the picture, play and stop) |
| The game with no editor | `bench.py same trip_720p s41 s42`: ok. `gpu_total` of the 26 scenarios: -1.0 % to +0.6 % against `s41`. The window: the column with its sections, as before |
| Validation run of the game | `--bench-frames 900 --bench-script trip`, `MAKI_LOG=debug`: the trip ran, no validation error, no error line |
| Validation run of the editor | Sessions of some minutes with `MAKI_LOG=debug`: no validation error, no error line |
| Symbols of the engine DLL (profile `dynamic`) | 54,809 of 65,535 (stage 41: 51,784). 16 % is left |

The session in the editor (scripted clicks and keys, pictures and the log read back):

| What | Result |
|---|---|
| Select in the outliner and in the picture; the gizmo | The lamp and a cube were selected; a drag of the X handle moved the cube; Ctrl+Z put it back |
| Rename in place | F2 on the lamp, a new name, Enter: "5 event commands now name `lamp2`"; Ctrl+Z |
| An event | A click on "flicker": the Events tab shows its pages, commands, and arguments |
| Play and stop | F5: the picture has the play frame, the tool row has "Stop", the outliner and the inspector show the game; F5 again: the room is back |
| The menus | File, Edit, Add, View, Window: the commands with their keys, grey when they cannot run |
| The command search | Ctrl+P, "layout debug", Enter: the layout "Debug"; "window inspector": the closed tab is open again |
| A tab | Dragged out of its dock: it floats. On the bottom target of the left dock: a split. On the middle target: a group. Closed with its button, opened from the search |
| The layout over a new start | The docks, the layout name, and the place and size of a floating tab are as before the close; the same over a second start |
| Input (watch 2) | A right drag on a floating tab over the picture and on the outliner: the camera does not turn. On the picture: it turns |
| A drag of a split (watch 1) | The images of the renderer were made again 2 times in a drag of 1 s: the size of the picture follows when the pointer rests (`PICTURE_REST_FRAMES`, stage 31). No change was needed |
| `[ui] scale` 1.0 and 1.5 (watch 9) | The default layout is right at the two. At 1.5 the docks were too narrow at first: see section 6 |

## 3. Files

| File | Role |
|---|---|
| `crates/ui/src/shell.rs` (new) | The shell: the dock tree, the tabs, the menu row, the tool row, the "Window" menu, the command search, the layout file, the named layouts |
| `crates/ui/src/commands.rs` (new) | `ToolCommand`, `ToolCommands`, `add_command`, the keys of the commands (`command_keys`), the row of a command in a menu |
| `crates/ui/src/widgets.rs` (new) | The property grid, the tree, the tool button, the notice, the jobs and the frame time of a status row |
| `crates/ui/src/theme.rs` (new) | The theme table and the icons |
| `crates/ui/src/lib.rs` | `ToolPanel`, `add_tool_panel`, `add_menu`, the layout functions of `Panels`; `run_ui` calls the shell; the pointer rule of a tool; the two tests |
| `crates/ui/src/stats.rs` | `FrameTime`: the frame counter and the status row read one value |
| `crates/ui/src/help.rs` | A row of a key has the name of its command |
| `crates/editor/src/lib.rs` | `add_commands` (the command list of the editor, in place of `Editor::keys`), `Command::Rename`, `Editor::rename` and `rename_spawn`, the layout file, the notice of an error; `[editor]` has no panel sizes |
| `crates/editor/src/panels.rs` | The panels on the registries and the widgets: the menus (what is under their commands), `tools`, `play_row`, the outliner on the tree, the inspector on the grid, the status row in four parts; the named layouts |
| `crates/editor/src/widgets.rs` | The rows of a room file (file, texture, mesh, pose, look) on the grid of `maki_ui` |
| `crates/editor/src/event_panel.rs`, `overlay.rs`, `camera.rs`, `document.rs` | The colors of the theme; two rows on the grid; the play frame; the two views as commands; `Item::has_name` |
| `crates/editor/src/tests.rs` | The test of the editor in the shell |
| `crates/core/src/config.rs` | `ConfigFile::beside`: a file beside the config file |
| `Cargo.toml`, `crates/ui/Cargo.toml` | `egui_dock` with `serde`; `ron` in the ui crate |
| `licenses/` (new) | The licenses of the two symbol fonts of egui |
| `.gitignore`, `CLAUDE.md`, `OPEN_ITEMS.md` | The layout file; rule 5 and the line of `ui`; the rows of this stage |
| `docs/part6/pictures/` | The three pictures |
| `tools/bench_history.csv`, `tools/bench_env.csv` | The runs `s41` and `s42` |

## 4. Deviations from the stage file

- **The layout is not in the config file.** The seam of stage 30 writes `key = value` lines of one table; a dock tree is not that. The layout is a file of its own beside the config file, written when a change ends (the release of a mouse button, or a command). The test of the stage file names "a layout file".
- **`[editor]` has no `left_width`, `right_width`, `event_height`.** The layout file has the sizes. A config file with these keys gives an error at the start that names them (`deny_unknown_fields`, as for each unknown key). The `config.toml` of the repository root has no `[editor]` table.
- **No font file.** The icons are characters of the two symbol fonts that egui has built in (`emoji-icon-font`, MIT; `NotoEmoji`, OFL). Their licenses are in `licenses/`. No download and no new crate. A set made for tools (Phosphor, Lucide) is a new file or a new crate: a row in `OPEN_ITEMS.md`.
- **The debug panels keep their widgets.** Stats, Debug, Gameplay, Game state, Bench, and Help are tabs of the shell, but not on the property grid: a game shows the same panels in its column, and watch 5 keeps that as it is.
- **"Add" is a menu of the menu row**, not a button of the tool row: its kinds are commands of the list.
- **The Events tab stays** when no event is selected (one line of text). It opens and comes to the front when an event becomes the selected item. Before, the panel showed and hid with the selection.
- **An error of a command is a notice** at the bottom right, not a red text in the status row. The status row has "The command failed".
- **The frame counter in the corner of the picture is closed in the editor**: the status row has the frame time, and nothing is over the picture.

## 5. Decisions where the stage file was silent

- **The registries are in `maki_ui`**, not in the editor crate: a module to the left of the editor (devtools, a script crate) adds a panel and a command.
- **A command**: `can_run: fn(&World) -> bool` and `run: Fn(&mut World)`. Its key is not a text: it names an input action, and the binding table has the key. `command_keys` runs before the queue of the editor, so a key is a command of its frame as before. A command that cannot run does nothing (before, "Undo" with no step wrote a status line).
- **A named layout** says which tool panels it closes, if the debug panels are open, and the sizes of the three docks in points. A panel of a module that no layout names is open as its module added it.
- **A panel that the layout file does not know** is new: it is open as its module added it, in its default dock. The file has the list of the panels that it knew.
- **"Back to default"** is on the rows whose value the room file can leave out (pose, `blocks the player`, `on`, the scale of a texture, the values of a sound): the default is that of the file format.
- **Rename** is a command (F2, "Edit", the menu of a row). The text field starts with the label of the row.
- **A tab that starts to float** gets 460 x 520 points. `egui_dock` gives it the size of its dock, and the bottom dock is as wide as the window.
- **A dock that "Window" makes again** has the size that the layout of now gives to its side.
- **The pointer in a tool**: it is on a panel at each place but on the body of the picture tab with no floating tab over it. (`egui_wants_pointer_input` cannot tell: the docks fill the window.)
- **"View from the top" and "View of the game camera" are commands** (the "View" menu), not actions that the camera system reads.
- **Two new public things of other crates**: `ConfigFile::beside` (core), `FrameTime` (ui).

## 6. What the check found, and what was done

1. **`[ui] scale = 1.5`: the docks were two thirds of their size.** egui takes a new zoom at the start of its next frame, so the first frame had the window size in points with no zoom. `UiModule` now runs one empty egui frame after it sets the zoom.
2. **A floating tab was as wide as the window**, and after a new start it was in the top left corner. `egui_dock` 0.21.1 has a field for the place of a window that it never writes. The shell gives a new window its size, reads the place from egui, and writes it to the layout file.
3. **A dock that was made again had one fifth of the window**, not its size in points: fixed (section 5).
4. **`has_glyph` of egui says "no" for each character of the font that has the replacement character.** A test with it was wrong. The 18 icons were checked against the character tables of the two font files.

## 7. Uncertain

- The session was scripted: no person used the shell. The hand session of the stage 32 gate is still open (`OPEN_ITEMS.md`).
- One display (3840 x 2160 at 100 %), `[ui] scale` 1.0 and 1.5. In a narrow window the middle of the tool row is over its left part; the width where that starts was not measured.
- The layout file has the dock tree in the serde form of `egui_dock`. A later version of that crate can refuse an old file; the default layout is used then, with one warning in the log.
- The symbol count grew by 3,025. Stage 43 moves crates into the DLL: count again there.
