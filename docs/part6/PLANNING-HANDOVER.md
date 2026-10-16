# Planning handover: the gaps after the object model

Written 2026-10-16, after stage 53 (the object model of stages 47 to 53 is done). For the session that plans the stages after it. It plans; it does not build.

## Start here

1. `CLAUDE.md`, then `docs/part6/README.md` sections 2 (the decisions of the owner) and 3 (the stage table, the list of gaps). This file is the map over them; the decisions stay in `README.md`.
2. The reports `stage-47.md` to `stage-53.md`, section 6 ("Uncertain") of each: what the object model left.
3. `OPEN_ITEMS.md`: the limits of the engine that a gap can touch (named below where they do).

## How the owner plans

- A question, an answer with a recommendation and its cost, then "add it to the plan". A thing goes into the plan only after the owner says so. One question at a time; the answer is plain and short.
- The skills of this repository for it: `/grill-with-docs` (the interview; it writes `GLOSSARY.md` and `docs/adr/`), `/to-spec`, `/to-tickets` (GitHub issues with "Blocked by"). Only the owner starts them.
- What the session leaves:
  - Rows of the stage table (`README.md` 3.1), each with its number and the items it has.
  - A stage file for the first stage, in the form of `stage-52-wires.md`. A stage file for a later stage is written when that stage starts (`CLAUDE.md`).
  - GitHub issues, label `ready-for-agent`, each with its "Blocked by"; the edges that let lanes run in parallel.
  - `HANDOVER.md` brought up to date.
- The work goes by a branch and a pull request with auto-merge (`CLAUDE.md`, working agreement).

## Limits that bind each plan

- **The symbol limit of the engine DLL**: 61,315 of 65,535 after stage 52; each stage of the object model added 400 to 1,000. A new crate (physics on Rapier, a UI library, an animation crate) can pass it. The way out in `HANDOVER.md` (`opt-level = 2` for the engine crates in the profile `dynamic`) is not measured; a split into two DLLs changes 2.3. **Plan this first**: a measure of the way out, or a stage for it, before a stage with a large dependency.
- **The GPU budget**: the heavy scene is at 7.3 ms of 8 ms. Each item of the renderer (fog volumes, particles, post effects, decals, more lights, skinned meshes in the TLAS) has a gate on the GPU time and a picture gate (`CLAUDE.md`, `docs/CONVENTIONS.md`).
- **One machine** for the GPU checks: two lanes do not run the benchmark at the same time. CI runs `tools/check.sh` only (Windows, no GPU).
- **The order of the crates** (`CLAUDE.md`, rule 1): a new crate has its place in the chain; `scene`, `events`, `audio`, `gameplay` stay free of `gpu`, `render`, `ui`.
- **A new dependency** needs the owner (`CLAUDE.md`, "Stop and ask"): name each crate in the plan.

## The gaps

Each group: the items, where the owner decided them, what is there now, what an item needs, and the questions that the plan must answer. "Now" is from the code at stage 53.

### A. What the events lack (2.8; the next row of the stage table, "Not planned")

| Gap | Now | Note |
|---|---|---|
| Command lists for many users ("common events") | Each event has its own lists | A list in a file of its own, called from rows and scripts. A command `CallList`, and a script call |
| State of one object ("self switches") | Global flags only | `SetField` (stage 51) and a component of the object can carry it; decide: a flag field of `event`, or a general "object flags" component |
| Flow: "or", "else if", loops, wait for a condition | `If` with all-must-hold conditions, `Wait` in seconds | The command lists of `CommandRegistry::add_lists` take new forms |
| Numbers: random, compare two variables | Add, set, compare with a number | Small; built-in commands |
| Autorun again until the condition changes | One time (`OPEN_ITEMS.md`, "Autorun runs one time") | A trigger mode or a page flag |
| Any object as a target: move, show, hide, animate | Lights, doors, movers by name; `SetField` on a component with a live form | Overlaps the timeline (item 6) for "animate" |
| "Convert to script" (later, if the owner wants it) | Not there | Writes the Rust of a command list |

Questions: one stage or two? Which gaps does a game of the owner need first? Does "animate" wait for the timeline?

### B. Gameplay features (3.2, items 6 to 11)

| # | Item | Now | Needs, and notes |
|---|---|---|---|
| 6 | Timeline: tracks that change any field over time, for a cutscene | Command lists | `SetField` and the live form of a component (stage 51); `PlayState::Cutscene` (stage 53). The step mode of 2.6. Shares the core of keys and tracks with item 9 |
| 7 | UI of the game: menus, inventory, HUD | The text box, debug panels | egui for the game, or a UI of its own: a decision of the owner. Item 11 first if the text is in tables. Title and end screens of `PlayState` (stage 53) |
| 8 | Enemies that move: path search, line of sight | Not there; collision is boxes | Line of sight from the physics crate (2.9). A path search on what grid or mesh: a question |
| 9 | Skinned characters: skeleton and clips from glTF | Not there | 2.6: the step mode, clips on twos and threes, a cut with no blend, and the gate "no smear of the pose before" (denoiser, anti-aliasing). A skinned mesh in the TLAS each frame: GPU time. `OPEN_ITEMS.md`: "Player model from glTF", "Part of a model that moves alone" |
| 10 | Gamepad | Not there | Buttons and sticks as keys of the binding tables (`maki_platform`); a crate for the input (a dependency) |
| 11 | Text in tables (languages) | Text in the room file | A message names a key; a table for each language. Touches each text of a command |

