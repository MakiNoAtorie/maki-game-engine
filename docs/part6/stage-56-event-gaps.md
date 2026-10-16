# Stage 56: the gaps of the events

Plan: [README.md](README.md), 2.8 (the table of gaps), 2.15 ("Object state", "Routine", "The gaps of the events (group A)"). Report: `docs/part6/stage-56.md`. Needs: stage 55 (merged).

Goal: the events with no code can make the slice of 2.15: a character whose own state changes after a talk, a chest that stays open over a change of room and in the save, a command list that many events call, "or" and "else if", a loop, "wait until", a random number, an autorun that runs again while its condition holds, and show, hide, and move of any object by its id.
Gate: a test room in code: a chest object with a switch `open` opens by an interact, stays open after a transfer to another room and back, and after a save and a load; a routine with an argument, called from two events and from a script, does the same work; a loop with a break and a "wait until" end; a hidden object is not in the picture and not in the collision, and shows again. The game plays as before: the trip run passes, `bench.py gate` shows the same pictures, the GPU time against `s55` is the same.

State: written by the session from the plan and the code at commit `737d106`. Not seen by the owner.

## Now (the code at stage 55)

- A page has conditions (all must hold) and a trigger (`Interact`, `Touch`, `Autorun`, `Parallel`). `Condition` has three forms: `Flag`, `NotFlag`, `Variable(name, Compare, i32)` (`crates/events/src/model.rs`). No "or", no object state, no compare of two variables.
- `If(conditions, then, else)` is a command; a branch is a list that the interpreter opens (`step.open`). There is no loop, no break, no "wait until". `Wait(seconds)` exists.
- An autorun runs once each time its page becomes the active page (`EventMemory.autorun`, `crates/events/src/interpreter.rs`).
- `GameState` holds flags (`bool`) and variables (`i32`); `SetVariable`, `AddVariable`. No random number. The save (`crates/gameplay/src/save.rs`) holds the room, the pose, the flags, the variables; not the state of objects (an autorun page with a flag re-applies it).
- Commands that reach an object: doors and movers (`OpenDoor`, `CloseDoor`), lights, `SetField` on a live field, `Signal`. No show or hide anywhere (no visibility in the scene or in the extract), no move of an object.
- A script runs a command list (`ScriptContext::run`) and has `state()` for the flags and variables.
- Prefab files are the model of a data file: a reader, a watch (`AssetWatch::set` with a check), a reload on `AssetChanged`.
- The editor draws the conditions by hand (a kind combo and pickers, `event_panel.rs`); a command enum registered with `add_fields` gets its argument widgets from its field table.

## The way

- **Conditions from the field table first.** `Condition` derives `Fields`; the editor draws a condition from its table, as a command of a script crate (stage 55). A new form of a condition is then one variant, with no editor code. This is the prefactor of the stage.
- **Object state.** A component `state` on any object: named switches (`bool`) and numbers (`i32`) with their first values. The values live in the resource `ObjectStates`, keyed by the room file and the `ObjectId`: they stay over a change of room and go into the save (`SaveData` gets them, `#[serde(default)]`, so an old save loads). Conditions `Switch(object, name)`, `NotSwitch(object, name)`, `Number(object, name, Compare, i32)`; commands `SetSwitch`, `SetNumber`, `AddNumber`. An object argument of `ObjectId::default()` (0) is the object of the event ("this object"). `ScriptContext` reads and writes the state of an object by id.
- **Routines.** A file `<name>.routine.ron`: names of arguments (`i32`) and a command list. `CallRoutine(file, arguments)` runs it as an opened list; in the routine, the arguments are variables of the call that hide a `GameState` variable of the same name. Loaded and watched as the prefab files; a missing file or a wrong count of arguments is a problem of the room check. `ScriptContext::routine(file, arguments)` calls it from a script. The editor: "New routine" in the asset folder, and the routine opens in the event panel as a list of rows.
- **Flow.** `Condition::Any(conditions)` ("or"). "Else if" is an `If` alone in the else list of an `If`, which the event panel shows as one "else if" row (no change of the format). `Loop(commands)` runs its list again until a `Break`; `WaitUntil(conditions)` waits until they hold. A loop with no wait in its list stops at the step limit of the interpreter (`MAX_COMMANDS_PER_STEP`), as now, and goes on in the next step.
- **Numbers.** `RandomVariable(name, min, max)` (both included), from a generator in a resource with a seed (a fixed seed in `bench.py` and the tests). The generator is own code (a xorshift of a few lines): the plan names no new dependency; condition `Variables(a, Compare, b)`.
- **Autorun again.** A page flag `repeat` (default `false`): an autorun page with it starts again, one step after its run ends, while it is the active page.
- **Any object as a target.** A component `hidden` (a marker, live): a hidden object is not extracted for the renderer, has no collider, and its area gives no output. `Show(object)`, `Hide(object)`; `MoveObject(object, position, seconds)` moves the pose of a root object to a place in the room in a time (0: at once) and waits. Show, hide, and move are not saved (as the doors now): object state and an autorun page re-apply them.
- **New commands** are in enums of their own with `#[derive(Fields)]` and `add_fields`, so their rows need no editor code; their row texts by `add_texts`.

## Tickets

Proposal; the owner approves the breakdown (`/to-tickets`).

| Ticket | Blocked by |
|---|---|
| A. Conditions from the field table (prefactor) | none |
| B. Object state | A |
| C. Routines | none |
| D. Flow: "or", "else if", loop and break, "wait until" | A |
| E. A random number, a compare of two variables | D |
| F. An autorun that runs again | none |
| G. Show, hide, and move any object | none |

A, C, F, G can start at once. B and D after A; E after D (both add condition forms and commands in the same files).

## Done when

- The gate.
- Tests: each condition form in the editor from the field table; object state through a change of room and a save and load, an old save with no object state loads; "this object"; a routine with arguments from a row and from a script, a missing routine file and a wrong count of arguments in the check, a changed routine file loads again; "or", "else if", a loop with a break, a loop with no wait ends the step and goes on, "wait until"; a random number in its range, the same with the same seed; a compare of two variables; an autorun with `repeat` runs again while its page is active, once without it; a hidden object is not in the extract and not in the collision, and its area gives no output; a move of an object ends at its place and waits.
- Tests and clippy, static and dynamic; a validation run; the trip run; `bench.py gate`; the benchmark against `s55`; the symbol count of the engine DLL (`tools/check.sh`).
- A run by hand in the editor, by the input script: a state component in the inspector, a condition of object state, a routine made by "New routine" and called from a row, an "else if" row, play.

## What to watch for

1. **The pictures**: the extract skips a hidden object only; no room of the benchmark has one. `bench.py gate` shows the same pictures.
2. **The save**: an old save file loads (no object state); the object state of a room that is not loaded stays in the resource.
3. **The script DLL**: `ScriptContext` gets new functions. The reload test passes; the templates of "New script" build.
4. **The step limit**: a loop or a `repeat` autorun with no wait must not hold a frame; the limit of the interpreter stops it in each step.
5. **Ids**: object state is keyed by `ObjectId`, never by a name; an object with no id has no state (the check reports a `state` component on it).
6. **Two lanes in one file**: B, D, and E add condition forms. After A, each adds variants only; E waits for D.
7. **Not in this stage**: animate an object (the timeline, stage 64), "Convert to script", data assets (stage 57), the UI of the game (stage 59).
