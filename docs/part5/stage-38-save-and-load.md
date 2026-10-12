# Stage 38: save and load

Plan: [README.md](README.md). Report: `docs/part5/stage-38.md`. Item: M11 of [`../part4/MISSING.md`](../part4/MISSING.md).

Goal: the game writes its state to a file at a save point and starts from that file.
Gate: the game is closed after the switch of room B; the next start is in room B with the lamp off.

Decision of the plan (section 2, M11): no save in the middle of an event. A save point is a command; the save is written when its event ends.

Written after the report of stage 37, from the code at `f2c8cf5`.

## Context in the code

### The state

- `GameState` (`crates/events/src/lib.rs:48`): flags and variables, a resource. No serde derive.
- The room: `CurrentRoom::path` (`crates/scene/src/room.rs`). The player: `Interpolated` on the entity with `Player`.
- The state of a room is not saved: a room file says "the lamp is on", and an `Autorun` page with a flag condition sets it right after each load ("lamp is dead" in `room_b.ron`).
- `Step::memory` of a waiting command is a `Box<dyn Any>` (`crates/events/src/interpreter.rs:63`).
- `Audio::music` (`crates/audio`): the music that plays. `gameplay` does not depend on `audio`.

### The start and the play signal

- `RoomsModule` (`crates/gameplay/src/rooms.rs`) asks for the start room in `Startup` (`request_start_room`). `spawn_player` (`player.rs`) is in `Startup` too.
- `load_requested_room` with `spawn: None` leaves the player where it is. A room file that is bad changes nothing and logs the error.
- `ResetPlay` (`crates/core/src/app.rs:56`): each module sets its play state back. The flags and variables are not a part of it.

### Commands of a module

- `crates/audio/src/commands.rs` and `game/src/shake.rs`: an enum, `CommandRegistry::add`, `add_texts`, `add_defaults`.
- `EventRunner::running` gives the events that run, each with its name.
- A system of `FixedUpdate` declares its order to `run_events`.

### Who must not save

- `tools/bench.py`: the trip script presses the switch in each run. A save file would move the start of the next run.
- The editor plays a room that is not on disk, and its "Stop" brings the document back.
- `game/src/main.rs` has the module list; the test `the_editor_writes_each_room_file_with_the_same_content` has its own.

## Prompt

```text
Stage 38: save and load.
Read docs/part5/stage-38-save-and-load.md and item 11 of
docs/part4/MISSING.md first.

The save file
- A RON file with the room, the pose of the player, and GameState.
  The game names the file; [save] file and [save] enabled in the
  config. The write goes to a second file first, then a rename.

The save point
- A command SavePoint of a module of gameplay. It waits for nothing.
  The file is written in the step after its event ends, with the
  state of that step. With no save file (the editor, a benchmark) the
  command does nothing.

Load
- At the start, a save file that reads and whose room reads takes the
  place of the start room. Load runs ResetPlay, sets GameState, asks
  for the room with no spawn point, and puts the player at its pose.
- A debug key loads the file in the running game.
- A save file that is bad, or that names a room that is bad: an error
  in the log, and the game starts in its start room.

Content
- The switch of room B is a save point.

Out of scope: a save in the middle of an event, more than one save
file, a menu, the state of a room that no flag describes, the music
that plays, a version change of the file.

Done when: a test saves at the end of an event and a second app
starts from the file in that room with the flags and the pose; a test
starts with a bad save file; the game is closed after the switch of
room B and the next start is in room B with the lamp off; trip_720p
is byte-equal with the save off. Update OPEN_ITEMS.md ("Game state on
disk", "Memory of a waiting command"). Report.
```

## What to watch for

- **The benchmark.** `save.enabled = false` in each scenario of `bench.py`; no `save.ron` after a run; `trip_720p` byte-equal against `s37`.
- **The editor.** Play in the editor writes no file and reads none. A room file with `SavePoint` opens, shows its row, and saves with the same text.
- **The state at the write.** A command after `SavePoint` in the same event is in the file.
- **An event that changes the room and saves.** The file names the new room.
- **A crash in the write.** The old file stays whole until the rename.
- **Load in the running game.** An event that runs and a message that shows are gone after the load; the lamp of the room follows the flags.
- **The player.** It stands at the saved place and faces the saved way, with no blend from the old place.
- **A flag of the config or of a debug panel.** The save has the state of `GameState`, whoever wrote it.
