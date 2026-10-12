# Stage 32: editor UX

Plan: [README.md](README.md). Report: `docs/part5/stage-32.md`. Findings: U1 to U3, U5, U12 to U31 of [ux-pass.md](ux-pass.md).

Goal: no key or click of the editor does something that the user did not ask for, and the work of each minute needs fewer steps.
Gate: each finding of the stage is fixed, or the report gives the reason why not. A hand session of one hour makes a room with five props, two lights, and three events from "New room", and the report lists each place where the user stopped.

## Context in the code

The table of each finding in [ux-pass.md](ux-pass.md) names the file and the line at commit `baf8b7a`. Stages 30 and 31 moved code: find each place again before a change.

What the stage builds on:
- Key chords and the binding table (stage 30): a new key of the editor is a row of `BINDINGS` (`crates/editor/src/lib.rs`), not a test of a modifier.
- The panels, the status bar, and the picker with a search field (stage 31).
- `Command` and `Editor::run` (`lib.rs`): each change of the user is a command of the queue. A new one (frame the selection, cancel a drag) goes the same way.
- `History` (`crates/editor/src/ops.rs:526`) holds documents. `History::track` makes a step when the document changed and no drag or text field is busy.
- `Drag` (`crates/editor/src/gizmo.rs`) changes the document in each frame of a drag.
- `pick` and `View` (`crates/editor/src/view.rs`): the item of a pixel.
- `EventUi` (`crates/editor/src/event_panel.rs:30`): `row` is one selected row, `clipboard` a list of commands.
- `RoomTarget::text` (`crates/scene/src/room.rs:453`): the loader reads this text and not the file.

## Prompt to paste

```text
Stage 32: editor UX.
Read docs/part5/stage-32-editor-ux.md and docs/part5/ux-pass.md first.

Step 1: a hand session of 20 minutes in the editor as it is. Confirm
each finding U1 to U3, U5, U12 to U31, or mark it as wrong. Add what the
session finds. Report the list before the fixes, in the same table form.

Step 2: the fixes, in this order.

Traps
- U1: Delete, copy, paste, and duplicate go to the list that had the
  last click: the items or the command rows.
- U2: "Save as" asks before it writes over a file.
- U3: Play and Test ask no question. While the editor plays, a load of
  the path of the open document gets the text of the document.
- U5: the question for changes that are not saved is modal. Enter is the
  first answer, Esc is "Cancel".

Picture
- U12: F frames the selection. A double click on an outliner row does
  the same.
- U13: the handle and the item under the pointer show it.
- U14: Esc in a drag puts the values of the drag start back, and the
  history has no step. Esc with no drag clears the selection.
- U15: F5 plays and stops.
- U16: undo and redo put the selection back.

Outliner
- U17: a context menu on a row (Rename, Duplicate, Delete, Frame),
  rename in the row with F2, a filter field. A new item gets a name that
  the room does not have.
- U18: an event with a problem is red, with the problem as hover text.
  The count in the status bar opens a list; a click on a row of it goes
  to the event, the page, and the command.
- U19: a click on a place in "Where used" goes there, if it is in the
  open room.

Properties
- U20: a grid of two columns; X, Y, Z marks in the colors of the gizmo;
  units m, deg, s.
- U21: a texture and a glTF file are picked from the list of the files.
- U22: the doorway of a door has the label of the outliner.
- U23: one group for the cone of a light.
- U24: "Duplicate" for a second item and a second page. "Copy" only for
  the buffer.

Event panel
- U26: the explanation of the star is its hover text.
- U27: a line shows where a dragged row goes.
- U28: Shift+click selects a range of rows in one list. Copy, delete,
  and drag take the range.
- U29: a command set can give one line of help for each command. The
  "Add" list shows it as hover text.
- U30: a row of a built-in command is text for a reader. A command set
  can give the row text of its commands; without it the row is the RON
  text.
- U31: the hover text of a button has the key of its action, from the
  binding table.

Out of scope: what ux-pass.md lists under "Not planned"; new handles of
the gizmo (stage 35); pictures in the file picker (stage 37).

Done when: the gate of the stage file holds; a test for U1, U2, U3, U14,
U16, and U28; bench.py same trip_720p is byte-equal against the run
before. Stop and report, with the list of step 1 and the list of the
last session.
```

## What to watch for

- **U1 is a rule, not a case.** Try each key with a command row selected: Delete, Ctrl+D, Ctrl+C, Ctrl+V, Ctrl+Z. Then click an item in the picture and try them again.
- **U3 and the file on disk.** Play with a change that is not saved, walk to room B and back: room A is the room of the editor, not the file. After stop, the document still has its change and its mark.
- **U13 cost.** A pick in each frame reads each triangle of the room on the CPU (`pick` in `view.rs`). Ask for the time in a room with a model file. A pick only when the pointer moved is the cheap form.
- **U14 and the history.** A drag that Esc ends leaves no step: undo after it goes to the change before the drag.
- **U29, U30 and rule 5.** The help and the row text come from the command set of the module. `events` and `editor` get no line for `ShakeCamera`.
- **U30 and the checks.** A `Problem` finds its row by the row text today (`Problem::row` holds the page and the text). A new row text must not break the red mark: ask how a problem names its row after the change.
- **Too much at one time.** 23 findings. If the report says that some are not done, that is correct behavior: the traps and the picture group come first.
- **The hand session is the gate.** Do one yourself. A place where you reach for the RON file is a finding.
