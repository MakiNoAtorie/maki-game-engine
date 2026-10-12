# Stage 44: a new build of the script DLL while the editor runs

State: done. Stage file: [stage-44-script-reload.md](stage-44-script-reload.md), written by this session from the plan (`README.md`, 2.3 and 2.4) before the code; the owner did not see it. Each choice of it is in section 5.

## 1. What was built

- **Watch** (`maki_engine`, `reload.rs`): 3 times in a second the editor looks at the file times of the Rust files and the `Cargo.toml` files of the project folder (not `assets/`, not `target/`, not a hidden folder). No new crate.
- **Build**: a change starts the build command of stage 43 on another thread, with `--message-format short`. The status row shows the job (`maki_ui::Jobs` has its first writer). A change while a build runs starts one more build after it.
- **Load**: a build that wrote a new script DLL asks for another app in the same window. `maki_platform::NextApp` is the seam: the runner drops the old app, gives the new one the window, and runs its `Startup`. The new app has the modules of the new copy of the DLL; the old copy stays loaded and is not used.
- **What goes to the new app** (`maki_editor::Session`): the open document as the text of a room file, with its path and its "not saved" mark; the selection; the view of the editor camera; the spawn point of "Play". The layout comes from its file.
- **No load** if the new copy does not load (the checks of the loader), or if the open room does not read with the new code (a command is gone): the old app goes on, and a notice says why. While a game runs, the load waits for "Stop".
- **The panel "Build"** (bottom dock; it opens when a build fails): the state of the last build, and one row for each error and warning with its file and its line. A click on the file runs the command of the code editor (`[editor] code_editor`, default `code -g "{file}:{line}"`).
- **Two commands** in "File": "Build the scripts", "Open the project in the code editor". The module of the engine adds them and the panel with one call each: no change in the editor crate.

## 2. Results

| Check | Result |
|---|---|
| The gate: a saved change shows | Room B with a duplicated prop (not saved) and the event "mirror ball" selected. `shake.rs` changed on disk: the row of the event panel is "Shake the view: 0.6 s, 0.06 m" with no action; the title still has ` *`, the room has its 6 props, the event is still selected |
| The time from the save to the new code | 1.8 s: 0.2 s until the editor sees the file, 1.4 s build, 0.1 s load of the DLL and the modules, 0.2 s `Startup` of the new app (GPU device, pipelines, the meshes of room B). Three more loads in the session: the same |
| A build error | Two lines added to `shake.rs` (a type error, a variable that is not used): the tab "Build" opens with "The last build failed", the two rows with `game\scripts\src\shake.rs:190` and `:189`, and the line of cargo with no place; a notice at the bottom right. The editor goes on with the old code |
| A click on a row | With `code_editor = 'echo {file}:{line}> FILE'`: the file has `C:\...\game\scripts\src\shake.rs:190`. Visual Studio Code itself was not started |
| A game that runs | The file changed while the game ran: the build ended, no new app. After "Stop": the new app |
| A command of the open room is gone | `ShakeCamera` renamed in `shake.rs` with room B open: the build is good, the load is refused, the notice has the room file, the line, and "no command `ShakeCamera`". The file put back: the next build loads |
| Another file of the project (`game/src/main.rs`) | A build of 0.3 s, no new DLL, no load (watch 6) |
| An engine crate changed (watch 5) | "Build the scripts" after a change of `crates/editor`: the panel has the error of cargo ("failed to remove file ... maki_engine_dylib.dll") and the line "The engine DLL is in use: a change of an engine crate needs a new start of the editor" |
| Validation over three loads (`MAKI_LOG=debug`) | No validation error, no panic. The old app, with its GPU device, is dropped before the new one starts (watch 1) |
| `cargo test`, static | 153 passed, 0 failed |
| `cargo test --profile dynamic -p maki-editor-app -p maki-game-dll -p maki-engine` | 5 passed: the loader, the rows of a build, the scan, the project file, the command line |
| `cargo clippy --all-targets`, static and dynamic | Clean |
| `python tools/reload_test.py` | As in stage 43: build 1.36 s, the second load has the new text |
| The static game | `bench.py same trip_720p s43 s44`: ok. `gpu_total` of the 26 scenarios against `s43`: -0.2 % to +2.0 % (the +2.0 % is `idle_1080p`, 0.101 to 0.103 ms); `trip_720p` 1.917 ms in the two runs |

