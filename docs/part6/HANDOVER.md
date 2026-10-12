# Handover: part 6

Written 2026-10-12, at commit `0a371b5`; the state was brought up to date after stage 42. For the session that goes on with part 6.

## State

- Parts 2 to 5 are done (stages up to 40). Part 6 has started.
- Stage 41 (spike) is done and pushed: the Rust code of a project as a DLL that a program loads and loads again. Report: `stage-41.md`.
- The plan of part 6 is `README.md`: the decisions of the owner (section 2, thirteen of them), the stage table (3.1), and the list of what is not there (3.2, items 1 to 34).
- Stage files: `stage-42-editor-shell.md` (approved), `stage-43-editor-program-and-project.md`, `stage-45-asset-reload.md`, `stage-46-source-files.md` (planned, not approved). Stage 44 and the stages from 47 on have no file.
- Stage 42 (the shell of the editor) is done: docks, the registries of panels and commands, the widgets, the theme. Report: `stage-42.md`.
- `CLAUDE.md` has the "Current target" block of part 6, the line for `spike/`, and the working agreement of part 6. Rule 8 gets its new words in stage 43.
- The spike code is in the tree and stays until stage 43 removes it: `spike/engine`, `spike/scripts`, `spike/host`, `tools/spike_reload.py`, the feature `dynamic` of the game, the profile `dynamic`.

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

## Working agreement

The owner, 2026-10-12, for part 6: after each stage, always a sanity check, fill the gaps, fix the bugs, push, and then the next stage. `CLAUDE.md` ("Working agreement") has the loop and the cases to stop and ask. The session does not wait for the owner between two stages.

- A next stage with no stage file (44, and 47 on): write the file first. A proposal of a stage file that the owner did not change is taken as given (stages 43, 45, 46).
- The object model (stage 47 and later) is "in discussion" in the stage table: stop there and ask.

The owner plans by questions and answers: a question, an answer with a recommendation and its cost, then "add it to the plan". A thing goes into the plan only after the owner says so. An answer is plain and short.

## Next: stage 43, the editor program and the project

1. The base of the benchmark is the run `s42` (`tools/bench_history.csv`).
2. Read `stage-43-editor-program-and-project.md` and build what its prompt says.
3. The loop of `CLAUDE.md`: sanity check, gaps and bugs, report `stage-43.md`, the rows of `OPEN_ITEMS.md` that the stage closes or adds, commit, push.
4. Stage 44 has no file: write it first. And so on.

## Facts that cost time to find

- **The engine DLL does not link in the dev profile** (`LNK1189`, 65,535 symbols). Use `--profile dynamic` (the crates of others at `opt-level = 2`). All twelve engine crates: 51,784 symbols. A new crate in the engine can pass the limit: count again (stage 42 adds `egui_dock`).
- **Build of the spike**: `cargo build --profile dynamic -p maki-game --features editor,dynamic -p spike-scripts -p spike-host`. Always the same package set: with another set, cargo builds the engine DLL again, and a program that runs has that file.
- **A program of the profile `dynamic` needs the DLL of the standard library** in `PATH`: `<rustc --print sysroot>/lib/rustlib/x86_64-pc-windows-msvc/lib`. `cargo run` sets it. The output is in `target/dynamic`.
- **A script crate must name the engine DLL** (`extern crate maki_engine as _`), or it gets its own copy of the engine with no error.
- **The reload test**: `python tools/spike_reload.py`. It changes `spike/scripts/src/lib.rs` and puts it back.
- **The shake script** of the benchmark needs room B: a config with `[rooms] start = "game/assets/rooms/room_b.ron"`, `start_spawn = "west"`, and `[save] enabled = false`.
- **Git Bash has no `bc`**: time with `date +%s%3N`.
- **The memory note `reference-verify-running-game`** has the method to run, drive, and look at the game and the editor on this machine, with the places of the shell of stage 42.
- **`cargo fmt` formats the files of `spike/` too** (they were never formatted): `git checkout -- spike` after it.
- **The engine DLL after stage 42**: 54,809 symbols of 65,535.
- **Line endings**: git warns "LF will be replaced by CRLF" for new files. It is not an error.

## When to stop and ask

- A gate that cannot be met, or a decision of `README.md` section 2 that the code shows to be wrong.
- A GPU time or picture regression that is the code and not the machine (build the commit before in the scratchpad and run the two back to back).
- A new dependency that the stage file does not name.
- The symbol limit of the engine DLL: a split into two DLLs changes 2.3.
- A stage that the plan marks "in discussion", and an action that is not a commit and a push on `main`.
