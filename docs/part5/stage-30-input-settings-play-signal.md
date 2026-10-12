# Stage 30: key chords, saved settings, play signal

Plan: [README.md](README.md). Report: `docs/part5/stage-30.md`. Items: M4, M5 of [`../part4/MISSING.md`](../part4/MISSING.md).

Goal: three seams below the editor that other modules need too: a binding with modifiers, settings that are written back, and a signal "a game starts".
Gate: the editor has no `MODIFIER` action and no list of the resources of other crates. A changed grid step is there after a restart. A script that presses and moves in one millisecond selects at the press place. A shake that runs at a stop does not go on in the next play.

## Context in the code

### Bindings

- `Binding` (`crates/platform/src/input.rs:139`): `key`, `action`, `description`, `mode`. One key or one mouse button. `Key::Control` and `Key::Shift` are keys as the others.
- A module adds a table of `(Key, Action, &str)` with `add_bindings` or `add_mode_bindings` (`input.rs:172`, `177`).
- The editor binds `Key::Control` to `MODIFIER` and tests it by hand: `Editor::keys` (`crates/editor/src/lib.rs:643`). The tool keys W, E, R test that it is not down (`lib.rs:659`). `MODIFIER` also turns the snap around in a drag (`lib.rs:321`), and `ADD_TO_SELECTION` is Shift.
- The help panel makes its rows from the table: `rows` (`crates/ui/src/help.rs:36`), with `Key::label`.

### Pointer

- `RawInput` (`input.rs:280`): `KeyDown(Key)`, `KeyUp(Key)`, `PointerMoved`, `PointerLeft`. A mouse button is a `KeyDown`.
- `Pointer::position` (`input.rs:253`) is the place in the frame. `Editor::viewport` reads it in the frame where `SELECT` is `just_pressed` (`lib.rs:327`).

### Settings

- `Settings` (`crates/core/src/config.rs:31`): a resource with `Serialize`, a `TABLE` name, and `check`. `App::add_settings` reads the table.
- `ConfigFile` (`config.rs:80`) holds the path and the text. `finish` writes the file only if there was none. The workspace has the crate `toml`, not an editing crate: a plain write drops the comments of the file.
- `EditorSettings` (`lib.rs:123`): `snap`, `grid_step`, `angle_step`. The left panel writes the resource (`panels.rs:116-134`). The debug panels write other settings resources (`[render]`).
- `tools/bench.py` passes a config file to each run.

### Play state

- `reset_play_state` (`lib.rs:690`) sets back `EventRunner`, `MessageBox`, `ScreenFade`, and `MoveCapture`. `Editor::play` and `Editor::stop` call it, and put the `GameState` back.
- `CameraShake` (`game/src/shake.rs`) is state of a game module that the editor does not know.
- The game has no "new game" moment: its state is the default of each resource at the start of the app.

## Prompt to paste

```text
Stage 30: key chords, saved settings, play signal.
Read docs/part5/stage-30-input-settings-play-signal.md and items 4 and 5
of docs/part4/MISSING.md first.

Key chords
- A binding has a key and its modifiers (Ctrl, Shift, Alt). Ctrl+Z and Z
  are two bindings. The tables of the modules that have no chord stay as
  they are.
- A plain key does not start its action while a chord of the same key and
  mode is down (Ctrl+S is not "tool: scale" and not a walk key).
- The help panel shows "Ctrl+Z".
- The editor: no MODIFIER action. Undo, redo, duplicate, save are chords.
  The snap switch in a drag and "add to the selection" stay held keys.

Press place
- The press of a mouse button carries its place. A click of the editor
  uses the place of the press, not that of the frame.

Settings
- A module can ask that its settings table is written back to the config
  file when the resource changes. Only a module that asks is written.
  EditorSettings asks.
- The comments and the other tables of the file stay as they are.
- The write is not in each frame of a drag.

Play signal
- A schedule in core that the caller runs at once: "the play state goes
  back to its start". Each module with play state adds a system to it
  for its own resources.
- The editor runs it on play and stop, and holds no resource of another
  crate in reset_play_state. The game runs it at its start.
- CameraShake of game/ joins it.

Out of scope: a binding editor, save and load (stage 38), panel sizes in
the settings (stage 31 adds them with this seam).

Done when: the gate of the stage file holds, each with a test where a
test can show it; bench.py compare against the run before is within
noise. Update OPEN_ITEMS.md ("Key chords", "Config values at run time",
"Place of the pointer at a click", "Play state of a module"). Stop and
report.
```

## What to watch for

- **Held modifiers in the game.** A walk key with Shift or Ctrl down must walk, unless that mode has a chord of the key. Try W with Ctrl held in the game, and W with Ctrl held in the editor.
- **Key up of a chord.** Ctrl released before Z: the action ends one time, and Z does not start "plain Z". Ask for the test.
- **Which tables are written.** A change in the "Render" debug panel must not reach `config.toml`: the next benchmark run would use it. Check the file after a session with debug panels.
- **Benchmark configs.** `bench.py` runs with its own config files: none is written by a run.
- **The schedule, not a list in another place.** `rg "EventRunner|MessageBox|ScreenFade|MoveCapture" crates/editor/src/lib.rs` has no hit in a reset function.
- **`GameState` at stop.** The editor keeps the state of before play (the "Game state" panel sets flags for a test). The signal must not clear what the user set.