## 3. Files

| File | Role |
|---|---|
| `crates/engine/src/reload.rs` (new) | The watch, the build on its thread, the load, the panel "Build", "Open in editor", the two commands; the tests of the scan and of the rows |
| `crates/engine/src/editor_program.rs` | `Setup`, `editor_app`: one function makes the first app and each next one |
| `crates/engine/src/loader.rs`, `lib.rs`, `Cargo.toml` | The name of the engine DLL for the hint; the module; `bevy_ecs` |
| `crates/platform/src/window.rs`, `lib.rs` | `NextApp`, `MakeApp`, and the swap of the app in the runner |
| `crates/editor/src/lib.rs` | `Session`, `session`, `Command::Restore`, `EditorModule::session`, the setting `code_editor` |
| `crates/editor/src/tests.rs` | The test of the session |
| `crates/ui/src/theme.rs` | The icon of the build panel |
| `docs/part6/stage-44-script-reload.md` (new) | The stage file |
| `README.md`, `OPEN_ITEMS.md`, `docs/part6/README.md`, `docs/part6/HANDOVER.md` | The section "Project" and the `[editor]` table; the row of the script DLL; the stage table; the state |

## 4. Deviations from the stage file

- **The selection goes to the new app.** The stage file said it is lost. With it, the gate holds as written: the event panel shows the new text with no action.
- **A failed build also gives a notice** ("see the panel Build"), not only the open panel.
- The stage file has no other deviation: it was written by this session one step before the code.

## 5. Decisions (the stage file is a proposal of this session)

- **A new app for each load, with all of the renderer made again.** README 2.3 says "into a new app". The other way (the new app takes the GPU device and the meshes of the old one) is faster by about 0.2 s and needs a hand-over in the render crate: not worth it at this time of a load.
- **The document goes over as text**, and is read with the command registry of the new app: its event commands are values of types of the old copy of the DLL. This is also the check "the open room reads with the new code".
- **The watch is a look at file times**, not a crate that the OS tells of a change (`notify`): no new dependency, and 3 looks in a second at some ten files cost nothing that shows.
- **The watched files** are all Rust files and `Cargo.toml` files of the project folder, also those of the program of the game: `project.toml` does not say where the script crate is. A build for a file that is not of the script crate takes 0.3 s and loads nothing.
- **The rows of the build** come from the lines of `cargo build --message-format short`. The JSON form of cargo needs a JSON crate as a new direct dependency.
- **"Open in editor"** runs the command line with the shell (`cmd /C` on Windows): `code` is a `.cmd` file there. "Open the project in the code editor" runs the first word of the setting with the project folder.
- **The first build stays before the window** (stage 43): a new start of the editor always has the code of the project in its first frame.
- **Not in the stage** (the stage file says so): "New script" and its templates, the log panel, a panic of a script that the editor catches (item 31), pause and one frame (item 33).

## 6. Uncertain

- The session was scripted. Visual Studio Code was not started: the command line of "Open in editor" was checked with a command that writes it to a file.
- A build while another program has the lock of the build directory (`rust-analyzer` of the code editor) was not tried: cargo then waits, and the job shows that long.
- What a load loses: the undo steps, and what egui keeps of the panels (the folds, the scroll places, a text field that has the focus). The status row says that the undo steps are gone.
- A load makes the GPU device again. On another GPU or with a room of many meshes this can take longer than the 0.2 s of room B.
