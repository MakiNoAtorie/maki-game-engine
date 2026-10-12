# Part 4: from renderer to engine

Goal: the code becomes an engine that a game is built on, and the game is built with tools, not by hand edits of RON. At the end of part 4, a room can be laid out, lit, textured, and scripted in an editor, then played from the same window.

Part 3 made the picture. Part 4 does not touch picture quality, except where textures need it.

## How to use these files

- Before stage 24, commit the work tree (section 1) and replace the "Current target" section of `CLAUDE.md` with the block in section 2. The rest of `CLAUDE.md` stays.
- One stage at a time. Each stage file has a "Context in the code" section, a prompt, and a "What to watch for" list. Paste the prompt into a fresh session. `CLAUDE.md` gives the rules and the working agreement.
- After each stage, check the "What to watch for" list before you accept it. Save the report as `docs/part4/stage-NN.md`, as the part 3 reports. Pictures go in `docs/part4/pictures/`.
- If a stage report shows that a seam is wrong, stop and fix the seam before the next stage. Every stage after 24 builds on its split.

## Stages

| Stage | File | Goal | Gate | Needs |
|---|---|---|---|---|
| 24 | [stage-24-engine-game-split.md](stage-24-engine-game-split.md) | Generic modules leave `game/`. Config sections, input actions, and event commands are owned by modules. | `game/` holds only content and game-specific code; a game-side command works with no edit of `events`. | - |
| 25 | [stage-25-textures.md](stage-25-textures.md) | Textures from glTF and room files: base color, emissive, alpha cutout; UVs on generated meshes. | A textured glTF room shows correctly, at rest and in motion, with the measured GPU cost. | 24 (config tables) |
| 26 | [stage-26-editor-shell.md](stage-26-editor-shell.md) | Editor mode, free camera, outliner, property panel, save, play/stop. | A room is changed and saved in the editor and plays the same in the game. | 24 (actions, config), 25 (texture fields in the property panel) |
| 27 | [stage-27-viewport-editing.md](stage-27-viewport-editing.md) | Picking, gizmos, snapping, add/delete/duplicate, undo/redo, overlays. | A room can be laid out with the mouse alone. | 26 |
| 28 | [stage-28-event-editor.md](stage-28-event-editor.md) | Pages, conditions, triggers, commands, name pickers, validation, test run. | The part 2 vignette can be rebuilt from an empty room in the editor. | 24 (command registry), 27 |

Rules for all stages:
- The game runs the same with the editor off. The benchmark scenarios keep their numbers (within noise), and the editor adds no cost to a build without it.
- Room files stay the source of truth (rule 8). The editor edits room data, not the live world.
- Validation clean and `cargo clippy` clean, as before.
- Compare benchmarks with `python tools/bench.py run --label <stage>` and `python tools/bench.py compare <before> <after>`. The rows are in `tools/bench_history.csv`.

## 1. State at the start of part 4

Read of the work tree on 2026-10-12: `44877d8` (`max_bounces is 2 by default`) and changes that are not committed in 6 files (`README.md`, `crates/gpu/src/lib.rs`, `crates/scene/src/room.rs`, `crates/ui/src/lib.rs`, `crates/ui/src/stats.rs`, `game/src/debug.rs`). Those changes add `add_corner` to the UI and remove the models of the old room at a room change. Commit them before stage 24. The `OPEN_ITEMS.md` row "Owner of the meshes of a model" then needs an update.

