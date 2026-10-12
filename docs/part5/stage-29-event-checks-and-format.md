# Stage 29: event checks in the loader, event format

Plan: [README.md](README.md). Report: `docs/part5/stage-29.md`. Items: M1, M2 of [`../part4/MISSING.md`](../part4/MISSING.md), U4 of [ux-pass.md](ux-pass.md).

Goal: a room file that a person wrote gets the checks of the editor at load, and the event format can test a variable in an `If`.
Gate: `OpenDoor("dor")` in `room_a.ron` is an error with the line at load, with the text that the editor shows. An event opens a door when a counter is 3, made in the editor with no text field. The three room files load after the conversion. `trip_720p` is byte-equal.

## Context in the code

### Checks

- `maki_scene::check_room` (`crates/scene/src/room.rs:403`) parses the room, checks the doorway of each door, then runs each `RoomValidator`.
- `RoomValidator` is `Box<dyn Fn(&str) -> Result<(), String>>` (`room.rs:431`): it gets the text of the file, not the `RoomDef`. An error stops the load. There is no warning.
- The events crate adds one validator: `CommandRegistry::validate_events` (`crates/events/src/command.rs:236`). It parses the events and returns `line:column: reason`.
- The editor has the other checks: `maki_editor::problems` (`crates/editor/src/events.rs`). A `Problem` has the event, the row (page and row text), and a text. It reads `Builtin` commands only, and it gets the other room files as `OtherRoom` (file name, spawn points, names used), read at open and save (`Editor::others`, `crates/editor/src/lib.rs:300`).
- `names` (`events.rs`) gives each flag and variable with its uses (`Use::writes`). The "Where used" panel shows the counts (`crates/editor/src/panels.rs:397`).
- The loader runs in a frame of the game (`load_requested_room`, `room.rs:501`), also at a `Transfer`.

### Format

- `Condition` (`crates/events/src/model.rs:73`): `Flag`, `NotFlag`, `VariableAtLeast`, `VariableEquals`. `Condition::holds` reads the `GameState`.
- `Builtin::If(String, Vec<Command>, Vec<Command>)` (`crates/events/src/builtin.rs:28`): one flag.
- `EventDef::area` is an `Area`; size 0 is "no area" (`Area::is_empty`, `model.rs:39`). The editor shows a checkbox for it (`event_panel.rs:324`), and `view.rs:218` and `overlay.rs:151` test the size.
- The editor has a widget for a list of conditions (`conditions`, `event_panel.rs:206`) and one for the flag of an `If` (`arguments`, `event_panel.rs:145`). `edit_list` and `rows` (`events.rs`) match `Builtin::If`.
- `Document::rename_in_events` and `renamed` (`ops.rs`) follow a name through the commands.
- Room files of the game: `game/assets/rooms/room_a.ron`, `room_b.ron`, `room_c.ron`. `tools/convert_config.py` is the pattern of a converter. `--editor-resave` writes a room file as the editor saves it.

## Prompt to paste

```text
Stage 29: event checks in the room loader, and the event format.
Read docs/part5/stage-29-event-checks-and-format.md and items 1 and 2 of
docs/part4/MISSING.md first.

Format (do this first: the checks then read the new format)
- If takes a list of conditions, the same Vec<Condition> as a page (all
  must hold): If(conditions, then, else).
- One condition for a variable: Variable(name, Compare, value), with
  Compare = Equals, NotEquals, Less, AtLeast. It takes the place of
  VariableAtLeast and VariableEquals. Flag and NotFlag stay.
- EventDef::area is Option<Area>. No code tests a size of 0 for "no area".
- A converter for the room files that exist, in tools/, after the pattern
  of tools/convert_config.py. Convert the three room files of the game.
  Their comments stay.
- The editor: the condition widget of a page is the widget of an If. No
  second widget.

Checks
- The checks of maki_editor::problems move into the events crate. The
  editor and the loader call the same code and show the same text.
- A RoomValidator gets the RoomDef and the text. It returns errors and
  warnings. A warning is logged and the room loads.
- Error: a light, door, or mover that the room does not have; two events
  with one name. Warning: a Transfer to a room file or a spawn point that
  does not exist.
- Each message of the loader has the line of the command in the file.
- A module can add a check for its own commands, as it adds the commands
  (rule 5). Give ShakeCamera of game/ a check as the proof, or a test
  command if ShakeCamera has nothing to check.
- "Where used" and the flag picker mark a flag or variable that an event
  reads and no event writes. This check is of the editor: it needs all
  room files.

Out of scope: "or" in conditions, an expression language, Choice (stage
33), a change of Autorun.

Done when: room_a.ron with OpenDoor("dor") gives the error with its line
at load, and the editor shows the same text on the row; an event that
opens a door when a counter is 3 is made in the editor with no text
field; the three room files load; bench.py same trip_720p is byte-equal
against the run before the stage. Update OPEN_ITEMS.md ("Checks that only
the editor makes", "Events with the same name", "Limits of the event
format"). Stop and report.
```

## What to watch for

- **The line of a command.** The parser gives a position for a parse error only. A check runs on values that have no position. Ask how the line is found, and try it with two equal commands in one file.
- **Disk reads in a frame.** The `Transfer` check needs the spawn points of another file. The loader runs at each room change: a read of each neighbor file there is a long frame. The check can run one time for each file, or only where the start room is checked (`check_start_room`).
- **Two copies of the checks.** After the stage, `crates/editor/src/events.rs` must hold no check of a name. Search it for `Builtin::OpenDoor`.
- **The converter and comments.** Compare each room file before and after with `git diff`: only `If`, the variable conditions, and `area` change.
- **`trip_720p`.** The event starts in the debug log of the script are the same as before. A warning at load must not be an error: room B is the neighbor of room A, and the two name each other.
- **A flag with no write that is correct.** A flag that the "Game state" panel sets for a test has no write in a room file. The mark is a hint, not an error.
