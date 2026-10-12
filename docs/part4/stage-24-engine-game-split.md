# Stage 24: engine/game split

Plan: [README.md](README.md). Report: `docs/part4/stage-24.md`.

Goal: a second game could be built on the engine crates with no copy of code from `game/`.
Gate: `game/` holds only content and game-specific code; a game-side command works with no edit of `events`.

## Context in the code

### What is in `game/src` (1824 lines)

| File | Lines | What it is | Where it belongs |
|---|---|---|---|
| `camera.rs` | 125 | One `Camera` entity that follows the `Player`, with the room clamp. Reads 5 config keys (`camera_*`). | Engine. Settings of its own. |
| `player.rs` | 124 | Walk, run, collide (`move_circle`), face the move direction. Constants `PLAYER_RADIUS` 0.3 and `PLAYER_HEIGHT` 1.7. Makes the capsule or loads `player_model_path`. | Controller: engine. Radius, height, model: settings or components. |
| `light.rs` | 102 | Debug control: the light actions move the first light of the room. Constants for speed and intensity steps. | Engine, as a debug module. |
| `controls.rs` | 91 | Quit, who gets the move actions, the debug toggle of the movers, a log line for each action. | Engine (quit, action log); the rest with its debug module. |
| `bench.rs` | 624 | Benchmark runner, its panel, and the scripts `WALK_SCRIPT` and `TRIP_SCRIPT`. Uses `render` (`FrameGraph`, `RenderGpu`) and `ui`. | Runner: engine, right of `ui`. Scripts: `game/`. |
| `debug.rs` | 200 | Renderer debug panels (`DebugView`, tone curve, resolutions). Writes `Config` at run time. | Engine. Not in `render`: `ui` is right of `render`. `StatsPanelModule` in `ui` is the pattern. |
| `events_ui.rs` | 75 | The message window, and the debug panel of the game state (flags, variables, running events). | Engine. Stage 28 uses the game state panel. |
| `rooms.rs` | 37 | The start room of the config, and the reload action. | Engine. |
| `stages.rs` | 195 | Debug list of the room files on the disk, and of the scenes that `tools/bench.py` made in `target/bench`. | List of rooms: engine (stage 26 opens rooms with it). The bench directory: with the benchmark module. |
| `tick_logger.rs` | 20 | A log line for each second of fixed steps. | Engine or delete. |
| `main.rs` | 231 | Command line, log setup, the module list. | Stays. |

### Where the moved code can go

- `Camera`, `Player`, `Light`, `Collider`, `move_circle`, `Mover` are in `scene`. The controller and the light control are ordered against `maki_events::run_events`, and the controller reads `EventRunner` (the player stops while an event has the control). So a crate for them is right of `events`. None of them needs `gpu` or `render`.
- The benchmark runner and the renderer panels need `render` and `ui`. They cannot go in a `gameplay` crate left of `gpu`.

### Config

- `crates/core/src/config.rs`: `Config`, 42 fields, `#[serde(default)]`, `Config::load_or_create`. `config.toml` is not committed: the game writes it on the first run.
- Fields by reader: window 4, render size 3, path tracer 9, denoiser 5, anti-aliasing 2, look 7, camera 5, player 3, start room 2, timestep 2.
- `ToneCurve` is in `core` only because `Config` has it. It belongs to the renderer.
- `tools/bench.py` writes config keys by name (`SCENARIOS`, and `--set KEY=VALUE` of `reference` and `quality`): `samples_per_frame`, `max_bounces`, `start_room`, `camera_follow_time`, and others. New table names change these keys.
- `README.md` has a table of the 42 keys.

### Input actions

- `crates/platform/src/input.rs`: `Action` is a closed enum, 18 values. `Bindings` is a table of `Binding`; the help panel (`crates/ui/src/help.rs`) is made from it. `ScriptedActions` feeds the benchmark scripts.
- `ToggleLightControl`, `ToggleMovers`, `NextStage`, `PreviousStage`, `LightUp` and the other light actions belong to debug modules, not to `platform`.
- Stage 26 needs editor actions with a binding table of their own. That is not possible while the enum is closed.

### Event commands

- `crates/events/src/model.rs`: `Command`, 14 kinds: `SetFlag`, `SetVariable`, `AddVariable`, `Wait`, `If(flag, then, else)`, `OpenDoor`, `CloseDoor`, `LightOn`, `LightOff`, `LightIntensity`, `Message`, `FadeOut`, `FadeIn`, `Transfer(room, spawn)`.
- `crates/events/src/interpreter.rs`: one `match` runs them. `Wait`, `Message`, the fades, the doors, and `Transfer` wait over frames. `If` is handled apart (it holds command lists).
- Room files write a command as a RON enum value: `SetFlag("power_off", true)`. With an open set, the name selects the argument type. The files can stay as they are.
- Load checks: `scene` has `RoomValidators`, a list of `fn(&str) -> Result<(), String>`; `events` adds `validate_events`. A plain `fn` cannot hold a command registry.
- A door or light name that the room does not have is found at run time only (a warning in the log), not at load.
- `TRIP_SCRIPT` presses the switch of room B at frame 350. A `ShakeCamera` in that event moves the camera in measured frames of `trip_720p`.

## Prompt to paste

