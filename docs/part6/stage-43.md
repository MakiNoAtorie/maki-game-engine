# Stage 43: the editor program, and the game as a project

State: done. Stage file: [stage-43-editor-program-and-project.md](stage-43-editor-program-and-project.md) (a proposal that the owner did not change: taken as given). Plan: `README.md`, 2.2 and 2.3.

## 1. What was built

- **`maki-engine`** (`crates/engine`), the last engine crate and the only one that a project names: the engine crates by their short names (`maki_engine::core`, `::events`, ...), the project file, the module list of a game and of the editor, the command line, the log setup, and the two entry functions `run_game` and `run_editor`.
- **`maki-engine-dylib`** (`crates/engine/dylib`): all engine crates as one DLL. The feature `dynamic` of `maki-engine` links it, so each program and each DLL that names `maki-engine` takes the engine from it: a script crate cannot leave it out.
- **The loader** (`maki_engine::load_project`) refuses, with a message and with no call into the file: a DLL that is older than the engine DLL, a DLL that does not load, a DLL with its own copy of the engine crates, and a file that is not the DLL of a project.
- **The project** `game/`: `project.toml` (name, room directory, start room and spawn point, save file, the package of the script DLL), `assets/`, the script crate `scripts/` (`shake.rs`, the benchmark scripts, the entry function `add_modules`), and `scripts/dll/` (the script crate as a DLL: one line). No engine crate has a constant of this game.
- **Two programs.** `maki-editor [PROJECT [CONFIG]]` (`programs/editor`): engine and editor, no code of a game. It builds the script DLL of the project with the fixed build command and loads it at its start. `maki-game` (`game/src/main.rs`, three lines): engine and script crate linked statically, no editor, no DLL.
- **The benchmark scripts are a registry** (`maki_devtools::add_bench_script`): the project adds them. `BenchModule` has no script field.
- **Gone**: `spike/`, the features `editor` and `dynamic` of the game, `maki-game --editor`. `tools/spike_reload.py` is `tools/reload_test.py`.
- `run.bat` starts the game, `editor.bat` the editor. `README.md` and `CLAUDE.md` (the workspace, rule 1, rule 8) have the new form.

Picture: [the editor program with the project](pictures/s43_editor.png).

## 2. Results

| Check | Result |
|---|---|
| `editor.bat` | Builds (0.25 s with no change), starts `maki-editor`, the log has "project loaded", room A shows |
| "Play" with the command of the project | The editor with room B: Play, the player walked to the mirror ball by keys, the event ran `ShakeCamera` and its message (log: `event starts event="mirror ball"`). No engine crate has that command |
| The trip script | In the game (dev build, `--bench-frames 900 --bench-script trip`, `MAKI_LOG=debug`): room A, room B, the switch, room A. The editor program has no benchmark (section 4) |
| The game: picture | `bench.py same trip_720p s42 s43`: ok. `same trip_720p s40 s43`: ok (byte-equal to stage 40) |
| The game: GPU time | `gpu_total` of the 26 scenarios against `s42`: -1.1 % to +0.9 %. `trip_720p`: 1.912 ms (s40), 1.924 (s42), 1.917 (s43) |
| `cargo test`, static | 152 passed, 0 failed |
| `cargo test --profile dynamic -p maki-editor-app -p maki-game-dll` | 1 passed (the loader: section 3) |
| `cargo clippy --all-targets`, static and dynamic | Clean |
| Validation run of the game and of the editor (`MAKI_LOG=debug`) | No validation error, no error line |
| `python tools/reload_test.py` | Load 0: "Shake the camera: ...". The source changed, build 1.31 s while the program ran, load 1: "Shake the view: ...". The app of load 0 still has the old text |
| The engine DLL in that build (watch 3) | Not written: its file time did not change |
| Symbols of the engine DLL (watch 2) | 54,858 of 65,535 (stage 42: 54,809). The script DLL: 2.4 MB, 2,245 symbols |
| A build error of the script crate | A broken `shake.rs`: the editor starts, a notice has the error of cargo with the file and the line, room A opens. See section 4 for room B |
| A wrong script name | `--bench-script fly`: the game ends with "no script `fly`. The scripts: walk, trip, shake" |
| `run.bat` | Builds and starts the game |

