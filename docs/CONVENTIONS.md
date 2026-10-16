# Conventions for agents

Build, test and delegation rules that bind future work. Keep under 8 KB. Add a line only when a rule binds future work; delete lines that stop being true. Facts about this machine (tool paths, key/screenshot method) are in the auto-memory, not here.

## Quiet output
Never pull full logs into context. Filter to errors and failures.
- All checks: `sh tools/check.sh` (format, clippy and tests of the static and the dynamic build, errors only). `sh tools/check.sh fast` is the pre-commit hook (`.githooks`; once per clone: `git config core.hooksPath .githooks`). CI (`.github/workflows/ci.yml`, Windows) runs `tools/check.sh` on each pull request and each push to `main`; the GPU checks stay local. Read a red job with `gh run view --log-failed | rg -e '^(error|FAIL)' -A 5 | head -60`.
- One build: `cargo build 2>&1 | rg -e '^(error|warning)' -A 5 | head -60`. Tests: `cargo test -q` runs the default members; `--workspace` also links the engine DLL in the dev profile, which passes the symbol limit.
- Benchmark: `tools/bench.py` writes CSV. `compare A B --total` gives one row per scenario.
- A step of the GPU time with equal pictures: `bench.py confirm COMMIT SCENARIO...` runs the build of the commit before and this tree back to back (two rounds) and drops its runs. Equal within a few percent: the machine.
- Run by hand with no input of the desktop: `maki-editor game CONFIG --input-script FILE --input-shots DIR` (format in `crates/devtools/src/input_script.rs`, example `tools/editor_scripts/stage54.txt`); read the PNGs.
- Picture gate: `bench.py gate` runs the merge-base with `origin/main` and this tree back to back and compares the pictures (`equal`, `noise`, `CHANGED`); it drops its runs. A label (`s52`) is a base for the GPU time only. Run the gate after a change of a room file or of the renderer, and in each stage loop before the full run (20 minutes). `bench.py drop X` removes a side run.

## Editing files
- A room file in the form of the editor (a test compares them): `MAKI_WRITE_ROOMS=1 cargo test -q -p maki-game-scripts the_editor_writes`, then `git diff game/assets/rooms`.
- Write a file with the Write tool: a Bash heredoc or `printf` turns the two characters of an escape (a backslash and `n`) into a new line, and a heredoc breaks on an odd count of `'`.
- Edit with the Edit tool after a Read of the part. Many edits, also in many files: `python tools/replace.py --script EDITS.py` (`EDITS = [(file, old, new), ...]`, the script written with the Write tool; each old text one time; all or nothing; keeps CRLF).

## Model per task
Decide before each subagent and pass `model` explicitly; state the choice and why in one line.
- Opus: renderer and `gpu` work, shaders, serialization and room format, the script DLL loader, the ECS schedule, cross-crate refactors, failures nobody understands yet.
- Sonnet: a module that follows an existing pattern, editor panels, CLI and config wiring, docs, small fixes.
- Haiku or Explore: read-only searches.

## Subagents
- Parallel implementers each have a worktree and a branch from the stage branch (owner, 2026-10-15). Two writers never touch the same files. A branch is merged into the stage branch when its checks pass, then its worktree and branch go. The stage is reviewed (`/code-review`) after the merges, in the same stage.
- A lane keeps the stage branch green: it fixes a test outside its scope that its own change breaks.
- A lane builds into the `target/` of its worktree (first build cold, about 4 minutes dynamic); `tools/reload_test.py` runs there too. GPU tools run in the main checkout.
- In a worktree agent, a Bash command with `$(...)` or `python -c` near `git` is refused: write the commands to a scratchpad script and run it with `sh`.
- The scratchpad is shared by the lanes: the name of each scratchpad file starts with the lane (`l42_edit.py`).
- A session or subagent whose work ends in its own branch or pull request gets its own worktree (`use_worktree: true`): the main checkout stays on the branch of the orchestrator.
- Implementers run the local checks above, do not edit shared notes, and end their report with at most 10 bullet points of new conventions. The orchestrator folds them into this file.
- A prompt names: the ticket and the stage file, the base commit of the lane, what merged lanes changed, the parallel lanes and their files, the checks, and a report of at most 20 lines (files, each acceptance line, what is not verified, new conventions).

## Engine facts that bind code
- A game or a test app with `PlayerModule` adds `GameModule`: the rules give the controller the control. Find the player with `With<Player>`; move the control only with `give_control`.
- `maki_core::PlayState` is the state of the game, changed along `can_change`; `Time::paused` is of the editor.
- A test room: `maki_scene::room_text` and `maki_events::short_events` (the tests of the format itself keep literal texts).
- A command started inside the egui frame (menu, palette, command button) goes through `maki_ui::queue_command`: it runs after `run_ui`, when `Panels` is back in the world.
- A script function called by a `CallScript` row queues its commands in `ScriptContext`; the row runs them as an opened list, as `If` runs a branch.
- Output names of a component are `pub const` on its type (`TriggerArea::USED`); the registration uses its `OUTPUTS`.
- A component that `SetField` can change has a live form (`add_live_component_type` with a `Live` constant that lists the live fields). A live write uses `set_if_neq`.
- The room check reads the component types from a copy in `CommandRegistry`: a check test runs one `app.update()` first.
- An editor-only change of a game module's key goes in a Startup system of the editor that edits `Bindings`.
- A reference to an object is an `ObjectId` (`Kind::Object`), never a name. Ids start at 1; 0 is "no object" (`GiveControl(0)`: the player; in a state command: the object of the event). The editor makes a reference with `object_ref` or `object_pick`.
- A running event carries the id of its object: `RunningEvent::new(..).of(world, entity)`. A prefab change key is `change_key` (`id/component`); the editor reads a prefab file to edit with `ops::prefab_to_edit`.
- A file that a room names loads by `FileRef::resolve(directory)`. Only `FileId::write` writes an id file, only from `sources::scan`; after a change of the file list, `forget_id_tables()`. Only `ids::correct_paths` changes the file references of a document.
- A command enum of a script crate derives `Fields` and calls `registry.add_fields::<T>()`: its arguments then get widgets with no editor code.
- A paste keeps object ids only for a cut into its own room. `History` keeps the highest `next_id`: an id once given is not given again.
- A headless egui test checks a shown text through the `egui::Shape::Text` galleys of `FullOutput.shapes`.
- A condition form: one `Condition` variant (`#[field(label, pick = "flags"|"variables")]`), its arm in `names()`, and in `conditions_of` if it names an object. A list field is `Vec<T: Fields>`.
- A command reads through `step.state(world)` (`Facts`) and writes with `step.set_variable`, never `GameState`: routine arguments hide game variables. Flow: `Step::jump`; a stack search stops at a routine call frame.
- A command module has its own `add(app, &registry)`; a command with a command list uses `add_lists`.
- A command reaches an object and its children (`tree_entities`). Readers of meshes, lights, colliders, areas filter `Without<Hidden>`.
- Play state goes back on `ResetPlay`; code that sets it at the start runs after that `ResetPlay`, not in `Startup`.
- Lane tests in a shared crate: `src/tests/<lane>.rs`, one `mod` line; temp directories prefixed by the lane.

## Code intelligence
- `rust-analyzer-lsp` in local sessions (lookup by symbol, errors after each edit). No index tool until searches repeat.
