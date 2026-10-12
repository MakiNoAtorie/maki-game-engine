# Stage 26: editor shell

Plan: [README.md](README.md). Report: `docs/part4/stage-26.md`.

Goal: an editor mode where a room is opened, inspected, changed, saved, and played, in the same window as the game.
Gate: a room is changed and saved in the editor and plays the same in the game.

## Context in the code

### The document

- `RoomDef` (`crates/scene/src/room.rs`) is `Clone`, `PartialEq`, `Serialize`, `Deserialize`, with `deny_unknown_fields`: width, depth, height, doorways, props, lights, doors, movers, spawns.
- `RoomDef.events` is `IgnoredAny` with `skip_serializing`: `scene` does not read the events, and a `RoomDef` that is written back has none. `events` reads them from the same text (`parse_events`, `Vec<EventDef>`). The document is the two parts, and a save writes one file.
- After stage 24, an event command is written through the command registry.
- `name` is `Option<String>` on props, lights, doors, and movers. The outliner needs a label for an item with no name.
- `spawns` is a `BTreeMap<String, [f32; 3]>`: a save writes them in name order, and a spawn has no place for more fields.
- `DoorDef.doorway` is an index into `doorways`.
- The room files use short forms: fields at their default are left out (`pose: (position: (..))`), one item on one line.
- The three room files have 29 comment lines (14, 7, 8): a file header that names the format, and design notes on props, lights, and events. The `ron` crate does not keep comments.

### Load and reload

- `RoomRequest` and `load_requested_room` load a room in full. `CurrentRoom` has the path, the `RoomDef`, and the text.
- Cost of a full load: 2 ms for a room with generated meshes, 29 ms for the first load; each mesh group waits for its BLAS build (`OPEN_ITEMS.md`, "BLAS build stops the game"). A reload keeps the model files that the room still names.
- A change of a light or an object cuts the history of the indirect light to `light_history_samples` for some frames. A full reload in each frame of a slider drag gives a noisy picture, not only a slow one.

### UI

- The picture has the size of the config and is not scaled. Panels are a column at the right of the picture, and the window is wider by the column (`WindowMargin`). `add_panel` adds to the column, `add_window` adds a floating window, `add_corner` a readout on the picture.
- A closed panel opens again only from code (`OPEN_ITEMS.md`, "List of panels").
- egui 0.36 with no clipboard; user textures and paint callbacks are not drawn (`OPEN_ITEMS.md`, "egui features that are off"). No file dialog crate is in the workspace. The room list of `stages.rs` reads the room directory.
- Tested only at a display scale of 100 %.

### Camera and input

- `Camera` is a component of `scene`. The renderer uses the first `Camera` entity (`OPEN_ITEMS.md`, "Two cameras").
- The action system has keys and the wheel only. Pointer position, mouse buttons, and drag are new in `platform`. `InputCapture` tells that the UI has the pointer or the keyboard.
- After stage 24, a module registers its actions and bindings. The help panel is made from the bindings.

### Play mode

- State that events change: `GameState` (flags, variables) in `events`; `Light` and `Mover` components of room entities; the room itself (`Transfer`); `ScreenFade`; `MessageBox`; `EventRunner`.
- The `Player` entity lives over room changes. An editor with no player and an empty room is the case "TLAS with no instance" of `OPEN_ITEMS.md`, which did not run yet.
- The game binary has the cargo feature `profile`: the pattern for a feature `editor`.

## Prompt to paste