## 3. The loader test

`programs/editor/tests/loader.rs`, with the DLL of the game of this repository:

1. The DLL of the build is loaded: the app has `ShakeCamera`.
2. A copy with an old file time: refused, "is of another build of the engine".
3. The engine DLL itself (a DLL with the engine crates in it): refused, "has its own copy of the engine".
4. A file that is no DLL: refused, "does not load".

In each refusal the test checks that no code of the file ran (the app has no `ShakeCamera`).

How a copy of the engine is found: `maki_core` has a marked static (`MAKI_ENGINE_COPY`). A DLL that links to the engine DLL does not have that symbol; a DLL with its own engine crates exports it. The loader looks for the symbol and calls nothing.

The check found a real case of point 2: `cargo test` builds only what a test needs, so it built the engine DLL again and left the script DLL of an earlier build. The loader refused it. The test package now names the DLL package as a dev-dependency, and cargo builds it.

The reload over two builds needs a build while a program runs: `tools/reload_test.py` with the program `reload_test` (`programs/editor/src/bin`), the host of the spike.

## 4. Deviations from the stage file

- **The script crate is two packages.** `game/scripts` is the code (a normal library: the game links it, `cargo test` runs its tests). `game/scripts/dll` is the same crate as a DLL (one line: `maki_engine::export_project!`). Cargo has one set of crate types for a package, and a script crate with the type `dylib` would be built as a DLL in the static build too, with its own copy of the engine (and in the dev profile with more symbols than a DLL can have). `project.toml` names the DLL package.
- **"A project names only this crate"** holds for the engine. The script crate also names `bevy_ecs`, `glam`, and `serde`: their derive macros name these crates by their own names.
- **"A DLL of another build of the engine"** is found by the file time against the engine DLL, and by a load that fails (a function that the engine DLL does not have). No build stamp is in the two files: a stamp from a build script does not change when only a dependency changes. `OPEN_ITEMS.md` has the limit.
- **"Play runs the trip script"**: the editor program has no benchmark module (watch 5, the proposal of the stage file), so no script runs there. In the editor, Play was driven by keys to the event with the command of the project. The trip script ran in the game.
- **The editor has no "Bench" tab and no stage list** (the same decision). Stage 42 listed "Bench" among the debug tabs.
- **A build error: a room with a command of the project does not open.** The stage file says "the rooms open with no command of the project". Room A opens. Room B names `ShakeCamera`: it does not read, with the error that each unknown command gives. A form for a command that no module has is not in the events crate (`OPEN_ITEMS.md`).
- **The reload test** changes the row text of the command of the game. The test of the spike also changed the size of a resource; the game has no code made for that.
- **The save file** is `game/save.ron` (a path of `project.toml`, from the project folder), not `save.ron` in the working directory.

## 5. Decisions where the stage file was silent

