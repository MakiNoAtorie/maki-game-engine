# Conventions for agents

Build, test and delegation rules that bind future work. Keep under 8 KB. Add a line only when a rule binds future work; delete lines that stop being true. Facts about this machine (tool paths, key/screenshot method) are in the auto-memory, not here.

## Quiet output
Never pull full logs into context. Filter to errors and failures.
- Build: `cargo build 2>&1 | rg -e '^(error|warning)' -A 5 | head -60`
- Dynamic build: `cargo build --profile dynamic -p maki-editor-app -p maki-game-dll 2>&1 | rg -e '^(error|warning)' -A 5 | head -60`
- Clippy: `cargo clippy --workspace --all-targets 2>&1 | rg -e '^(error|warning)' -A 5 | head -60`
- Tests: `cargo test -q 2>&1 | rg -e 'FAILED|panicked|^error|test result' | head -40` (the default members; `--workspace` also links the engine DLL in the dev profile, which passes the symbol limit)
- Benchmark: `tools/bench.py` writes CSV; read only the summary rows, not the per-frame data.
- No CI in this repository: the local checks above are the gate.

## Model per task
Decide before each subagent and pass `model` explicitly; state the choice and why in one line.
- Opus: renderer and `gpu` work, shaders, serialization and room format, the script DLL loader, the ECS schedule, cross-crate refactors, failures nobody understands yet.
- Sonnet: a module that follows an existing pattern, editor panels, CLI and config wiring, docs, small fixes.
- Haiku or Explore: read-only searches.

## Subagents
- Parallel implementers may each have a worktree and a branch (owner, 2026-10-15); one implementer works on the checked-out branch. Two writers never touch the same files. A parallel branch is merged back when its checks pass, then its worktree and branch go.
- Implementers run the local checks above, do not edit shared notes, and end their report with at most 10 bullet points of new conventions. The orchestrator folds them into this file.
- Prompt template:

      You are an implementer subagent. Implement <task> of stage <NN>.
      Read: docs/part6/stage-NN.md, docs/CONVENTIONS.md.
      Stay inside <crates>; edit shared files only by adding lines.
      Seams agreed (do not ask the owner): <seams>.
      Local checks: <commands from "Quiet output">. Keep output filtered.
      Report: files changed, status of each "Done when" line, anything
      unverified, at most 10 bullet points of new conventions.

## Code intelligence
- Use `rust-analyzer-lsp` (`/plugin install rust-analyzer-lsp@claude-plugins-official`, `rust-analyzer` on PATH) in local sessions: lookup by symbol and errors after each edit save build round trips.
- No index tool (Serena, vector index) until searches for the same things repeat.