```text
Stage 24: engine/game split.

Goal: a second game could be built on the engine crates without copying
code from game/. game/ keeps only content, its module list, and code that
is specific to this game. Read docs/part4/stage-24-engine-game-split.md
first: it lists what is in game/src and where the seams are closed today.

1. Generic gameplay leaves game/
- Move the top-down camera follow (with the room clamp), the walking
  character controller (walk, run, collide, face the move direction), and
  the light control into engine crates, as modules with their own
  settings. Nothing moved may keep a constant or an assumption that belongs
  to this game (PLAYER_RADIUS, PLAYER_HEIGHT, room names, spawn names):
  those become settings or components.
- The controller works for any entity with a marker component, not only
  the Player entity, so an NPC can use it later.
- These modules need events (run_events, EventRunner) and not gpu or
  render. If they need a new crate (for example `gameplay` between
  `events` and `gpu`), propose its place in the dependency order in the
  report before you create it, and update rule 1 in CLAUDE.md.
- The benchmark runner and its CLI flags become a module that a game opts
  into. It needs render and ui, so it sits right of ui. WALK_SCRIPT and
  TRIP_SCRIPT (frame numbers that fit the room files) stay in game/ and
  are given to the module.
- The renderer debug panels (game/src/debug.rs) move to the engine. render
  cannot depend on ui: put them where StatsPanelModule is, or say why not.
  Panels about this game stay in game/.
- rooms.rs (start room, reload), the room list of stages.rs, the message
  window and the game state panel of events_ui.rs, and tick_logger.rs: move
  each to the engine or give the reason why it is specific to this game.

2. Config owned by each module
- Replace the single Config struct. core loads the TOML file and keeps the
  raw tables; each module declares its own settings type, its table name,
  and its defaults, and reads its table when it is added. Each module gets
  a typed settings resource: no system reads a value by a string key.
- ToneCurve leaves core and goes to the renderer.
- Errors name the table and the key, like the room file errors name the
  file and the line.
- A missing file is still written with defaults, now from all registered
  modules.
- The existing flat config.toml keeps working, or the stage writes a
  converter (code, not a hand edit). Choose one and say which.
- Settings that change at run time from the debug panels keep working.
- Update the config keys in tools/bench.py (SCENARIOS and --set) and the
  config table in README.md.

3. Input actions owned by each module
- Action is a closed enum in platform. A module registers its actions and
  their default bindings without an edit of platform. platform keeps no
  action that belongs to a debug tool or to this game.
- The help panel still lists each action with its binding. ScriptedActions
  still feeds the benchmark scripts.
- Rule 4 stays: gameplay code reads actions, never raw keys.

4. Extensible event commands
- A game or module registers event commands without editing the events
  crate: a name, an argument type that serde reads from RON, and a handler
  that runs in the interpreter. A handler can finish at once or wait over
  frames (like Wait, Message, FadeOut, OpenDoor, Transfer).
- The 14 built-in commands use the same registration path. If, with its
  two command lists, is the hard case: say how it is registered.
- Validation of room files still finds unknown commands and bad arguments
  at load, with the file and the line. RoomValidators holds plain fn
  pointers today; change it if the registry needs that.
- Room files do not change, or change only in a mechanical way that the
  stage does with code.
- Add one game-side command to prove the path: ShakeCamera(seconds,
  strength), registered in game/, used by an event in room B. The trip
  script presses the switch of room B at frame 350: say what the shake
  does to trip_720p, or use an event that the script does not run.

5. Docs
- Update README.md and CLAUDE.md (crate list, dependency order).
- OPEN_ITEMS.md: update the items this stage touches ("Benchmark scripts
  and room files", "Camera clamp", "List of panels" if a menu is added).

Out of scope: new features, renderer changes, the editor, pointer input
(stage 26).

Done when game/ holds only content, game-specific modules, and main.rs;
ShakeCamera works with no change outside game/ and the room file; the
trip_720p and walk_720p benchmarks give the same numbers as before (within
noise, `tools/bench.py compare`); validation and clippy are clean. In the
report, list what remains in game/src with a reason for each file, and the
line count of game/src before (1824) and after. Stop and report.
```

## What to watch for

- **Moved but not generalized.** A camera module in an engine crate that still reads `PLAYER_RADIUS` or a spawn named `"start"` is the same coupling in a new place. Search the moved code for constants that came from this game. `start_room` and `start_spawn` have defaults in `core` today that name `game/assets/rooms/room_a.ron`.
- **The config split becomes a dynamic bag.** A `HashMap<String, toml::Value>` that each system reads by string is worse than the old struct. Each module gets a typed settings resource.
- **ShakeCamera is the real test.** If it needs a change in `events` or any engine crate, the command seam is not done. The same test for actions: a new action of `game/` must need no edit of `platform`.
- **Benchmark drift.** Compare the rows of `tools/bench_history.csv` before and after. A pure move of code does not change GPU or picture numbers. Check that `bench.py` still sets its config keys: a key that the new loader ignores gives a run with defaults and numbers that look like a regression.
- **System order.** The moved modules are ordered against `run_events`, `move_movers`, and `interpolate_transforms`. A lost `.before` or `.after` shows as a one-frame lag of the camera or the player, not as an error.
