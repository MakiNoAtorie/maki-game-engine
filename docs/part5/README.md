# Part 5: from editor to game

Goal: the editor is a tool that a person uses for a full day without a text editor beside it, and the engine has what a game longer than a vignette needs: checked content, choices, sound, save and load, prefabs.

Input: [`../part4/MISSING.md`](../part4/MISSING.md) (14 items, cited as M1 to M14) and the UX pass in [ux-pass.md](ux-pass.md) (31 findings, cited as U1 to U31). Part 5 does not touch picture quality.

## How to use these files

- Before stage 29, replace the "Current target" section of `CLAUDE.md` with the block in section 3.
- One stage at a time, as in part 4: paste the prompt of the stage file into a fresh session, check its "What to watch for" list, save the report as `docs/part5/stage-NN.md`, pictures in `docs/part5/pictures/`.
- The stage files of stages 29 to 32 exist. The file of each later stage is written after the report of the stage before it: its "Context in the code" section names code that stages 29 to 32 change. Until then, section 2 and the item in `MISSING.md` are its specification.
- A stage that makes an `OPEN_ITEMS.md` row true or false updates the row.

## 1. Stages

| Stage | File | Goal | Gate | Items | Needs |
|---|---|---|---|---|---|
| 29 | [stage-29-event-checks-and-format.md](stage-29-event-checks-and-format.md) | The loader makes the checks of the editor. `If` takes conditions; a variable is compared with one general condition. | `OpenDoor("dor")` is an error with the line at load, with the text that the editor shows. An event opens a door when a counter is 3, made with no text field. `trip_720p` byte-equal. | M1, M2, U4 | - |
| 30 | [stage-30-input-settings-play-signal.md](stage-30-input-settings-play-signal.md) | Key chords in the binding table, the press place of a click, settings written back, a play signal for modules. | The editor has no `MODIFIER` action and no list of the resources of other crates. A changed grid step is there after a restart. | M4, M5 | - |
| 31 | [stage-31-editor-window.md](stage-31-editor-window.md) | The picture of the editor has the size that the panels leave. Panels with a width that the user sets. The event panel under the picture. Pick lists with a search field. | The editor fits 1920 x 1080 with the default config. A resize changes the view. Pick and gizmo are exact after a resize. Benchmark numbers do not move. | M3, U6 to U11 | 30 (saved panel sizes) |
| 32 | [stage-32-editor-ux.md](stage-32-editor-ux.md) | The traps and the daily friction of the UX pass. | Each finding of the stage is fixed or has its reason. A hand session of one hour makes a room with events and lists what is left. | U1 to U3, U5, U12 to U31 | 30, 31 |
| 33 | later | A styled text box. A `Choice` command with one list for each answer. "A command with lists" is a trait of the command set, not a case for `Builtin::If`. | A question with two answers, made in the editor; each answer runs its list. | M6 | 29 |
| 34 | later | Crate `audio`. Sound assets with handles. `PlaySound`, `PlayMusic`, `StopMusic` from a module. A sound at a place in the room. A file picker in the editor. | The switch of room B clicks; the lamp of room A hums and stops with the power; stop in the editor ends all sound. | M7 | 30 |
| 35 | later | Marks cut by the depth of the picture. A direction handle for a cone light, corner handles for an event area and a doorway, a group turn, the two poses of a mover. | The grid does not show on a prop. A cone light is aimed and an event area is sized with the mouse. | M8 | 31 |
| 36 | later | The loader keeps a mesh that did not change. The BLAS build is recorded in the frame. Model files and textures load on a thread. | A drag of a prop color and of the room width follows the mouse. A model file of 100 000 triangles changes with no long frame. | M9 | - |
| 37 | later | egui user textures. Pickers with a small picture for textures and models. One room of the game with a model from a modelling tool. | A model and a texture are picked by their picture; the model shows in two rooms with one load; a click selects it. | M10 | 36 |
| 38 | later | A save file: room, spawn point or player pose, `GameState`. A save point as an event command. Load uses the play signal. | The game is closed after the switch of room B; the next start is in room B with the lamp off. | M11 | 30 |
| 39 | later | An item buffer between rooms. A prefab file that a room names. | A table with two chairs is in two rooms; one change of it shows in the two. | M12 | - |
| 40 | later | Benchmark scripts name places. Then rooms A and B made again from "New room", by hand. | The switch of room B is moved by 1 m and the trip script still presses it. `trip_720p` byte-equal with the rooms of the session, or each difference has its reason. | M13, M14 | 32 |

