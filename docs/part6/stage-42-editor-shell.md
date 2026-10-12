# Stage 42: the shell of the editor

Plan: [README.md](README.md), 2.13. Report: `docs/part6/stage-42.md`. Needs: nothing of part 6 (the editor is still a mode of the game in this stage).

Goal: the editor has a layout that can hold all that the plan adds: docks with tabs, a registry of panels and of commands, one set of widgets, one theme. No new feature.
Gate: each thing that the editor does at `1c3158a` works in the new shell (the session of stage 40, by hand or by its script). A module adds a panel with one call and no change in the editor crate. The layout is saved, and "Window > Reset layout" gives the default. The game with no editor: `bench.py same trip_720p` says ok.

Decisions of the owner (2026-10-12): approved; `egui_dock` for the docks; no log panel in this stage.

Written from the code at `1c3158a`.

## Context in the code

- `Panels` (`crates/ui/src/lib.rs:156`): `add_panel` (line 229) for the debug panels of the game, `add_tool_panel` (line 252) with `ToolPlace` (line 95: top, left, right, bottom, status), `add_window` (line 298), `set_tool_layout` (line 201) with the sizes of the side panels.
- The picture of the game is not an egui image. `Picture` (`crates/platform/src/window.rs:73`) is its place and size in the window; `PictureFit::Window` (line 105) takes the window less a `WindowMargin` (line 62) of four sides. The renderer, the UI, and the editor camera read `Picture`.
- The editor crate: `panels.rs` (outliner, properties, tools, 908 lines), `event_panel.rs` (1186), `widgets.rs` (395: the picker, the file list), `overlay.rs` (the marks). `Command` (`lib.rs:189`) is each action of the editor; `BINDINGS` (`lib.rs:107`) has its keys; `left_width`, `right_width`, `event_height` (`lib.rs:146`) are saved in `[editor]`.
- The debug panels (Stats, Debug, Gameplay, Game state, Bench, Stages) are one window, "View > Debug panels".
- The UI has text only: no icon. The style is the default of egui with some changes.
- `docs/part5/ux-pass.md` has the findings U1 to U31; U17, U20, U25, and U31 are open in part (`OPEN_ITEMS.md`, "Editor UX: what stage 32 left").

## Rules of the layout (README 2.13)

1. The picture has the most room, and no panel is over it.
2. Nothing stops the work: no window that must be answered, but for a thing that cannot be undone. A long job runs behind and shows in the status row.
3. One mode: a game that runs. The frame of the picture has the play color then.
4. Each panel is a tab of a dock. A drop on a side splits; a drop on the middle groups.
5. The places that the other engines have: objects left, inspector right, files and log at the bottom, play in the middle of the tool row.
6. One widget for one job, in each panel.

## Prompt

```text
Stage 42: the shell of the editor.
Read docs/part6/README.md (2.13) and docs/part6/stage-42-editor-shell.md
first. Check the current version and API of egui_dock against the egui
version of the workspace before use.

Docks
- The tool layout of `Panels` is a tree of docks with tabs (egui_dock):
  left, right, bottom, and the middle with the picture. `ToolPlace` is
  the default dock of a panel, not a fixed place.
- The middle dock gives `Picture` its place and size. A tab of the
  middle is a document: one room now.
- A tab is moved, split, grouped, closed, and opened again from
  "Window". A tab can float in the window of the editor.
- The layout is saved (the seam of stage 30) and read at the start.
  "Window > Reset layout". Named layouts: Build, Logic, Look, Animate,
  Debug; with the panels of today some are the same.

Registries
- A panel: id, title, icon, default dock, and its draw function. A
  module adds it with one call. "Window" lists each panel.
- A command of the editor: id, name, key, and "can run now". The menus,
  the hover text of a tool button, the key help, and the command search
  (Ctrl+P) read this one list.

The panels of today, in the shell
- Outliner (left), Inspector (right; the properties of today), Events
  (bottom), Stats, Debug, Gameplay, Game state, Bench (bottom tabs,
  closed in the default layout).
- The tool row: tools and snap at the left, play and stop in the
  middle, the layout list at the right.
- The status row in four parts: the last message, the jobs that run,
  the count of problems, the frame time.

Widgets and theme
- One property grid (a label column and a value column, X Y Z marks,
  "back to default"), one tree (rename in place with F2, a filter, a
  menu on the right mouse button), one tool button (icon, hover text
  with the key), one notice that does not stop the work.
- The panels of today use them. No panel has its own form of these.
- One theme table: distances, text sizes, and the colors of an error,
  a warning, the selection, and a game that runs. An icon font, with
  its license in the repository.

Done when
- The gate.
- A test: a module of the test adds a panel and a command; they are in
  "Window", in the command search, and in the default dock.
- A test: the layout is written and read; a layout file with a panel
  that no module has is read with no error.
- `cargo test` and `cargo clippy` clean, with and without `editor`.
- A validation run of the editor and of the game.
- Pictures of the default layout and of the layout "Debug" in the report.
```

## What to watch for

1. **The picture in a dock.** `Picture` comes from the rectangle of the middle tab. A drag of a split changes the size of the internal picture in each frame of the drag: the renderer makes its images again. Measure it; if it is slow, the size changes when the drag ends.
2. **Input.** A click and a key go to egui or to the picture, never to the two (`Pointer::press`, stage 30). Check with a tab that floats over the picture: this is the one case where a panel is over the picture.
3. **A floating tab is in the window of the editor.** A panel on a second monitor needs a second window of the OS and a second swapchain: not in this stage (a new row in `OPEN_ITEMS.md`).
4. **The symbol count of the engine DLL** with `egui_dock` (51,784 of 65,535 in stage 41). Build the profile `dynamic` one time and write the count in the report.
5. **The game with no editor** has `add_panel` and the debug window as today. No dock code runs there.
6. **No empty tab.** The registry lets a later stage add Assets, Log, Build, Timeline, Look. This stage adds none of them.
7. **The saved layout of an old version.** A panel id that is gone, and a new panel that the file does not have: the panel is in its default dock, with no error.
8. **The findings of the UX pass that this stage closes** (U17 rename in place and filter, U20 the grid of the properties, U31 a button shows its key): say so in the report and in `OPEN_ITEMS.md`.
9. **The display at 3840 x 2160 and `[ui] scale`**: the default layout at 1.0 and at 1.5.
10. **Tests of the editor that click by position** (the session script of stage 40, the memory note on verification): the places of the panels change. Correct the script; do not drop it.
