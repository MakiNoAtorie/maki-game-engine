# Stage 65: the gamepad

Plan: [README.md](README.md), 3.2 item 10 ("Buttons and sticks as keys of the binding tables"), 2.15 ("Dependencies": `gilrs` (gamepad); "All before the slice"; "Movement"; "Menus"). Report: `docs/part6/stage-65.md`. Needs: stage 61 (the grid walk), stage 59 (the menus).

Goal: a gamepad plays the game. Its buttons and the directions of its sticks are inputs of the binding tables beside the keys; each module gives its actions their default pad inputs with `add_bindings`, as it gives keys. A stick gives a direction with a magnitude: the free walk walks slower with a small tilt; the grid walk takes the dominant axis. The menus of the game move by the pad through the actions. A pad plugged in or out while the game runs is found or released.
Gate: a test in code: a pad button press starts its action and its release ends it; a stick under the dead zone starts no action, past it starts the action with a value from 0 to 1; half a tilt walks the free walk at half the speed; on the grid a diagonal tilt moves one cell along its dominant axis; a menu moves down and picks by the pad; a lost pad releases its held inputs. The game plays as before with keys: the trip run, `bench.py gate`, the GPU time against `s64`.

State: written by the session from the plan and the code at commit `05f84bd`; a proposal: taken as given if the owner does not change it.

## Now (the code at stage 64)

- The inputs (`crates/platform/src/input.rs`): `Key` is a table (`keys!`): a name, its winit key codes, a label ("W", "Ctrl", "Left mouse button"); `Key::from_label` reads a label, `is_pointer` tells a mouse input. `Chord` is a key with modifiers. `Binding { chord, action, description, mode }`; `Bindings(Vec<Binding>)` is the one table that the input system and the help panel read. `add_bindings(app, &[(key, action, text)])` from `Module::build`.
- `RawInput` (`KeyDown`, `KeyUp`, `FocusLost`, `PointerMoved`, `PointerLeft`) comes from the window driver (`window.rs`, `window_event`). `update_actions` (schedule `Input`) makes `ActionState` (pressed, just pressed, just released: on or off, no value) and `FixedActionState` for `FixedUpdate`. `ScriptedActions` holds actions down for a benchmark or a test. `InputCapture` (keyboard, pointer) stops inputs that the UI has.
- The move (`crates/gameplay/src/controls.rs`): `MOVE_FORWARD/BACK/LEFT/RIGHT`, `RUN` on W/A/S/D, the arrows, Shift. `move_direction(actions, camera)` adds the pairs of opposite actions (-1, 0, 1) on the camera axes along the floor and normalizes: length 1 or 0. `light_control.rs` uses it too.
- The walk (`player.rs`, `steer_player`): free: direction from `move_direction`, speed walk or run. Grid: `along_axis` takes the dominant axis of the same direction (Z on a tie), and `grid_step` moves cell to cell.
- The menus (`crates/events/src/menu.rs`, `MenuInput::of`) and the choices of a message (`builtin.rs`) read `CHOICE_UP`, `CHOICE_DOWN`, `INTERACT`, `BACK` (`events/src/lib.rs`: Z/Enter, Up/W, Down/S, X/Backspace). egui draws them; it does not move their focus. `PAUSE` (`game.rs`) is P.
- The help panel (`crates/ui/src/help.rs`) lists `binding.chord.label()` and the description of each binding of the mode.
- The input script (`crates/devtools/src/input_script.rs`): `press`, `release`, `tap` take `Key` labels joined by `+`, written as `RawInput`.
- The editor: its mode has its own bindings (camera, chords); in play (F5) the game mode is in use. No gamepad crate is in the workspace (`Cargo.toml`); `gilrs` 0.11.2 is the current version.

## The way

