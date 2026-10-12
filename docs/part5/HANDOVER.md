# Handover: part 5

Written 2026-10-12, at commit `baf8b7a` (stage 28). For the session that does the stages.

## State

- The plan of part 5 is written and not committed: `docs/part5/` (this file, `README.md`, `ux-pass.md`, the stage files 29 to 32) and `docs/part4/MISSING.md`.
- No code of part 5 exists. `CLAUDE.md` still has the "Current target" block of part 4.
- The stage files of stages 33 to 40 do not exist. `README.md` section 1 has their goal, gate, items, and needs.
- The decisions of `README.md` section 2 are proposals that the user did not change. Take them as given.
- The UX pass is from a read of the code and of the pictures. The editor was not run for it.

## Task

The user's instruction of 2026-10-12: do the stages one after the other; after each one a sanity check, fill the gaps, fix the bugs, push, then the next stage.

This changes one line of the working agreement in `CLAUDE.md` for part 5: the report of a stage is still written, but the session does not stop and wait after it. All other rules of `CLAUDE.md` hold (architecture rules, no branch, commit on `main`).

## First steps

1. Replace the "Current target" block of `CLAUDE.md` with the block in `README.md` section 3.
2. Commit the plan and that edit on `main`, and push.
3. `python tools/bench.py run --label p5base`: the base that stage 29 compares against.

## Loop for each stage, 29 to 40

1. **Stage file.** For stages 29 to 32 it exists. For a later stage, write `stage-NN-<name>.md` first, in the form of the others (context in the code with file and line, prompt, what to watch for), from `README.md` section 1, the item in `../part4/MISSING.md`, and the code as it is then.
2. **Build** what the prompt of the stage file says. Only that stage.
3. **Sanity check.** All of:
   - `cargo test`, `cargo clippy` clean, with and without `--features editor`.
   - A validation run with no error (`MAKI_LOG=debug`, `--bench-frames 300`), and `--bench-script trip` for the room loop.
   - `python tools/bench.py run --label sNN`, `compare` against the stage before; `bench.py same trip_720p` where the stage must not change the picture.
   - Each line of "Done when" of the prompt, by a test or by hand.
   - Each point of "What to watch for" of the stage file, with its result.
   - For a stage that changes the editor: run it and use it (the memory `reference-verify-running-game` has the method). Close the editor before `cargo build`.
4. **Gaps and bugs.** What the check finds is fixed in the same stage, then the check runs again. A thing that cannot be fixed in the stage goes into the report and, if it stays true, into `OPEN_ITEMS.md`.
5. **Report**: `docs/part5/stage-NN.md` in the five-point form of `CLAUDE.md` (built, files, deviations, decisions, uncertain), plus the numbers and the results of "What to watch for". Pictures in `docs/part5/pictures/`.
6. **Open items**: update the rows that `README.md` section 4 names for the stage.
7. **Commit** on `main` as `Stage NN: <name>` and **push**.
8. Next stage.

## When to stop and ask

- A gate that cannot be met, or a seam of an earlier stage that is wrong for a later one (fix the seam first; if the fix changes a decision of `README.md` section 2, ask).
- A GPU time or picture regression that is the code and not the machine. To tell them apart: build the commit before in the scratchpad and run the two binaries back to back (memory `reference-verify-running-game`).
- A new dependency that the plan does not name. Stage 34 names `kira`: check its current version and API in the docs before use.
- An action that is not a commit and a push on `main`.

## Known risks

- **Machine noise.** GPU medians of one binary moved up to 18 % in half an hour (`docs/part4/stage-28.md`, section 8). A step in `compare` is not proof of a regression.
- **Stage 29, the line of a command.** A check runs on parsed values with no position; the gate asks for the line.
- **Stage 30, the config file.** The workspace has `toml`, which drops comments on a write. Only a table whose module asks is written; `[render]` must not reach `config.toml` from a debug panel.
- **Stage 31, the benchmark.** The picture size seam must leave the game as it is: the benchmark runs with no editor.
- **Stage 32, 23 findings.** The traps (U1, U2, U3, U5) come first. Line numbers in `ux-pass.md` are of `baf8b7a`.
- **Editor tests write files.** "New room" and "Save" use `game/assets/rooms`: use a copy of the room in the scratchpad, and delete an `untitled.ron` after a test.
