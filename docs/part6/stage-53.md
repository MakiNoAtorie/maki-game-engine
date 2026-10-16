# Stage 53: who is the player, and the states of the game

State: done. Stage file: [stage-53-player-and-states.md](stage-53-player-and-states.md) (written by the implementer of a parallel lane; the owner did not see it). Plan: `README.md`, 2.1, 2.14, items 4 and 5 of 3.2. Tickets: GitHub issues #15 and #16.

Stage 53 ran in a worktree beside stage 51 (its tickets had no edge to #12 and #13), and was merged after stage 51, before stage 52.

## 1. What was built

- **Three things for the player** (#15), a small form of the pawn, the player controller, and the game mode of Unreal:
  - The pawn is the entity with `maki_scene::Player`; each user of the player (events, scripts, sound, camera, save, benchmark) finds it by that marker, with no change.
  - The controller (`maki_gameplay::Controller`): `home` (the body from the player prefab) and `active`. `steer_player` reads `active` and no rule. `give_control(world, entity)` moves the marker to another object; a target with no `Walker` and `PlayerDef` gets a copy of those of the pawn before. A room change that removes the pawn, and `ResetPlay`, give the control back to `home`.
  - The rules (`GameModule`, settings table `[rules]`, `GameRules`): the state at the start of a play, and if the pause action works. The control is on in the state play while no event blocks the player: a system of the module, not code in the player.
- **The states of the game** (#16): `maki_core::PlayState` (title, play, pause, cutscene, end) with its table of changes (`can_change`). Pause: `App::advance` runs no fixed step (the events, the scripts, the walk stand), the sound is silent. The action "pause" (P) changes between play and pause. `ResetPlay` sets the start state of the rules: the start of the game, play and stop of the editor, a load of a save.
- **Rows and scripts**: the commands `SetPlayState(state)` and `GiveControl(name)` of `GameModule`, through `CommandRegistry`; a script calls them with `ScriptContext::call`.
- **After the merge**: the event interpreter reads the state: outside play a press of interact starts no event, no script hook. A message still closes.

## 2. Results

| Check | Result |
|---|---|
| Gate: the control moves to another object | Test: a script gives the control to a box; the box walks, the pawn before stands; an empty name and `ResetPlay` give it back; one entity has `Player` at each time |
| Gate: the state changes by a row and a script; pause stops the simulation | Tests: the change table; no fixed step in pause and the steps again after; a row (`parse_command("SetPlayState(Cutscene)")`) and a script change the state, a change out of the table is refused with a warning |
| Gate: play and stop of the editor | The editor play test: a pause does not stay after "Stop" |
| Test: interact outside play | `interact_starts_nothing_outside_the_state_play` (events) |
| Trip run (debug, validation layers) | The same events as in stage 51; no error, no warning |
| Tests and clippy | Static and dynamic (`tools/check.sh`, on the merged tree): all pass, clippy clean |
| Symbols of the engine DLL | 60,881 of 65,535 (stage 51: 60,011) |
| Benchmark against `s51` | Run `s53`: pictures of `trip_720p`, `walk_720p`, `test_1080p`, `materials_720p`, `textured_walk_720p` byte-equal to `s51`; GPU total of each scenario within 3 % |

## 3. Deviations from the stage file

- **The base of the benchmark is `s51`**, not `s52`: the lane was merged before stage 52.
- **Pause goes back to play only**, not to a cutscene (`ponytail` note in `PlayState::can_change`): a pause in a cutscene needs the state before the pause.

## 4. Decisions where the stage file was silent

- `PlayState` is in `maki-core`, not in `maki-gameplay`: the fixed step of the core and the sound read it; no second pause flag beside `Time::paused` (that one is of the editor).
- Title, cutscene, end: the fixed steps run; the pawn takes no move action, and (after the merge) interact starts nothing.
- `Controller::active` is false by default: a game with `PlayerModule` and no `GameModule` cannot move, so a missing rules module shows at once. A game program or a test app with `PlayerModule` adds `GameModule`.
- The target of `GiveControl` is the first entity with the name and an `Interpolated` pose.
- With `start = "Title"`, F9 (load) and "Play" of the editor go to the title too: `ResetPlay` sets the start state.

## 5. Files

| File | Role |
|---|---|
| `crates/core/src/play_state.rs` (new), `lib.rs` | `PlayState`, its change table |
| `crates/core/src/app.rs` | The state in `App::new`; no fixed step in pause |
| `crates/audio/src/lib.rs` | Silent in pause |
| `crates/gameplay/src/control.rs` (new) | `Controller`, `give_control`, the control back at `ResetPlay` and after a room change |
| `crates/gameplay/src/game.rs` (new) | `GameModule`: `GameRules`, the pause action, `GameCommand`, the rule of the control |
| `crates/gameplay/src/player.rs`, `lib.rs`, `save.rs` | `steer_player` reads the controller; `home`; exports; the test app |
| `crates/engine/src/lib.rs` | `GameModule` in the module list |
| `crates/editor/src/tests.rs` | The play test checks that no pause stays after "Stop" |
| `crates/events/src/interpreter.rs`, `tests.rs` | Interact starts nothing outside play (after the merge) |
| `docs/part6/stage-53-player-and-states.md` (new) | The stage file |

## 6. Uncertain

- **Not run by hand**: the key P, the camera on a new pawn, a load with `start = "Title"`. The tests have no window.
- **No room has an example** of `SetPlayState` or `GiveControl`; the tests make their own rooms.
- **A title screen** is a state only: no picture of a title, no menu (a later stage of the UI).

## 7. Code review (2026-10-16)

Stage 53 was merged with no review. The `code-review` skill ran afterwards on `30d2b87..cad730e` (Standards and Spec, two agents). Branch `review/stage-53`.

### Fixed, each with a test

| Finding | Axis | Fix | Test |
|---|---|---|---|
| A rename of an object does not change its `GiveControl` rows | Standards (CODING_STANDARDS: a command that names an object follows a rename) | Arm for `GameCommand::GiveControl` in `Document::renamed` | `a_rename_changes_the_events_and_a_name_stays_unique` (4 commands) |
| `SetPlayState(Pause)` with `[rules] pause = false` stops the game for good: no fixed step runs, the action is off | Both | The command refuses `Pause` when the pause action is off, with a warning | `a_row_does_not_pause_a_game_with_no_pause_action` |
| A save point with the control on a room object saves the place of that object; the load puts it on the player body | Spec | The save takes the place of `Controller::home` | `a_save_with_the_control_on_an_object_keeps_the_place_of_the_player` |
| In a pause the picture jumps back up to one step: the accumulator was set to zero, so `alpha` was 0 | Spec | A pause keeps the accumulator (as `Time::paused`); a step that pauses drops only the full steps after it | `no_fixed_step_runs_in_a_pause_and_a_step_that_pauses_is_the_last` checks `alpha` |
| No test of "the pawn does not move while an event blocks it" (Done when) | Spec | – | `the_pawn_stands_while_an_event_blocks_it` |
| `SetPlayState` and the pause action write the change table again | Standards (Duplicated Code) | Both call `PlayState::change` | – |
| Comments of two lines, one that tells the code | Standards (Comments) | One line each, the narration removed | – |

Each new test fails on the code before the fix (checked by hand).

### Not changed

- **P in the edit mode of the editor** sets `Pause`: `ResetPlay` at play and at stop sets the start state, and the edit mode is silent and has no fixed step. No effect.
- **A row of `SetPlayState` runs only through a script call in the test**: a row and `ScriptContext::call` go to the same handler of `CommandRegistry`; the test parses the row.
- **No test of a room change that removes the pawn** (watch point): `control_home_if_lost` is in place; a test is a follow-up.
- **`Controller::active` false with no `GameModule`**, `Controller` added by two modules, `GiveControl("")` for the home body, `named`, no `is_paused()` method: judgement calls of the Standards review. The first is a decision of section 4; the others are small.

### Checks

`tools/check.sh`: all pass, static and dynamic. Trip run (debug, validation layers): the same legs and events as in stage 51, no error, no warning. No benchmark: no change in the render path, and the trip never pauses.