Rules for all stages, as in part 4:
- The game runs the same with the editor off. A build without the feature `editor` has no editor code.
- `python tools/bench.py run --label sNN` and `compare` against the stage before. A stage that must not change the picture also runs `bench.py same trip_720p`.
- Validation clean, `cargo clippy` clean.
- Room files stay the source of truth. A format change comes with a converter for the files that exist.

Order: the table follows `MISSING.md` but for two moves. M1 and M2 are one stage (the two change the same condition code, room files, and editor widget). M5 moves to M4 (the two are small seams below the editor). The UX stage is after the window stage: it changes the same panels, and a layout change after it would undo its work.

## 2. Decisions that `MISSING.md` leaves open

Proposals. Change them here before the stage starts.

| Item | Question | Proposal | Why |
|---|---|---|---|
| M1 | Error or warning at load | A name of the room itself (light, door, mover) that does not exist, and two events with one name: error. A `Transfer` to a file or spawn point that does not exist: warning at load, error when it runs, as today. A flag with reads and no write: the editor only. | A room is made before its neighbor. A flag check needs all room files; the loader has one. |
| M2 | How far | `If(conditions, then, else)` with the same `Vec<Condition>` as a page ("all hold"). `Variable(name, Compare, value)` with `Equals`, `NotEquals`, `Less`, `AtLeast` in place of the two variable kinds. No "or", no expressions. | The editor has the widget for a condition list (`conditions` in `event_panel.rs`): `If` uses it as it is. The converter runs one time for the two changes. |
| M2 | `EventDef::area` | `Option<Area>`. | The editor shows a checkbox for a size of 0 today. |
| M3 | Place of the event panel | A panel under the picture, as wide as the picture: pages and conditions at the left, command rows in the middle, arguments at the right. The property column keeps name, area, notes. | A command row is one line of RON text (a `Message` with its full text); the column is 440 pixels. A floating window hides the picture that the area gizmo is on. |
| M5 | Message or schedule | A schedule in `core` that the caller runs at once (`ResetPlay`). | A message arrives one frame later; the editor loads the room in the same frame. |
| M7 | Output crate | `kira`; effects are loaded, music is streamed. Check the current version and its API in the docs before stage 34. | It has tweens, tracks, and streaming; a sound that fades with the power needs them. |
| M11 | Save in the middle of an event | No. A save point is a command; the save is written when its event ends. | `Step::memory` stays a `Box<dyn Any>`. |
| M12 | Copied or named | Named by the room. | The gate of M12 asks for it ("one change shows in the two"). |
| M13 | Places or copies | The script names places. | A copy of the rooms tests rooms that the game does not have. |

Not in part 5: the items that `MISSING.md` leaves out, and what [ux-pass.md](ux-pass.md) lists under "Not planned".

## 3. New "Current target" block for `CLAUDE.md`

```text
## Current target (part 5: from editor to game)
Make the editor a tool for daily work and give the engine what a longer game
needs: checks at load, a richer event format, an editor window that fits the
display, key chords, a play signal, choices, sound, save and load, prefabs.
The plan is in `docs/part5/`. Parts 2 (`docs/part2/`), 3 (`docs/part3/`), and
4 (`docs/part4/`) are done.
The architecture must let later features plug in without rework (upscaling,
software tracer backend, Metal backend, skinned characters, baked lighting).
Favor correct seams over feature breadth.
Picture quality and GPU time must not regress: compare `tools/bench.py`
runs before and after each stage.
```

Stage 34 adds the crate `audio` to the crate list and to rule 1 of `CLAUDE.md`.

## 4. Open items that part 5 closes

| Row of `OPEN_ITEMS.md` | Stage |
|---|---|
| Checks that only the editor makes, Events with the same name, Limits of the event format | 29 |
| Key chords, Config values at run time, Place of the pointer at a click, Play state of a module | 30 |
| Size of the editor window, Display scale other than 100 % | 31 |
| Message window, Dialogue with choices | 33 |
| Marks behind objects, What the gizmo does not change, Graphics pipeline state | 35 |
| BLAS build stops the game, Asynchronous loading | 36 |
| Room file props from files, egui features that are off, Player model from glTF | 37 |
| Game state on disk, Memory of a waiting command | 38 |
| Benchmark scripts and room files | 40 |

"Autorun runs one time" and "Step mode of the events" stay as they are: no item asks for a change.
