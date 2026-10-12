# Stage 41: spike, the Rust code of a project as a DLL

State: done. The mechanism works on Windows. Not built: the reload in the editor window (section 5). Plan: `README.md`, section 3.1.

## 1. What was built

- **`maki-engine`** (`spike/engine`), a `dylib` with all twelve engine crates. A program and a script DLL that link to it have one copy of each engine crate, of `bevy_ecs`, and of the standard library. It has `load_project`: a copy of the script DLL is loaded and its function `maki_project(&mut App)` is called. The copy is never unloaded.
- **`spike-scripts`** (`spike/scripts`), a `dylib`: the file `game/src/shake.rs` of the game (not a copy), and a module of its own with a command, a resource, and a system.
- **`spike-host`** (`spike/host`), a program with no code of a game and no window: it makes an app from the engine modules and the DLL, shows the row texts of the commands and runs a frame, and does so again after each new build of the DLL.
- **The game with the feature `dynamic`**: `shake.rs` is not compiled in; `ShakeModule` comes from the DLL. With `--features editor,dynamic` this is the editor with the code of the game in a DLL.
- **Profile `dynamic`** (`Cargo.toml`): the dev profile, with the crates of others at `opt-level = 2`. It builds into `target/dynamic`. The normal debug and release builds did not change.
- **`tools/spike_reload.py`**: the reload test.

## 2. Results

| Check | Result |
|---|---|
| The engine DLL links | Yes, with the profile `dynamic`: 51,784 exported symbols of 65,535 |
| The host runs code of the DLL | Row text of `ShakeCamera` and of the spike command (a `TypeId` lookup and a downcast across the DLL edge), a system in `Update`, a log line through the logger of the host |
| A change reaches a program that runs | The host loaded the second build: other text, and a resource of another size (8 and 512 bytes of padding under one type name). The app of the first build still ran the old code |
| Build after a change of the script crate | 1.0 s (two runs: 0.99, 1.04). Static build, stage 40: 2.5 to 5.3 s |
| The game with the DLL, `--bench-frames 400 --bench-script shake`, room B, `MAKI_LOG=debug` | The event "mirror ball" ran with its shake command and its message. No validation error, no error line |
| The editor with the DLL | Starts, shows room B, closes. One error line: the model file is not next to the copy of the room in the scratchpad (not the DLL) |
| Normal build: `cargo test`, with and without `editor` | 148 and 40 passed, 0 failed |
| Normal build: `cargo clippy --all-targets`, with and without `editor`; the spike with `dynamic` | Clean |
| Normal build: `bench.py run --only trip_720p` against `s40` | `gpu_total` 1.912 to 1.921 ms (+0.5 %); `bench.py same trip_720p`: ok |

Sizes: `maki_engine.dll` 47 MB, `spike_scripts.dll` 0.4 MB, `maki-game.exe` 1.3 MB. First build of the profile: about 2 minutes.

## 3. What failed first, and what it means

1. **Symbol limit** (`LNK1189`). In the debug profile the DLL does not link: not with all crates, and not with `core` to `gameplay` only. A crate at `opt-level` 0 or 1 exports each generic function that it made. With the crates of others at 2: 35,149 symbols for `core` to `gameplay`, 51,784 for all. 21 % is left. If the engine grows past the limit: two DLLs (the first number shows that a split works).
2. **A script crate that does not name the engine DLL** (`extern crate maki_engine as _`) builds and loads with no error, and has its own copy of the engine (26 MB, 30,382 symbols). The loader must refuse such a DLL.
3. **The package set of a build must not change.** `cargo build -p spike-scripts` alone gave some crates other features than the build with the host; cargo then built the engine DLL again, and could not write it: the host had the file. The editor must build the project with the same command as its own build.
4. **The DLL of the standard library** is in the directory of the compiler, not next to the program. `cargo run` sets the path; a start by hand, a tool, or a copy of the editor on another machine needs it in `PATH` or next to the program.

## 4. Files

| File | Role |
|---|---|
| `spike/engine/` | The engine DLL and the loader |
| `spike/scripts/` | The script DLL |
| `spike/host/` | The program of the reload test |
| `tools/spike_reload.py` | Changes the script crate while the host runs, builds, shows the result, puts the source back |
| `Cargo.toml` | Members `spike/*`, `default-members` (the spike builds only when named), `libloading`, the profile `dynamic` |
| `game/Cargo.toml`, `game/src/main.rs` | The feature `dynamic` |
| `docs/part6/README.md` | The plan of part 6, with the result of this stage |

## 5. Deviations, decisions, uncertain

Deviations:
- The reload was shown in a program with no window, with a new app for each build. The editor does not do it yet.

Decisions where the plan was silent:
- A build profile of its own, so that the debug build of today stays as it is.
- `libloading` 0.8.9: `ash` has it already, no new crate in the tree.
- The entry function has the Rust ABI: the DLL and the program come from one compiler and one build of the engine.
- The spike is in `spike/`, not in `crates/`: it is not an engine crate, and `cargo build` and `cargo test` do not build it.

Uncertain:
- **The reload in the editor.** It needs a new `App` while the window and the GPU device stay. The spike shows that two builds live in one process; it does not show that step. It is plain Rust (the window and the GPU are engine types), and it is the first work of the next stage.
- **Memory.** Each load keeps its copy (0.4 MB of file now). Not measured over many loads.
- **A crash in script code** ends the editor. Not looked at.
- **No check of the DLL before the call**: a DLL of another compiler or of an older engine build is undefined behavior.
- **Speed of the `dynamic` build** against the debug build: not measured. The editor showed 43 frames per second at 3840 x 2160.
- **Linux and macOS**: not tried. They have no symbol limit and no file lock.