- **Pad inputs are keys.** New rows of the `keys!` table, with no winit code: the buttons ("Pad A", "Pad B", "Pad X", "Pad Y", "Pad LB", "Pad RB", "Pad LT", "Pad RT", "Pad Start", "Pad Select", "Pad L3", "Pad R3", "Pad up/down/left/right" for the D-pad) and the 8 directions of the two sticks ("Left stick up", ..., "Right stick right"). Names of the Xbox layout, with "Pad" so that "A" stays the key A. `Key::is_pad`. So `Chord`, `Bindings`, `add_bindings`, the help panel, and `from_label` take them with no new API.
- **The axes.** `RawInput::PadAxis(Stick, Axis, f32)` (one axis, x or y, of the left or right stick, from -1 to 1) and `RawInput::PadLost` (all pad inputs up, as `FocusLost`). `update_actions` turns a stick into its 4 directions: a direction is down when its part is past the dead zone, with the value `(v - dead) / (1 - dead)`. Triggers are buttons (down past half).
- **The value of an action.** `ActionState::value(action) -> f32`: the largest value of the inputs that hold it; 1 for a key, a pad button, or a scripted action; 0 when not pressed. `FixedActionState` has it too. `pressed` stays as it is.
- **The dead zone.** `[gamepad] dead_zone = 0.2` (a `Settings` table of `PlatformModule`), for each axis. ponytail: per axis, not radial; radial if a stick feels square.
- **The source.** `gilrs` (named in the plan, 2.15), in a new `crates/platform/src/gamepad.rs`, owned by the window driver (`run`), polled before each `app.update()`; it writes `RawInput` as the keys do. A headless app (tests, the bench) has no source. Default features (Windows Gaming Input: events only while a window of the process has the focus; the game has one). gilrs filters off (`with_default_filters(false)`): the dead zone is the engine's, so a test sees it. If `Gilrs::new` fails: a warning, the keys only.
- **One pad.** The first connected pad is player 1; the others are ignored. Hot-plug: on `Disconnected` of player 1, `PadLost`, and the next connected pad becomes player 1.
- **The analog walk.** `move_direction` sums the values (not on or off) and clamps the length to 1: a stick at half gives 0.5, keys give 1. `steer_player` free: direction normalized, speed times the length. Grid: `along_axis` as now (the dominant axis). Running is `RUN`, a button.
- **The default pad bindings** (each module in its own table): `ControlsModule`: left stick and D-pad to `MOVE_*`, Pad X to `RUN`; `EventsModule`: Pad A to `INTERACT`, Pad B to `BACK`, D-pad and left stick up/down to `CHOICE_UP/DOWN`; `GameModule`: Pad Start to `PAUSE`. No pad binding for quit and the debug actions. The editor mode gets none.
- **The input script.** `press Pad A` works by the label; a new step `stick left|right X Y` writes `PadAxis` (0 0 releases).
- **Not in this stage.** Rumble (the plan does not ask it); a screen of the game to change the bindings (the plan names the binding tables only); more than one player; the pad in egui panels and in the editor mode; glyphs (pictures) of the buttons.

## Tickets

| Ticket | Blocked by |
|---|---|
| A. Pad inputs as keys, `PadAxis`/`PadLost`, the dead zone, `ActionState::value` (prefactor) | none |
| B. The gamepad source on `gilrs`: player 1, hot-plug | A |
| C. The analog walk and the default pad bindings of the modules | A |
| D. The `stick` step of the input script, a test room run by the pad | A |

B, C, D are parallel lanes after A. Files: A `crates/platform/src/input.rs`, `lib.rs`; B `crates/platform/src/gamepad.rs` (new), `window.rs`, `Cargo.toml`, `crates/platform/Cargo.toml`; C `crates/gameplay/src/controls.rs`, `player.rs`, `game.rs`, `crates/events/src/lib.rs`; D `crates/devtools/src/input_script.rs`. B adds `mod gamepad;` in `lib.rs` after A is merged.

## Done when

- The gate.
- Tests: a pad label reads and gives its label back; a pad button press starts its action, `PadLost` ends it; a stick at 0.1 starts nothing, at 0.6 starts its direction with the value 0.5 (dead zone 0.2); a key and a stick on one action: the value is the largest; `FixedActionState::value`; `move_direction` of half a tilt has length 0.5, of a key and a tilt at once at most 1; the free walk at half a tilt walks at half the speed; on the grid a tilt of (0.7, 0.4) moves one cell along X; a menu moves down and picks with Pad down and Pad A; the input script parses `stick left 0.5 0` and `press Pad A`; the help panel lists the pad rows.
- Tests and clippy, static and dynamic; a validation run; the trip run; `bench.py gate`; the benchmark against `s64`; the symbol count (`gilrs` is in the engine DLL); the reload test.
- A run by hand, by the input script: the test room walked by the stick (free and grid), a message answered, the pause menu opened and moved by the pad. And, if the owner has a pad, a run with it: plug in and out while the game runs.

## What to watch for

1. **Rule 4**: no gameplay code reads a pad or an axis; it reads actions and their values.
2. **Rule 5**: each module adds its own pad rows; `platform` knows no action.
3. **The keys stay as they are**: a key gives the value 1, so the free walk and the bench paths do not change. `bench.py gate`.
4. **Drift**: a stick at rest is under the dead zone; no action starts and the log of action presses stays quiet.
5. **Focus**: WGI gives no events without the focus; on `FocusLost` the pad inputs are released too, as the keys.
6. **The symbol count and the build time**: `gilrs` brings the `windows` crate; count after the build.
7. **The CI runner** has no pad: no test needs a real pad; `Gilrs::new` is only in `run`.
