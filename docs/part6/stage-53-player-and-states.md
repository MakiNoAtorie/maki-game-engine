# Stage 53: who is the player, states of the game

Plan: [README.md](README.md), 2.1, 2.14, items 4 and 5 of 3.2. Report: `docs/part6/stage-53.md`. Needs: stage 49 (the player is a prefab), stage 50 (scripts, commands as functions).

Goal: the thing in the world, what controls it, and the rules of the game are three things, as the pawn, the player controller, and the game mode of Unreal, in a small form. The game has the states title, play, pause, cutscene, and end, and changes between them; rows and scripts change the state.
Gate: a row and a script move the control to another object and change the state of the game; pause stops the simulation; play and stop of the editor still work. The game plays as before: the trip run passes, the benchmark against `s51` shows the same pictures (stage 53 was merged before stage 52: a parallel lane).

State: written by the session from 2.1, 2.14, items 4 and 5 of 3.2, and issues #15 and #16. Not seen by the owner.

## Tickets

| Ticket | Content |
|---|---|
| #15 | The pawn (`maki_scene::Player`, the marker), the controller (`maki_gameplay::Controller`), the rules (`maki_gameplay::GameRules`, the module `GameModule`). The control moves to another object |
| #16 | `maki_core::PlayState`: title, play, pause, cutscene, end, and the changes between them. The event command `SetPlayState`; pause stops the fixed step; play and stop of the editor |

## The way

### The pawn and the controller (#15)

- The pawn is the entity with `maki_scene::Player`. The event interpreter, the scripts, the sound, the camera, the save, and the benchmark find the player by it, and do not change.
- `Controller` (a resource of `maki-gameplay`): `home`, the entity that `PlayerModule` makes from the player prefab, and `active`, true while the move actions steer the pawn. `steer_player` is the controller: it reads `active` and no rule.
- `give_control(world, entity)`: the marker goes to the entity. A target with no `Walker` and `PlayerDef` gets those of the pawn before (what walking needs). The pawn before stands. A room change that removes the pawn, and `ResetPlay`, give the control back to `home`.
- The model reload of the player changes `home` only, not an object that has the control.

### The rules (#15)

- `GameModule` (a module of `maki-gameplay`) has the settings table `[rules]` (`GameRules`): the state at the start of a play (`start`: title or play), and if the pause action works (`pause`).
- The rule of the control is a system of the module, not code in the player: the control is on in the state play while no event blocks the player.

### The states (#16)

- `PlayState` is a resource of `maki-core`, so that each crate can read it (the fixed step of the core, the sound, a later rule of the events). `App::new` adds it; its value at start is play.
- The changes: title to play; play to pause, cutscene, end, title; pause to play, title; cutscene to play, end; end to title. `PlayState::change` refuses another change.
- Pause: no fixed step runs (the loop of `App::advance` reads the state), so the events, the scripts, and the walk stand; the sound is silent. The pause action (P) changes between play and pause in `Update`.
- Title, cutscene, end: the steps run; the pawn does not take the move actions.
- `ResetPlay` sets the start state of the rules: the start of the game, play and stop of the editor, a load of a save. The editor needs no change for this.

### Rows and scripts (#16)

- `GameModule` registers the enum `GameCommand` in `CommandRegistry`: `SetPlayState(PlayState)` and `GiveControl(name)` (the first object with the name; an empty name: `home`).
- A script calls them with `ScriptContext::call`, the same handler as a row.

## Done when

- The gate.
- Tests: the change table of the states; no fixed step in pause, and the steps again after; a row (the text of a command) and a script change the state and refuse a change that is not in the table; the control moves to another object, the object walks with the move actions, the pawn before stands, and `ResetPlay` gives the control back; the pawn does not move in a cutscene or while an event blocks it; `ResetPlay` sets the start state.
- Tests and clippy, static and dynamic; a validation run; the trip run; the benchmark against `s51`; the symbol count of the engine DLL.

## What to watch for

1. **The users of `Player`**: the interpreter, audio, camera, save, bench query `With<Player>`; exactly one entity has it after each change of the control.
2. **A pawn that a room removes**: the control goes back to `home` before `place_player`, or no player is placed.
3. **Pause in the editor**: `Time::paused` is of the editor, the pause state is of the game; stop leaves no pause behind.
4. **The order in `FixedUpdate`**: the rule of the control runs after `run_events` and before `steer_player`.
5. **The interact of the events in a cutscene**: the interpreter is in `crates/events`, which another lane changed in stage 51; after the merge it reads the state: interact starts nothing outside play.
