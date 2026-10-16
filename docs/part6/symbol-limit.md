# The symbol limit of the engine DLL

State: done. Ticket #30, spec #23. Plan: `README.md` 2.15 ("First"). Work outside the stages.

## 1. What was built

- **The setting**: `opt-level = 2` for each engine crate in the profile `dynamic` (root `Cargo.toml`, one `[profile.dynamic.package.<name>]` table for each crate of the engine DLL). `package."*"` names only the crates of others, so each engine crate has its own table. The game, the script crate, its DLL, and the editor program stay at 0.
- **The count tool**: `tools/symbol_count.py` reads the export directory of a PE file (`NumberOfNames`, offset 24) and prints CSV: file, count, limit, margin. With no argument it counts `target/dynamic/maki_engine_dylib.dll`; it fails above the margin 60,000, and prints a skip line when the DLL is not there. `selftest` counts `kernel32.dll` (a number between 1,000 and the limit).
- **The check**: `tools/check.sh` runs the selftest in the step "tools" and the count in the step "symbol count", after "build dynamic". CI runs `check.sh`, which builds the dynamic profile, so CI has the count too.

## 2. Results

Same machine, same worktree, one run each. Clean build "before": no `target/`; "after": `target/dynamic` deleted. Engine change: one string in `crates/core/src/config.rs` (the lowest crate: all engine crates build again). Script change: the string of `game/scripts/src/shake.rs` that the reload test changes. Each change was put back. Times in seconds, of `cargo build --profile dynamic -p maki-editor-app -p maki-game-dll`.

```csv
measure,before,after
exported_names,61823,16460
clean_build_s,155,215
engine_change_s,13,15
engine_revert_s,13,14
script_change_s,2,3
script_revert_s,2,3
```

| Check | Result |
|---|---|
| Editor and script DLL link | Yes |
| `cargo test --profile dynamic -p maki-editor-app -p maki-game-dll` | Passes (the loader test) |
| `python tools/reload_test.py` | Passes: load 1 has the new text, the build of the change 2.47 s |
| `sh tools/check.sh` | All steps pass |

The gain: 45,363 names (73 %). 49,075 names are left below the limit.

## 3. Deviations from the spec

- The test of the count tool counts a system DLL (`kernel32.dll`), not a DLL that a test builds: it is on each Windows machine and on the CI runner, and needs no build.
- `check.sh` does not skip in CI: CI builds the dynamic profile in `check.sh` already, so the count runs there. The skip line stays for a run of the tool with no build.

## 4. Decisions where the spec was silent

- The margin stays 60,000 (the spec): far above the count now, and 5,535 below the limit, the step of about two large stages.
- `maki-macros` stays at 0: it is a proc macro, code of the build, not in the DLL.

## 5. Uncertain

- One run of each time, with other cargo processes of other sessions on the machine: a difference of 1 or 2 s is noise. The clean build is 60 s longer (39 %); the rebuilds are about equal.
- A debugger shows less in the engine crates of the editor build (optimized code). The static build (`cargo build`, the dev profile) is not changed, so a debug session can use it.
- The cause of the gain is very likely the sharing of generic functions (`-Zshare-generics`), which rustc turns on at `opt-level` 0 and 1 only: at 2, a crate that uses a generic function of the engine makes its own copy, and the engine DLL does not export it. Not checked against the rustc source.
- The frame time of the editor with the faster engine code was not measured.
