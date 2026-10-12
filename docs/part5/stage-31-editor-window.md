# Stage 31: editor window

Plan: [README.md](README.md). Report: `docs/part5/stage-31.md`. Items: M3 of [`../part4/MISSING.md`](../part4/MISSING.md), U6 to U11 and the picker of U4 of [ux-pass.md](ux-pass.md).

Goal: the editor fits a 1920 x 1080 display, and each tool has a place of its own.
Gate: the editor fits the display with the default config. A window resize changes the view, not the panels. The pick and the gizmo are exact after a resize. The benchmark numbers do not move.

## Context in the code

### Picture size

- `WindowSettings::width` and `height` (`crates/platform/src/window.rs:72`) are the size of the picture. The window is the picture and `WindowMargin` (`window.rs:64`): `fit_window` (`window.rs:103`) asks for that size.
- The renderer takes the picture size from the settings: `extract_frame` (`crates/render/src/lib.rs:340`), with `picture_left` from the margin. A change of `WindowSettings` resets the accumulation (`lib.rs:337`).
- The UI puts the picture at `left_width, 0` with the size of the settings (`run_ui`, `crates/ui/src/lib.rs:310`), and writes the margin (`ui/lib.rs:391`).
- The editor: `editor_view` (`crates/editor/src/lib.rs:676`) makes the `View` from the settings size, and `viewport` takes `WindowMargin::left` from the pointer place (`lib.rs:326`). `overlay::draw` uses `ui.clip_rect()` as the picture (`overlay.rs:112`).

### Panels

- `LEFT_WIDTH` 240, `COLUMN_WIDTH` 440, the two panels `resizable(false)` (`ui/lib.rs:81-83`, `317`, `331`).
- `Place` (`ui/lib.rs:66`): `Column`, `Left`, `Window`, `Corner`, `Overlay`. The column is one scroll area with a fold for each panel; its first row is the "Panels" menu.
- The editor adds: the left panel (file, play, tools, outliner), "Properties" and "Where used" in the column, the overlay, the question window (`panels::build`, `crates/editor/src/panels.rs:27`).
- The event panel is the property panel of an event (`event_panel`, `crates/editor/src/event_panel.rs:265`): name, area, pages, conditions, trigger, command rows, arguments, test button, in one column.
- Pick lists are `egui::ComboBox` (`pick`, `event_panel.rs:92`; `choice`, `widgets.rs:112`). `pick_or_new` (`event_panel.rs:115`) adds a text field "new:".
- The status is `Editor::status`, one line in the left panel (`panels.rs:279`).
- The window title is set one time (`window.rs:197`).
- egui is 0.36; display scale is tested at 100 % only.

## Prompt to paste

```text
Stage 31: editor window.
Read docs/part5/stage-31-editor-window.md, item 3 of
docs/part4/MISSING.md, and findings U6 to U11 of docs/part5/ux-pass.md
first.

Picture
- One resource tells where the picture is in the window and how large it
  is. The renderer, the UI, and the editor read it; no other place
  computes it. In the game it is the size of [window], as today.
- In the editor the picture has the size of the room that the panels
  leave. The user sets the window size; the editor does not.
- A resize of the window or of a panel changes the picture size. During
  the drag the frame must not stop.

Panels of the editor
- Top: a menu bar (File, Edit, View) and the row with play, stop, the
  tools, and the snap.
- Left: the outliner. Right: the properties. Each with a width that the
  user drags, and that is saved (the seam of stage 30).
- Under the picture, while an event is selected: the event panel, with a
  height that the user drags. Pages, conditions, and trigger at the left;
  command rows in the middle; the arguments of the selected row at the
  right. Name, area, and notes of the event stay in the properties.
- Bottom: a status bar. An error is red and stays until the user closes
  it. The count of problems of the events is there.
- The debug panels (Help, Stats, Debug, Gameplay, Game state, Bench) are
  one window that the View menu opens. Closed at the start.
- During play: the running events and the game state take the place of
  the outliner tools and the properties. No disabled tools.
- The window title is the room file, the mark of a change, and the name
  of the editor.

Pick lists
- One picker with a search field for each list of names (flags,
  variables, lights, doors, spawn points, room files, commands). "New..."
  is an entry of the list where a new name is possible, not a field that
  is always there.

Out of scope: panels that dock or float, the fixes of stage 32, marks
with depth (stage 35).

Done when: with the default config the editor is in a 1920 x 1080
display; after a resize a click selects the item under the pointer and a
handle follows the pointer (a test of View with two sizes, and a check
by hand); the editor shows correctly at a display scale of 150 %;
bench.py compare against the run before is within noise and bench.py
same trip_720p is byte-equal. Update OPEN_ITEMS.md ("Size of the editor
window", "Display scale other than 100 %"). Pictures of the editor with
an event open, and during play. Stop and report.
```

## What to watch for

- **The game did not change.** A build with no feature `editor`: the window, the picture, and each benchmark number are as before. The panel code of the game (`Place::Column`) can stay as it is; the editor needs new places, not a change of the old ones.
- **Rule 2 and rule 3.** The picture size reaches the renderer in Extract, as the margin does today. No Vulkan type in the new resource.
- **Resize cost.** Each new picture size makes new images in the frame graph and resets the accumulation. A drag of a panel edge that does this in each frame can stop. Ask what the stage did (new size at the end of the drag, or in steps), and check validation during a resize.
- **Aspect ratio.** The view of the editor has the shape of its place, not 16:9. "View of the game camera" (`VIEW_GAME`) then shows more or less than the game: a frame of the game picture in the view is the honest answer.
- **Pointer offset.** One place subtracts the picture origin: after the stage, `rg "WindowMargin" crates/editor` has no hit.
- **Event panel height.** With the event panel open at 1080 pixels, the picture must stay large enough to place the area. Check the picture of the report.
- **The search field takes the keyboard.** A key typed into it must not start a tool (`InputCapture`).
