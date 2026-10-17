# Handover: part 6

Written 2026-10-12, at commit `0a371b5`; the state was brought up to date after stage 49 (2026-10-15) and after the plan of 2026-10-16. For the session that goes on with part 6.

## Now (2026-10-16)

- The plan after the object model is decided: `README.md` 2.15 (a vertical slice of the owner, made with the tools that the stages build; all before the slice) and the stage table 3.1 (stages 54 to 65, an editor lane, a renderer lane).
- `GLOSSARY.md` and `docs/adr/` exist (the interview `/grill-with-docs`). ADR 0001: objects are named by a fixed id.
- Stages 54 to 59 and the symbol limit (#23, `symbol-limit.md`) are done.
- Next: stage 60 (physics on Rapier, item 15). The session writes its stage file and proposes the tickets; the owner approves the breakdown.

## State

- Parts 2 to 5 are done (stages up to 40). Part 6 has started.
- Stage 41 (spike) is done and pushed: the Rust code of a project as a DLL that a program loads and loads again. Report: `stage-41.md`.
- The plan of part 6 is `README.md`: the decisions of the owner (section 2, thirteen of them), the stage table (3.1), and the list of what is not there (3.2, items 1 to 34).
- Stage files: `stage-42-editor-shell.md` (approved), `stage-43-editor-program-and-project.md`, `stage-45-asset-reload.md`, `stage-46-source-files.md` (planned, not approved). Stage 44 had no file, and the stages from 47 on have none: the session writes each one.
- Stage 42 (the shell of the editor) is done: docks, the registries of panels and commands, the widgets, the theme. Report: `stage-42.md`.
- Stage 43 (the editor program, and the game as a project) is done: `maki-engine`, the engine DLL, `programs/editor`, the project `game/` with its script crate. Report: `stage-43.md`.
- Stage 44 (a new build of the script DLL while the editor runs) is done: the watch, the build behind the work, the load into a new app in the same window, the panel "Build", "Open in editor". Stage file `stage-44-script-reload.md` (written by the session, not seen by the owner), report `stage-44.md`.
- Stage 45 (a changed asset file shows at once) is done: the watch of the asset files (`maki_assets::AssetWatch`), each store forgets a changed file, the room loads again. Report: `stage-45.md`.
- Stage 46 (source files of other programs) is done: the table of converters in the editor crate (`sources.rs`), `.blend` and `.obj` to `.glb` by Blender with no window, `.psd` to `.png` by own code, one thread that looks at the folder of the room files. Report: `stage-46.md`.
- Stage 47 (fields that describe themselves) is done: the derive `Fields` (`crates/macros`), `maki_core::fields` (the table of the fields of a type, its value as a tree of `serde_json::Value`, the registry `FieldTypes`), the derive on the item kinds of a room, and the inspector of the editor from the table (`crates/editor/src/fields.rs`). The room file did not change. Stage file `stage-47-fields.md` (written by the session), report `stage-47.md`.
- Stage 48 (the room file is a list of objects) is done: GitHub issues #1 to #5, closed. Objects with components as RON text, the registry of component types, the editor on objects only (room, object, event). Report: `stage-48.md`; its sections 3 and 6 list what is open.
- Stage 49 (prefabs with changes, the player as data) is done: issues #6 and #7, closed. Report: `stage-49.md`.
- Stage 50 (script components with hooks, commands as functions, "New script", the log panel) is done: issues #8 to #11. Report: `stage-50.md`.
- Stage 51 (events as components, "Set a field", "Call a script function"): issues #12, #13. Report: `stage-51.md`.
- Stage 53 (who is the player, the states of the game) ran as a parallel lane and was merged before stage 52: issues #15, #16. Report: `stage-53.md`.
- Stage 52 (wires, "Send a signal"): issue #14. Report: `stage-52.md`. The benchmark base is now `s52`. The object model (stages 47 to 53) is done; no ticket is open. The stages after it are planned (see "Now").
- Stage 54 (what the object model left): issues #22, #24 to #29. Report: `stage-54.md`. The base of the GPU time is now `s54`; the pictures go through `bench.py gate`. Its run by hand is open (section 3 of the report).
- Stage 55 (object ids, files with an id): issues #40 to #47. Report: `stage-55.md`. The base of the GPU time is now `s55`. Its run by hand was done by the input script (section 3 of the report).
- Stage 56 (the gaps of the events): issues #50 to #56. Report: `stage-56.md`. The base of the GPU time is now `s56`.
- Stage 57 (data assets): issues #58 to #61. Report: `stage-57.md`. The base of the GPU time is now `s57`.
- Stage 58 (text in tables): issues #63 to #66. Report: `stage-58.md`. The base of the GPU time is now `s58`.
- Stage 59 (the UI of the game): issues #69 to #73. Report: `stage-59.md`. The base of the GPU time is now `s59`. The owner uses the GPU for other work while runs go: judge a GPU step by `bench.py confirm` only.
- `CLAUDE.md` has the "Current target" block of part 6, the workspace of stage 43, rule 8 in its new words, and the working agreement of part 6.
- `spike/` is gone. The profile `dynamic` is the build of the editor.

## What changed in the direction (read this first)

The owner, 2026-10-12: the engine took the workflow of RPG Maker as its design. That was not the intent. The intent is a general engine, with a workflow as in Unity, Godot, and Unreal, in which a game in the style of RPG Maker can be made. The events stay, as a way with no code on top of the general system (`README.md` 2.8). Do not add a feature in the RPG Maker form as the main way to make gameplay.

In short, what `README.md` section 2 decides:

- Objects with components that modules register; a kind is a prefab; no node tree (2.1).
- The editor is a program of its own that opens a project; scripts are Rust in a script crate of the project, loaded as a DLL; a script change does not close the editor (2.2, 2.3).
- No code editor in the engine: VS Code, with glue in the editor (2.4).
- A changed asset file shows with no restart; `.blend`, `.obj`, `.psd` by converters in the editor (2.5).
- Animation with a step mode, for the look of drawn animation; no smear of the denoiser at a pose jump (2.6).
- Templates of "New script" (2.7). Physics on Rapier, particles, fog, no fluid simulation (2.9). Lights, effects, post effects, baked bounce light as a mode of a room (2.10). Asset browser (2.11). Log, console, debug tools (2.12).
- The shell of the editor comes before all of them (2.13).
- How the object model is built: a component in the document is a name and a tree of values, the table of fields from a derive of the engine on `serde` (not `bevy_reflect`), a script as a component with a trait of hooks, wires as in Source. Stages 47 to 53 (2.14).

## Working agreement

The owner, 2026-10-12, for part 6: after each stage, always a sanity check, fill the gaps, fix the bugs, push, and then the next stage. `CLAUDE.md` ("Working agreement") has the loop and the cases to stop and ask. The session does not wait for the owner between two stages.

- A next stage with no stage file (44, and 47 to 53): write the file first. A proposal of a stage file that the owner did not change is taken as given (stages 43, 45, 46).
- The object model was "in discussion" until the owner said "add it to the plan" for 2.14 (2026-10-12). Stages 47 to 53 are planned: no stop between them but the cases of `CLAUDE.md`.

The owner plans by questions and answers: a question, an answer with a recommendation and its cost, then "add it to the plan". A thing goes into the plan only after the owner says so. An answer is plain and short. Questions can come in rounds: all the questions that do not wait on another answer at once.

This repository builds an engine, not a game (`CLAUDE.md`): a stage builds the tools; the owner makes the content of the slice.

## Next: issues #6 to #16, and what can run in parallel

The rest of the plan (stages 49 to 53) is GitHub issues #6 to #16 of `MakiNoAtorie/maki-game-engine`, each with its "Blocked by". The owner, 2026-10-15: branches and worktrees are allowed for parallel work (global rules and `docs/CONVENTIONS.md`); two writers never touch the same files.

The blocking edges:

```
#6 prefab instances -> #7 player prefab -> #15 who is the player
#8 script hooks -> #9 commands as functions -> #12 events as components -> #13 general commands -> #14 wires
#8 -> #10 "New script"      #8 -> #16 states of the game      (#12 needs #8 and #9)
#11 log panel: no edge
```

After stage 50: lane A has #15 left; lane B has #12, #13, #14 (one chain) and #16; lane C is done. The table below is as it was written after stage 49:

| Lane | Tickets, in order | Files it mostly writes |
|---|---|---|
| A: prefabs | #15 (#6 and #7 done) | `crates/scene` (prefab files as objects, instance changes: a new `prefab.rs`), the inspector marks in `crates/editor/src/panels.rs`, make and unpack in `ops.rs`, `crates/gameplay` (player), `game/assets/prefabs/` |
| B: scripts | #8; then #9, #10, #16 in parallel; then #12, #13, #14 | `crates/engine` (loader), a script trait (`crates/scene` or `crates/core`), `game/scripts`; #9 `crates/events`; #10 an editor command and `templates/`; #16 `crates/gameplay` and play/stop of `crates/editor/src/lib.rs` |
| C: log panel | #11 | the log setup of `crates/engine`, a panel in `crates/devtools` (by `maki_ui::add_tool_panel`, not in the editor crate) |

Where the lanes meet, and what to agree before they start:

- **`crates/scene/src/object.rs`**: #6 (the changes of an instance against its prefab, on the tree) and #8 (an "insert" of a component of `bevy_ecs` in `ComponentType`) both change the registry. Agree the fields of `ComponentType` first, or let #8 merge first; #6 keeps its code in `prefab.rs`.
- **The room files**: #7 (the player as a prefab object) and #12 (events into objects) both rewrite `game/assets/rooms/*.ron`. Not at the same time: #12 is late in lane B, so the order comes by itself if lane A is not late.
- **`crates/editor/src/panels.rs`, `lib.rs`**: #6 (marks of changed values), #16 (play and stop). Small, separate places; merge one after the other.
- **The symbols of the engine DLL**: 16,460 of 65,535 (`symbol-limit.md`). The step "symbol count" of `check.sh` counts after each build and fails above 60,000.
- **One machine for the GPU**: benchmark and validation runs one at a time, never two lanes at once. `tools/bench.py run --behind` keeps the window behind the owner's work. The base of the next benchmark is `s49`; a step is compared with the commit before, built in the scratchpad and run back to back (the machine drifts up to 20 % in a day).
- **Reports**: each stage keeps its report (`stage-49.md` ... ). With lanes, a stage is done when its tickets are merged and the stage loop of `CLAUDE.md` passed on the merged branch.

Not parallel: the tickets inside one lane (each needs the one before), and #12 to #14 (one chain).

Open from stage 48 (report, sections 3 and 6): the names of the component types are text (a name constant on each type); the editor keeps the gizmo handles and the "Add" presets of some component types; `test_1080p` 3 % of GPU total between the passes, not explained.

## Facts that cost time to find

- **The engine DLL does not link in the dev profile** (`LNK1189`, 65,535 symbols). Use `--profile dynamic` (all crates of the DLL at `opt-level = 2`). A new crate in the engine can pass the limit: count again (`python tools/symbol_count.py`).
- **The build of the editor**: `cargo build --profile dynamic -p maki-editor-app -p maki-game-dll` (`editor.bat`, `maki_engine::BUILD`). Always the two packages: with another set, cargo can build the engine DLL again, and a program that runs has that file. `cargo test` with the same profile and packages builds only what a test needs.
- **A program of the profile `dynamic` needs the DLL of the standard library** in `PATH`: `<rustc --print sysroot>/lib/rustlib/x86_64-pc-windows-msvc/lib`. `cargo run`, `cargo test`, and `editor.bat` set it. The output is in `target/dynamic`.
- **A crate links the engine DLL only if it names it**: `maki-engine` does (`extern crate maki_engine_dylib as _`, feature `dynamic`). A DLL with its own copy of the engine loads with no error from the OS: the loader finds it by the symbol `MAKI_ENGINE_COPY`.
- **The reload test**: `python tools/reload_test.py`. It changes `game/scripts/src/shake.rs` and puts it back.
- **A load of new code is a new app** (`maki_platform::NextApp`): the old app is dropped, the window stays. What must go on is handed over by the module that asks (`maki_editor::Session`). A resource of a module does not go to the next app by itself.
- **A crate type is of the package**, not of the build: the script crate is a library, and `game/scripts/dll` is its DLL form.
- **The shake script** of the benchmark needs room B: a config with `[rooms] start = "game/assets/rooms/room_b.ron"`, `start_spawn = "west"`, and `[save] enabled = false`.
- **Git Bash has no `bc`**: time with `date +%s%3N`.
- **The memory note `reference-verify-running-game`** has the method to run, drive, and look at the game and the editor on this machine, with the places of the shell of stage 42.
- **The symbol limit, measured** (`symbol-limit.md`): `opt-level = 2` for the engine crates in the profile `dynamic` takes the engine DLL from 61,823 to 16,460 names. Clean build 155 s to 215 s; rebuild after a line of `maki-core` 13 s to 15 s; after a line of a script 2 s to 3 s. Count: `python tools/symbol_count.py` (also a step of `check.sh`, fails above 60,000).
- **Line endings**: git warns "LF will be replaced by CRLF" for new files. It is not an error.

## When to stop and ask

The list is in `CLAUDE.md` (working agreement, "Stop and ask"). Of part 6 only:

- The step "symbol count" fails (above 60,000): the next way out is a split into two DLLs, which changes 2.3.
- A GPU time step with equal pictures: `bench.py confirm` first (`docs/CONVENTIONS.md`); ask only if it stays.
