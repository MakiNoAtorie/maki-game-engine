# Conventions for agents

Build, test and delegation rules that bind future work. Keep under 8 KB. Add a line only when a rule binds future work; delete lines that stop being true. Facts about this machine (tool paths, key/screenshot method) are in the auto-memory, not here.

## Quiet output
Never pull full logs into context. Filter to errors and failures.
- All checks: `sh tools/check.sh` (format, clippy and tests of the static and the dynamic build, errors only). `sh tools/check.sh fast` is the pre-commit hook (`.githooks`; once per clone: `git config core.hooksPath .githooks`). CI (`.github/workflows/ci.yml`, Windows) runs `tools/check.sh` on each pull request and each push to `main`; the GPU checks stay local. Read a red job with `gh run view --log-failed | rg -e '^(error|FAIL)' -A 5 | head -60`.
- One build: `cargo build 2>&1 | rg -e '^(error|warning)' -A 5 | head -60`. Tests: `cargo test -q` runs the default members; `--workspace` also links the engine DLL in the dev profile, which passes the symbol limit.
- Benchmark: `tools/bench.py` writes CSV. `compare A B --total` gives one row per scenario.
- A step of the GPU time with equal pictures: `bench.py confirm COMMIT SCENARIO...` runs the build of the commit before and this tree back to back (two rounds) and drops its runs. Equal within a few percent: the machine.
- Run by hand with no input of the desktop: `maki-editor game CONFIG --input-script FILE --input-shots DIR` (format in `crates/devtools/src/input_script.rs`, example `tools/editor_scripts/stage54.txt`); read the PNGs.
- Picture gate: `bench.py gate` runs the build of the merge-base with `origin/main` and this tree back to back and compares the pictures (`equal`, `noise`, or `CHANGED`); it drops its runs. A label such as `s52` is a base for the GPU time only: `main` can have other pictures than the run of a label. Run the gate after a change of a room file or of the renderer, and in each stage loop, before the full run (20 minutes). `bench.py drop X` removes a side run.

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
- Parallel implementers may each have a worktree and a branch from the stage branch (owner, 2026-10-15); one implementer works on the stage branch. Two writers never touch the same files. A parallel branch is merged into the stage branch when its checks pass, then its worktree and branch go. Each lane is reviewed (`/code-review`) before or after its merge, in the same stage.
- A lane keeps the stage branch green: it fixes a test outside its scope that its own change breaks.
- A lane in a worktree builds into the `target/` of its worktree; the first build there is cold (about 4 minutes for the dynamic form). `tools/reload_test.py` reads the `target/` of its own checkout, so it runs in a worktree too. GPU tools run in the main checkout.
- In a worktree agent, a Bash command with `$(...)` or `python -c` near `git` is refused: write the commands to a scratchpad script and run it with `sh`.
- A session or subagent whose work ends in its own branch or pull request gets its own worktree (`use_worktree: true`): the main checkout stays on the branch of the orchestrator.
- Implementers run the local checks above, do not edit shared notes, and end their report with at most 10 bullet points of new conventions. The orchestrator folds them into this file.
- Prompt template:

      You are an implementer subagent. Implement <task> of stage <NN>.
      Read: docs/part6/stage-NN.md, docs/CONVENTIONS.md.
      Stay inside <crates>; edit shared files only by adding lines.
      Seams agreed (do not ask the owner): <seams>.
      Local checks: <commands from "Quiet output">. Keep output filtered.
      Report: files changed, status of each "Done when" line, anything
      unverified, at most 10 bullet points of new conventions.

## Engine facts that bind code
- A game or a test app with `PlayerModule` adds `GameModule`: the rules give the controller the control. Find the player with `With<Player>`; move the control only with `give_control`.
- `maki_core::PlayState` is the state of the game, changed along `can_change`; `Time::paused` is of the editor.
- A test that writes a room builds the text with `maki_scene::room_text` and its events with `maki_events::short_events`: the form of the file is in one place. The tests of the format itself (`crates/scene/src/room.rs`) keep their literal texts.
- A command started inside the egui frame (menu, palette, command button) goes through `maki_ui::queue_command`: it runs after `run_ui`, when `Panels` is back in the world.
- A script function called by a `CallScript` row queues its commands in `ScriptContext`; the row runs them as an opened list, as `If` runs a branch.
- Output names of a component are `pub const` on its type (`TriggerArea::USED`); the registration uses its `OUTPUTS`.
- A component that `SetField` can change has a live form (`add_live_component_type` with a `Live` constant that lists the live fields). A live write uses `set_if_neq`.
- The room check reads the component types from a copy in `CommandRegistry`: a check test runs one `app.update()` first.
- An editor-only change of a game module's key goes in a Startup system of the editor that edits `Bindings`.

## Code intelligence
- Use `rust-analyzer-lsp` (`/plugin install rust-analyzer-lsp@claude-plugins-official`, `rust-analyzer` on PATH) in local sessions: lookup by symbol and errors after each edit save build round trips.
- No index tool (Serena, vector index) until searches for the same things repeat.
