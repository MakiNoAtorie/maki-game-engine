# Stage 44: a new build of the script DLL while the editor runs

Plan: [README.md](README.md), 2.3 and 2.4. Report: `docs/part6/stage-44.md`. Needs: stage 43 ([stage-43.md](stage-43.md)).

Goal: the user saves a script file in the code editor, and the editor has the new code with no new start: it builds the script DLL behind the work and loads the new copy. A build error shows in a panel, and a click opens the file at the line in the code editor.
Gate: with the editor open and a room with changes that are not saved: a saved change of `game/scripts/src/shake.rs` shows in the event panel (the row text of `ShakeCamera`) with no action of the user, and the room still has its changes. A build error shows in the panel "Build" with its file and its line; a click runs the command of the code editor with the two. A game that runs is not stopped by a build.

Not in this stage: "New script" and its templates, the log panel (with the object model, README 3.1); a panic of a script that the editor catches (item 31); pause and one frame (item 33); a project outside the repository.

State: written by the session that did stage 43, from the plan (README 2.3, 2.4, 2.13) and the code at `67277d1`. Not seen by the owner: each point is a proposal.

## Context in the code

- `maki_engine::run_editor` (`crates/engine/src/editor_program.rs`) builds the script DLL one time, before the window, with `BUILD` and the DLL package of the project, and loads it with `load_project`. A build error is a notice with the last lines of cargo.
- `load_project` loads a copy of the DLL and never unloads it. The modules of the DLL add systems and closures (event commands) to the app: an app cannot give them back. README 2.3: the new copy goes "into a new app".
- `maki_platform::run` (`crates/platform/src/window.rs`) owns the app and the window (`Runner`). `Startup` of the render module makes the GPU device and the swapchain for the window.
- The editor (`maki_editor::Editor`) holds the document. Its event commands are values of types of the script DLL: a document of the old copy is not a document of the new one. `Document::text` and `Document::parse` go by way of the text of a room file.
- `maki_ui::Jobs` (stage 42) is the jobs part of the status row. It has no writer.
- The layout of the docks is in a file (stage 42): a new app reads it.

## Prompt

```text
Stage 44: a new build of the script DLL while the editor runs.
Read docs/part6/README.md (2.3, 2.4) and
docs/part6/stage-44-script-reload.md first.

Watch and build
- The editor watches the Rust files and the Cargo.toml files of the
  project folder (not `assets/`). No new crate: it looks at the file
  times, some times in a second.
- A change starts the build command of stage 43 on another thread. The
  status row shows the job. A change while a build runs starts one more
  build after it.
- The first build, at the start of the editor, stays before the window.

Load
- A build that wrote a new script DLL: the editor makes a new app in
  the same window, with the modules of the new copy, and drops the old
  app. `maki_platform` gets the seam: a request for another app.
- What goes to the new app: the open document (as the text of a room
  file, with its path and its "not saved" mark), the view of the editor
  camera, and the spawn point of "Play". The layout comes from its file.
- A document that the new code cannot read (a command is gone): no
  load. The old app stays, and a notice says why.
- While a game runs: the load waits for "Stop".

Build errors
- A panel "Build" (bottom dock, closed in the default layout): the
  state of the last build, and one row for each error and warning:
  file, line, text. A failed build opens it.
- A click on a row: "Open in editor". The command is a setting
  (`[editor] code_editor`, default `code -g {file}:{line}`).
- A command "Open the project in the code editor" in the "File" menu.

Done when
- The gate.
- A test: the list of the watched files and a change of one of them.
- A test: the rows of the build panel from the text of a cargo build
  with an error and a warning.
- A test: the document of a session goes to a new editor app with its
  change and its "not saved" mark.
- `cargo test` and `cargo clippy` clean for the two builds;
  `tools/reload_test.py`; a validation run of the editor over two loads.
- The time from the save of a file to the new code in the editor, in
  the report.
```

## What to watch for

1. **The GPU of the old app.** One swapchain for one window: the old app is dropped before `Startup` of the new one runs. Validation must be clean over a load.
2. **The time of a load.** The new app makes the GPU device, the pipelines, and the meshes again. Measure it. If it is long (more than about 2 s), say so in the report: a load that keeps the renderer is a stage of its own.
3. **Work that is not saved.** The document goes over as text. Undo steps and the selection are lost at a load: say so in the status row.
4. **A build while cargo of another program runs** (the code editor has `rust-analyzer`): cargo waits for the lock of the build directory. The job shows as long as it takes.
5. **The engine DLL.** A change of an engine crate cannot be built while the editor runs: the error of cargo names the file that is in use. The panel must show that text.
6. **A change of a file that is not of the script crate** (`game/src/main.rs`): the build runs and writes no new DLL. No load then.
7. **The loader checks of stage 43** hold for each load, not only the first.
8. **The static game** does not change: `bench.py same trip_720p`.
