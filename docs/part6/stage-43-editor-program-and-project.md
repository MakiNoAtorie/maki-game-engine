# Stage 43: the editor program, and the game as a project

Plan: [README.md](README.md). Report: `docs/part6/stage-43.md`. Needs: stage 41 ([stage-41.md](stage-41.md)) and stage 42 (the shell).

Goal: the editor is a program with no code of a game. It opens a project: a folder with assets and one Rust crate. The game that ships is another program, with no editor.
Gate: `maki-editor game` shows room A, and "Play" runs the trip with the shake command, which no engine crate has. The exported game gives the picture of `trip_720p` of stage 40, byte-equal, at its GPU times. `spike/` is gone.

Not in this stage: the load of a changed script DLL while the editor runs (stage 44), the object model (README 2.1).

State: proposal. The names and the items marked "proposal" are open for the owner.

Written from the code at `1c3158a`.

## Context in the code

- `game/src/main.rs` is the only program. It has the module list of the engine (lines 75 to 131), the constants of this game (`START_ROOM`, `START_SPAWN`, `SAVE_FILE`, `ROOMS`, lines 20 to 28), the command line (`--editor`, `--editor-resave`, the options of the benchmark), and the log setup.
- The code of this game: `game/src/shake.rs` (one event command) and `game/src/scripts.rs` (the three benchmark scripts, which name places of rooms A and B). `maki_devtools::BenchModule` takes the scripts as a field (`crates/devtools/src/bench.rs:71`).
- The editor is the crate `maki-editor`, behind the feature `editor` of the game. `EditorModule { rooms }` takes the room directory.
- Stage 41: `spike/engine` (the engine DLL and `load_project`), `spike/scripts`, `spike/host`, the feature `dynamic` of the game, the profile `dynamic`.
- What stage 41 found (its section 3): the symbol limit; a script crate that does not name the engine DLL gets a copy of the engine with no error; the package set of a build must not change; the DLL of the standard library is not next to the program.
- Paths of rooms are relative to the working directory (`game/assets/rooms/room_a.ron`); a model, a texture, and a sound are relative to the room file.
- `tools/bench.py:234` and `:305`, `tools/shot.py:43`, `run.bat`, and `README.md` name `maki-game` and its build command.

## Prompt

```text
Stage 43: the editor program, and the game as a project.
Read docs/part6/README.md, docs/part6/stage-41.md, and
docs/part6/stage-43-editor-program-and-project.md first.

CLAUDE.md
- New "Current target" block for part 6, the new crates in "Workspace",
  and rule 8 in new words: game content (rooms, objects, prefabs) is
  loaded at run time; the Rust code of a project is in its script
  crate; neither needs a new build of the editor.

The engine as one crate
- `maki-engine` (crates/engine), the last engine crate: it has the
  module list of a game and of the editor, the command line, the log
  setup, and the two entry functions (run a game, run the editor).
  A project names only this crate.
- `maki-engine-dylib`: the DLL of stage 41. The feature `dynamic` of
  `maki-engine` links it. A script crate cannot leave it out.
- The loader refuses a DLL that has its own copy of the engine, and a
  DLL of another build of the engine, with a message. No call into it.

The project
- `game/` is the project: `project.toml`, `assets/`, `scripts/`.
- `project.toml`: name, room directory, start room and spawn point,
  save file. No constant of this game is left in an engine crate.
- `game/scripts`: a crate with `shake.rs`, the benchmark scripts, and
  the entry function. The scripts of the benchmark are added by the
  project (a registry), not given to a module as a field.

The two programs
- `maki-editor PROJECT`: engine and editor. It builds the script DLL
  of the project with the fixed build command, and loads it at start.
  A build error of the script crate: the editor starts, shows the
  error, and the rooms open with no command of the project.
- The game: a program of the project, engine and script crate linked
  statically, no editor, no DLL. `tools/bench.py` and `tools/shot.py`
  build and run it.
- `maki-game --editor` and the features `editor` and `dynamic` of the
  game are gone. `spike/` and `tools/spike_reload.py` are gone; the
  reload test of stage 41 stays as a test of the loader.
- `run.bat` starts the game; `editor.bat` starts the editor. Each sets
  what the program needs to find its DLLs.

Done when
- `editor.bat` shows room A of the project; "Play" runs the trip
  script with the shake.
- The game: `bench.py run`, `compare` against s40, and
  `bench.py same trip_720p`.
- `cargo test` and `cargo clippy` clean for the two builds.
- A validation run of each program with no error.
- README.md has the new commands.
```

## What to watch for

1. **GPU time and picture of the game.** The game is a static program as today; only the place of the code changes. `bench.py same trip_720p` must say ok. A difference is a bug of the move.
2. **The symbol count** of the engine DLL after the move (51,784 of 65,535 in stage 41). Write it in the report. Past 60,000: split the DLL in this stage.
3. **One build command.** The editor and the DLL of the project come from one `cargo` call with one package set. Check: a build of the project while the editor runs does not try to write the engine DLL.
4. **The script crate as a workspace member.** A project outside the repository of the engine is not in this stage. Say in the report what it would need.
5. **The benchmark in the editor program.** The benchmark and its scripts run in the game. Decide if the editor needs them at all (proposal: no).
6. **Tests that used `shake::ShakeModule`** in `game/src/main.rs` (the editor writes each room file with the same content): they move with the code, they are not dropped.
7. **Paths.** A room path in `project.toml` is relative to the project folder, not to the working directory. `bench.py` and the memory note on verification use the old paths.
8. **The debug build of the game.** `cargo run -p <game>` in the dev profile must still work with validation, as today.

## Proposals for the owner

| Item | Proposal |
|---|---|
| Names of the programs | `maki-editor`, and `maki-game` for the game of this repository |
| Name of the project file | `project.toml` |
| Where the project is | `game/` in this repository, a member of the workspace |
| How the editor gets its project | First argument; `game` if none |
| Who starts the build of the script DLL | The editor, at its start; stage 44 adds the build after a change |

## After this stage

The stage table is in `README.md`, section 3.1.