| Area | State | Problem for an engine |
|---|---|---|
| Crates and rules | 8 crates and `game/`. The dependency order holds, no `ash` outside `gpu`, all `unsafe` in `gpu`, 80 tests. | None. Keep it. |
| Generic gameplay | `game/src` has 11 files, 1824 lines: camera follow, walking controller, light control, controls, benchmark runner, renderer debug panels, room list. The components that they use (`Camera`, `Player`, `Light`, `Collider`) are in `scene`. | A second game copies them. |
| Config | One `Config` struct in `core`, 42 fields: window, render size, path tracer, denoiser, anti-aliasing, look, camera, player, start room, timestep. `ToneCurve` (a renderer type) is in `core` because of it. | Each new module edits `core`. Breaks rule 5. |
| Input actions | `Action` is a closed enum in `platform`, 18 values. Some belong to this game or to debug tools (`ToggleLightControl`, `NextStage`, `ToggleMovers`). | A game or the editor cannot add an action without an edit of `platform`. Breaks rule 5. |
| Pointer input | The action system knows keys and the mouse wheel. No pointer position, no mouse buttons. egui reads the raw window events. | The editor camera and picking need them. |
| Event commands | `Command` is a closed enum in `events`, 14 kinds, run by one `match` in `interpreter.rs`. | A game cannot add a command without an edit of `events`. Breaks rule 5. |
| Room format | A generated box with doorways, and props (generated shapes or glTF files), lights (sphere, cone), doors, movers, spawns, events. `scene` reads all but the events; `events` reads the events from the same text. | Good base for an editor. The three room files hold 29 comment lines of design notes that a save would drop. |
| Textures | None. The glTF loader skips images on purpose (`model.rs`). `Mesh` has no UVs. `Gpu::write_image`, sampled images, and samplers exist in the bindless set (the UI font uses them). Images have one mip level and no sRGB format. | No real art is possible. The target look (Animal Crossing, Let's Go) depends on textures. |
| UI | The picture is not scaled. Panels are a column at the right; the window is wider by the column. Windows float on the picture. A closed panel opens again only from code. | The editor needs panels at two sides, a menu, and a viewport. |
| Authoring | Edit RON, press the reload action (`ReloadRoom`). A debug panel lists the room files (`stages.rs`). | Slow. No placement, no picking, no event tool. |
| GPU time | Budget 8 ms. With the defaults (`max_bounces = 2`): `test_1080p` 4.69 ms, `heavy_mixed_lights8_1080p` 9.34 ms (over the budget, `docs/part3/stage-23.md`). | Stage 25 adds work to the trace pass. Gates compare against the run before the stage, not against 8 ms. |

## 2. New "Current target" block for `CLAUDE.md`

```text
## Current target (part 4: from renderer to engine)
Turn the code into an engine that a game is built on, and build the tools
to make rooms: engine/game split, textures, an editor for rooms and events.
The plan is in `docs/part4/`. Parts 2 (playable vignette) and 3 (picture
quality) are done.
The architecture must let later features plug in without rework (upscaling,
software tracer backend, Metal backend, skinned characters, baked
lighting, audio, save/load). Favor correct seams over feature breadth.
Picture quality and GPU time must not regress: compare `tools/bench.py`
runs before and after each stage.
```

Stage 24 and stage 26 also change the crate list and rule 1 of `CLAUDE.md` (new crates). Each stage does that edit itself.

## 3. Open items that part 4 touches

Rows of `OPEN_ITEMS.md`. A stage that makes a check possible runs it and removes the row.

| Row | Stage | Why |
|---|---|---|
| Benchmark scripts and room files | 24, 28 | The scripts stay in `game/`. A room that the editor changes can break a script. |
| Camera clamp | 24 | The camera module moves and must not keep the room of the config. |
| List of panels | 24 or 26 | The editor has many panels: a menu is necessary. |
| Textures from glTF | 25 | The stage is the check. |
| Single-sided parts of a model | 25 | Camera rays cull back faces. Cutout leaves are single-sided. |
| Material kinds from glTF | 25 | The loader gets textures and the alpha mode. |
| Light of a mesh | 25 | An emissive texture is the same kind of light. |
| Cost of the path state | 25 | UVs and texture reads add to the trace loop. |
| Asynchronous loading | 25 | Image decode adds to the load time. |
| Two cameras | 26 | The editor camera is the second camera. |
| TLAS with no instance | 26 | An empty room in the editor, with no player. |
| BLAS build stops the game | 26 | Each full reload of a room waits for the GPU. |
| egui features that are off | 26, 27, 28 | No clipboard, no user textures, no paint callbacks. |
| Display scale other than 100 % | 26 | The editor has much text. |
| Graphics pipeline state | 27 | A line pass for overlays would be the second graphics state. |
| Room file props from files, Movers of a room file | 27 | The editor adds them to rooms. |
| Events with the same name | 27, 28 | The editor keeps names unique. |
| Autorun runs one time | 28 | The event editor shows the trigger; the difference from RPG Maker shows there. |
| Message window, Game state on disk | After part 4 | Section 4. |

## 4. After part 4

Ranked by what the editor shows is missing, not fixed. Likely candidates:

| Next block | Why it may come next |
|---|---|
| Audio | Horror lives on sound. Events get `PlaySound` and `PlayMusic` through the registry of stage 24; the editor gets a picker. |
| Save and load | `GameState` plus room and player (`OPEN_ITEMS.md`, "Game state on disk"). Necessary before any game longer than a vignette. |
| Styled text box and title screen | The message box is a debug window (`OPEN_ITEMS.md`, "Message window"). |
| Skinned characters | Characters that walk, not slide. A BLAS refit path before `build_tlas`. |
| Normal maps and material textures | Follow-up to stage 25, if the art needs them. |
| Room templates and prefabs | The same furniture group in many rooms. |
| Export build | Release package for each platform, with no editor and with cooked assets. |