```text
Stage 26: editor shell.

Goal: an editor mode where a room is opened, inspected, changed, saved,
and played, in the same window as the game. Plain default egui look, no
theming. Read docs/part4/stage-26-editor-shell.md first.

Structure
- A new crate `editor` at the top of the dependency order (after ui, before
  the binaries). The game binary gets it through a cargo feature `editor`
  and a `--editor` flag. A build without the feature has no editor code.
  Say if you prefer a separate binary and why. Update rule 1 in CLAUDE.md.
- The editor edits a document: the RoomDef of the open room and its events
  (Vec<EventDef>; RoomDef does not hold them), plus a dirty flag and the
  file path. The live world is rebuilt from the document when it changes.
  Rebuild only what changed where that is simple (one prop, one light); a
  full room reload is fine for the rest. Note the cost of a full reload
  (BLAS builds wait on the GPU, see OPEN_ITEMS.md).
- Editor panels: an outliner (room settings, doorways, doors, props,
  lights, movers, spawns, events by name; an item with no name gets a label
  from its kind and index), and a property panel for the selected item with
  plain widgets (numbers, vectors, colors, enums, file paths, the texture
  fields of stage 25). Events are listed only; the event editor is stage 28.
- A menu opens and closes each registered panel.
- The rendered picture is not hidden under the panels: panels dock at the
  sides, and the viewport fills the rest. Today the picture is not scaled
  and the window is wider by the panel column (WindowMargin). Keep that
  scheme with a margin at each side, or size the render target to the
  viewport, or use the full window with the camera adjusted: choose and
  say why.

Camera and input
- platform gets pointer input: position, buttons, and drag, with
  InputCapture respected. Gameplay still reads actions only (rule 4).
- Free editor camera: orbit, pan, zoom with the mouse; keys for a few fixed
  views (top, the game camera). The game camera stays as it is in play mode.
  A marker selects the active camera (OPEN_ITEMS.md, "Two cameras").
- Editor actions go through the action registration of stage 24, with
  their own binding table and a help panel generated from it.
- Mouse input over a panel does not move the camera.

Files
- Open room (a list of the room directory), save, save as, new room (empty
  box with defaults). No file dialog crate unless you give the reason.
- Saving writes RON that the game loads and that a person can still read and
  diff: stable field order, fields at their default left out, one item per
  line where it fits. Save room_a.ron with no change and show the diff in
  the report.
- Room files hold design comments (29 lines in the three files). Saving
  must not drop them silently. The ron crate does not keep comments. Either
  preserve them with a writer of our own, or move explanatory comments into
  a `notes` field on the items and on the room (convert the existing files
  with code). Spawns are a map of name to position: say where their notes
  go. Choose one and justify it; ask before choosing if neither is simple.
- An unsaved change asks before close, open, or play.

Play mode
- Play: snapshot the document and the game state, run the game modules from
  the room's spawn point (or the editor camera position, as a choice), with
  the game camera and controls.
- Stop: back to the editor with the document and the game state as they
  were before play. Changes made by events during play (flags, variables,
  lights, movers, a Transfer to another room, a fade, an open message) do
  not leak into the document or the editor.

Out of scope: picking, gizmos, undo (stage 27), the event editor (stage 28).

Done when a room is opened, a prop and a light are changed in the property
panel, the file is saved, the game (without the editor feature) loads it
and shows the change; play/stop restores the state, also after a Transfer;
a release build without the feature has no editor symbols; benchmarks keep
their numbers; validation is clean, also for a new empty room. Update
OPEN_ITEMS.md ("Two cameras", "List of panels", "TLAS with no instance").
Stop and report.
```

## What to watch for

- **Editing the world, not the document.** If the property panel writes to ECS components and a save reads them back, rule 8 erodes and play/stop gets fragile. The document is the only thing that is saved.
- **Lost events.** `RoomDef` does not serialize its events. Open `room_a.ron`, save it, and check that the three events are still in the file.
- **Comment loss.** Open `room_a.ron`, save it with no change, and diff. Whatever the stage chose, the design notes must survive.
- **Full reload on every slider drag.** A drag of the intensity of a light must feel instant. If it hitches or the picture turns to noise, the rebuild is too coarse.
- **Play that leaks.** Play, press the switch in room B, walk back to room A, stop. The editor must show the room and the flags of before the play.
- **Feature leakage.** Build release without `editor` and check that the binary size and the benchmark numbers agree with the stage 25 build.
- **Game input in the editor.** With the editor open and not in play mode, the move keys must not move the player, and text typed into a field must not start an action.
