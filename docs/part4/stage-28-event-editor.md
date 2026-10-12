# Stage 28: event editor

Plan: [README.md](README.md). Report: `docs/part4/stage-28.md`.

Goal: events are made and changed in the editor, in the RPG Maker model that the engine already uses.
Gate: the part 2 vignette can be rebuilt from an empty room in the editor.

## Context in the code

### The event format (`crates/events/src/model.rs`)

- `EventDef`: `name`, `area` (rectangle on the floor, default: none), `pages`.
- `Page`: `conditions`, `trigger`, `commands`. The active page is the last page whose conditions all hold (`active_page`). An event with no active page does nothing.
- `Condition` has four kinds: `Flag(name)`, `NotFlag(name)`, `VariableAtLeast(name, value)`, `VariableEquals(name, value)`. There is no general comparison.
- `Trigger`: `Interact`, `Touch`, `Autorun`, `Parallel`. `Autorun` runs one time when its page becomes active, not again and again as in RPG Maker (`OPEN_ITEMS.md`, "Autorun runs one time").
- The 14 built-in commands: `SetFlag(name, bool)`, `SetVariable(name, i32)`, `AddVariable(name, i32)`, `Wait(seconds)`, `If(flag, then, else)`, `OpenDoor(name)`, `CloseDoor(name)`, `LightOn(name)`, `LightOff(name)`, `LightIntensity(name, value)`, `Message(text)`, `FadeOut(seconds)`, `FadeIn(seconds)`, `Transfer(room file, spawn)`.
- `If` tests one flag only. It holds two command lists.
- `OpenDoor` and `CloseDoor` take the name of a door or of a mover.
- `Transfer` names a room file relative to the room directory, and a spawn of that room.
- After stage 24, commands come from a registry, and `ShakeCamera` is a command of `game/`.

### Checks

- At load: the events must parse (`validate_events` through `RoomValidators`). An unknown command or a bad argument is an error with the line.
- Not checked at load: a door, mover, or light name that the room does not have (warning at run time), a `Transfer` to a file or spawn that does not exist (error at run time), a flag that nothing sets, two events with one name.

### What exists

- The game state panel (from `game/src/events_ui.rs`, moved in stage 24): a checkbox for each flag, a number for each variable, and the list of running events with the current command.
- The message window is a debug window (`MessageBox`).
- The interpreter (`interpreter.rs`): `EventRunner` and `RunningEvent` hold the place of each running event. There is no pause and no step.
- No OS clipboard in egui (`OPEN_ITEMS.md`, "egui features that are off").

### The vignette to rebuild

| Room | Event | Pages |
|---|---|---|
| `room_a.ron` | `locked door` | Interact: message. With `power_off`: Interact, the door opens. |
| | `to room b` | Touch: fade out, `Transfer("room_b.ron", "west")`, fade in. |
| | `flicker` | With `power_off`: Parallel, the lamp flickers. |
| `room_b.ron` | `switch` | Interact: set `power_off`, `LightOff("lamp_b")`, message. With `power_off`: Interact, message. |
| | `lamp is dead` | With `power_off`: Autorun, `LightOff("lamp_b")`. No area. |
| | `to room a` | Touch: fade out, `Transfer("room_a.ron", "east")`, fade in. |

`TRIP_SCRIPT` (`trip_720p`) walks this loop by frame numbers. A rebuilt room must keep the places of the spawns, the switch, the door, and the passages, or the script must change (`OPEN_ITEMS.md`, "Benchmark scripts and room files"). `room_c.ron` (materials) is not part of the vignette.

## Prompt to paste

```text
Stage 28: event editor.

Goal: events are made and changed in the editor, in the RPG Maker model
the engine already uses: an event has pages; a page has conditions, a
trigger, and a list of commands. Read
docs/part4/stage-28-event-editor.md first.

Event panel
- Select an event (outliner or its area in the viewport) to open it. Edit
  its name and area (also by gizmo, from stage 27). An event can have no
  area (Autorun, Parallel).
- Pages as tabs: add, delete, duplicate, reorder. The page that would be
  active with the current game state is marked (active_page: the last page
  whose conditions hold).
- Conditions as rows. The format has four kinds (Flag, NotFlag,
  VariableAtLeast, VariableEquals): a row is a kind, a name, and a value.
  Do not add kinds in this stage; list in the report what the editor shows
  is missing.
- Trigger as a choice (Interact, Touch, Autorun, Parallel), with a note
  that Autorun runs one time.
- Commands as a list of rows, one line of text each, like RPG Maker: add,
  delete, reorder by drag, copy and paste (a buffer of the editor, also
  between events and rooms), indent for the two branches of If. Selecting a
  row opens its arguments.

Command arguments
- Each command kind has an argument editor. Built-in commands get proper
  widgets: names of doors, movers, and lights pick from the room; flags and
  variables pick from a list of every name used in all room files of the
  game (with "new"); Transfer picks a room file and then one of its spawns;
  numbers have sensible ranges.
- Commands registered by a game (stage 24) get an argument editor from the
  game if it gives one, and otherwise a RON text field validated as you
  type. ShakeCamera from stage 24 must be editable.

Validation and references
- The events are validated as they are edited, with the rules of the room
  loader, and with the checks that the loader does not make: a door, mover,
  or light name that the room does not have, a Transfer to a room file or a
  spawn that does not exist, two events with one name. Errors show on the
  row; a room with errors can be saved but the editor warns.
- A "where used" view for a flag or variable: every room, event, page, and
  row that reads or writes it.
- The game state panel exists (flags, variables, running events). In the
  editor it sets flags and variables before play, to test a page that needs
  a later state. Stop gives back the values of before play (stage 26).

Test run
- Run one event from the editor (in play mode, with the chosen game
  state), and step through its commands one by one with the current command
  shown. The interpreter has no pause or step today: add it as a seam that
  costs nothing in the game.

Out of scope: a visual graph editor, scripting languages, dialogue trees
with choices (list them as later work in OPEN_ITEMS.md if the format needs
a change for them).

Done when the part 2 vignette (room A: locked door, to room b, flicker;
room B: switch with the power_off flag, lamp is dead, to room a) is
rebuilt from empty rooms using the editor only, saved, and plays the same
in the game without the editor; the trip_720p script still fits (the
debug log of the script shows the same events as before); validation is
clean. Update OPEN_ITEMS.md ("Events with the same name", "Autorun runs
one time", "Benchmark scripts and room files"). Stop and report, including
anything about the event format that made the editor hard to build.
```

## What to watch for

- **The format fights the editor.** If the report says something like "If branches are hard to edit as rows", or "If cannot test a variable", the event format should change before more content depends on it. A change now is cheaper than after ten rooms.
- **Flag names as free text.** Typos in flag names are the classic RPG Maker bug. The picker and "where used" prevent them: try to break them. `power_off` is used in two room files.
- **Checks that only the editor makes.** A room file that a person edits by hand still loads with a wrong door name. If the new checks are useful, they belong in the loader too: ask for that as a follow-up.
- **The rebuild is the real gate.** Do it yourself in the editor, with no RON file open. Anything that forces you back into a text editor is a gap to list.
- **Step mode in the game.** A build without `editor` must run events as before: same frame counts in `trip_720p`.