- The proposals of the stage file were taken: the names `maki-editor` and `maki-game`, `project.toml`, the project in `game/` as a member of the workspace, the project folder as the first argument (`game` if none), the build of the script DLL at the start of the editor.
- **Where the crates are**: `crates/engine` and `crates/engine/dylib`; the editor program in `programs/editor` (the package `maki-editor-app`: the library is `maki-editor`); the program of the game in `game/` (`Cargo.toml`, `src/main.rs`), beside `project.toml`.
- **The default build is the static one.** `default-members` has the engine crates, the game, and its script crate. The editor program, the engine DLL, and the script DLL build only when named, with `--profile dynamic`. In the dev profile the engine DLL does not link (stage 41), so they must not be in `cargo build`.
- **One feature** `dynamic` of `maki-engine`: it links the engine DLL and compiles `run_editor` and the loader. The editor program turns it on; in the build command it then holds for the script DLL too.
- **The paths of a project** start at the working directory when the project folder is in it (`game/assets/rooms/room_a.ron`, as before), with `/` on each system; else they are absolute (the benchmark runs the game in another directory).
- **The game finds its project** by the folder of its program crate (`env!("CARGO_MANIFEST_DIR")`). Export is not in this stage (`OPEN_ITEMS.md`).
- **The entry function** of a project is `fn(&mut App)`. The editor calls it at the place in the module list where the game calls it (after `SaveModule`, before `RenderModule`): the systems have the same order in the two programs.
- **A script name of the command line** is checked after the modules are added (`maki_devtools::check_bench`): the project adds its scripts later than the command line is read.
- `maki-editor --resave ROOM` in place of `maki-game --editor-resave ROOM`. The title of the editor window has the name of the project. `ConfigFile` and the config tables did not change.
- The test "the editor writes each room file with the same content" moved to the script crate, with `maki-editor` as a dev-dependency (watch 6).

## 6. A project outside the repository (watch 4)

Not in this stage. It needs: a workspace of its own with the engine as a dependency (a path or a git address); the editor program as a member of that workspace, or a small program crate of the project that calls `maki_engine::run_editor` (cargo builds the programs of members only); the profile `dynamic` in its `Cargo.toml` (a profile is of the workspace root); a build command that names its packages; and "New project" in the editor to write these files. The engine DLL is then built one time for each project folder (about 2 minutes).

## 7. Files

| File | Role |
|---|---|
| `crates/engine/` (new) | `lib.rs`: the re-exports, the module list, `run_game`, `export_project!`. `project.rs`: `project.toml`. `editor_program.rs`: `run_editor`, the build command. `loader.rs`: `load_project` |
| `crates/engine/dylib/` (new) | The engine DLL |
| `programs/editor/` (new) | `maki-editor`; `src/bin/reload_test.rs`; `tests/loader.rs` |
| `game/project.toml`, `game/Cargo.toml`, `game/src/main.rs` | The project file; the program of the game |
| `game/scripts/` | The script crate: `lib.rs` (entry function, the room file test), `shake.rs`, `bench.rs` (was `game/src/scripts.rs`); `dll/` |
| `crates/devtools/src/bench.rs`, `lib.rs`, `script.rs` | `BenchScripts`, `add_bench_script`, `check_bench`; a run has the name of its script |
| `crates/editor/src/lib.rs`, `tests.rs` | No command line of its own; `EditorModule::project` |
| `crates/core/src/lib.rs` | `MAKI_ENGINE_COPY` |
| `crates/ui/src/widgets.rs` | `Notices::push` |
| `Cargo.toml` | The members, `default-members`, `maki-engine-dylib` |
| `editor.bat` (new), `run.bat`, `.gitignore` | The two start files; `game/save.ron` |
| `tools/reload_test.py` | The reload test, with the project |
| `game/assets/rooms/room_b.ron` | Two comments name the new files |
| `README.md`, `CLAUDE.md`, `OPEN_ITEMS.md`, `docs/part6/HANDOVER.md` | The new commands and the section "Project"; the workspace and rule 8; three rows; the state |
| `spike/` (removed) | |

## 8. Uncertain

- The build of the script DLL at the start of the editor blocks before the window: 0.25 s with no change, 1.3 s after a change of the script crate. A change of an engine crate while the editor runs cannot be built (the engine DLL is in use): the editor shows the error of cargo. Stage 44 moves the build behind the work.
- The file time check of the loader depends on the order in which cargo writes the two DLLs. It held in each build of this stage.
- Windows only: the two `.bat` files, and the DLL names in the loader test.
- No person used the editor program: the session was scripted.