Questions: the order of 6 and 9 (one animation core first?); egui or another UI for the game (7); the grid of the path search (8).

### C. The world (2.9; items 15 to 18)

| # | Item | Now | Needs, and notes |
|---|---|---|---|
| 15 | Physics, a crate on Rapier | Boxes, the walking controller | A new crate and a dependency (the symbol limit). Bodies, joints, ray and shape queries, a character controller, trigger areas. Closes `OPEN_ITEMS.md` "Open door leaf". Each body that moves changes the TLAS: measure a room with many |
| 16 | Particles: small meshes in the traced scene, sprites after the path tracer | Not there | Two kinds, two costs. Item 24 (effect files) builds on it |
| 17 | Fog volumes in the path tracer | Not there | A render stage with a gate on the GPU time |
| 18 | Water: a flat surface with ripples, no simulation | Not there | Normal textures first (`OPEN_ITEMS.md`, "Material textures") |

Questions: does the trigger area of the physics crate take the place of `area` (stage 50), or sit beside it? Is the walking controller kept or made from the character controller?

### D. Lights, effects, post effects (2.10; items 19 to 28)

| # | Item | Needs, and notes |
|---|---|---|
| 19 | Post effects as files of a project (a shader and its settings; `glslc` in the editor) | The first of 20, 21, 22 |
| 20 | Effect stack for a room or an area, as a component; values blend | 19. Scripts and the timeline (6) change it |
| 21 | Stock post effects (film, color, old looks, fade and flash commands) | 19, 20 |
| 22 | Helper images (normals, distance, base color) as inputs of a post effect | For outlines and fog by distance |
| 23 | Load of the shaders of the engine while it runs | Independent |
| 24 | Effect files and the effect editor | 16 |
| 25 | Decals | Blood, cracks, writing; GPU time |
| 26 | Light animation (flicker, pulse, fade; temperature; presets) | Can share the tracks of 6. Today a script does it (`game/scripts/src/beacon.rs`) |
| 27 | More light kinds: rectangle light, a mesh that glows as a light | `OPEN_ITEMS.md`, "Light of a mesh" |
| 28 | Baked lighting as a mode of a room (bounce from a bake) | 2.10 "Baked lighting": probes first; open: a bake for each state an event sets, or live bounce for lights that move. Gate: `relmse` and GPU time against the live room |

Each item has a GPU time gate (7.3 of 8 ms). Questions: which effects the owner wants first; 28 for a budget or as a look.

### E. The editor (items 12 to 14, 2.11, 2.12)

| # | Item | Now | Needs, and notes |
|---|---|---|---|
| 12 | Files with an identity (a move or a new name does not break rooms) | Paths | Before the file operations of 13 |
| 13 | Asset browser and drag and drop | File lists with small pictures | 2.11: part A (panel, search, drag of models and prefabs, drag from Windows, file operations with 12, double click) and part B (textures onto surfaces, sounds, files onto fields) |
| 14 | Values of a game that runs: select an object while it plays, show and change its values | Not there | The inspector reads the document now; this reads the world. The live form of a component (stage 51) reads and writes the world |
| 29 | Log panel | Done in stage 50 | |
| 30 | Command console | Not there | A registry of console commands, as settings and event commands |
| 31 | A panic of a script does not end the editor | Not there | The world of that game is dropped and made again |
| 32 | Log file in the project folder | Not there | Small |
| 33 | Pause and one frame for the game in the editor; "pause at an error" | `PlayState::Pause` is of the game (stage 53); `Time::paused` of the editor | Decide which of the two this is |
| 34 | Debug shapes from a script | Not there | `OPEN_ITEMS.md`, "Graphics pipeline state" |

### F. What the object model left (reports 51 to 53, section 6)

Small items; each can go into the stage that touches its code, or into one stage of its own.

- **Not run by hand**: the event panel on objects, the pickers of `SetField`, the wire tabs, "New script", the log panel. A run by the owner in the editor is the check (`CLAUDE.md`, "a run by hand").
- **`SetField` cannot change a light** (no live form for `light`); the check of `SetField` does not check the field; the components of a prefab instance are not seen by its check.
- **Wires**: the problem of an output has no index of the wire; signals and `entered` start wires outside the state play.
- **`Trigger::Interact` stands in for a running wire**; the output names are text in three places (stage 52 review).
- **No room uses `SetPlayState` or `GiveControl`**; a title is a state with no picture (stage 53).

## Edges between the gaps (a proposal, not decided)

- The symbol limit before 15 (physics) and before any stage with a large crate.
- 6 and 9 share keys, tracks, and the step mode of 2.6: one animation core, then each.
- 15 before 8 (line of sight), and before the trigger areas of 2.9.
- 19 before 20, 21, 22; 16 before 24; 12 before part A of 13.
- 11 before 7 if the UI shows text from tables.
- Group A needs nothing new; group F can ride with the stage that touches each file.

Lanes that can run in parallel (no shared files): an editor lane (12, 13, 30 to 34), a renderer lane (19 to 23, 25, 27; one GPU at a time for the benchmark), a gameplay lane (A, 6, 7, 10, 11). The physics crate and skinned characters touch the renderer and gameplay both: plan them alone.

## Questions for the owner, in order

1. The symbol limit: a measure of `opt-level = 2` first, or another way?
2. What comes first: the events (A), the animation core (6 and 9), physics (15), or the effects (D)?
3. Is the plan made from a game of the owner (a slice that names its needs), or from the list in full?
4. For each group chosen: the questions in its section above.
