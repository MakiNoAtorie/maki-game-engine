# maki-game-engine

Top-down 3D horror game engine with path-traced lighting, written in Rust.

## Current target (part 6: a general engine)
Make the engine a general engine with the workflow of Unity, Godot, and Unreal, in which a game in the style of RPG Maker can be made. It must not copy the workflow of RPG Maker: parts 4 and 5 made the event (pages, triggers, command rows) the only way to write logic, and the item kinds of a room a closed set. Part 6 corrects the two: objects with components that modules register, kinds as prefabs, scripts in Rust in a script crate of the project (loaded as a DLL, so a script change does not close the editor), and the editor as a program of its own that opens a project. The events stay, as a way with no code on the same system.
The plan, the decisions of the owner, and the stage files are in `docs/part6/`. Start with `docs/part6/HANDOVER.md`, then `docs/part6/README.md`. Parts 2 (`docs/part2/`), 3 (`docs/part3/`), 4 (`docs/part4/`), and 5 (`docs/part5/`) are done.
The architecture must let later features plug in without rework (upscaling, software tracer backend, Metal backend, skinned characters, baked lighting). Favor correct seams over feature breadth.
Picture quality and GPU time must not regress: compare `tools/bench.py` runs before and after each stage.

## Stack
- Rust stable, Cargo workspace.
- Vulkan 1.3 via `ash`.
- Ray queries in compute shaders: GLSL with `GL_EXT_ray_query`, compiled to SPIR-V by `build.rs` using `glslc` (Vulkan SDK).
- `bevy_ecs` standalone (not the Bevy engine). Do not use removed APIs: `SystemLabel`, `add_system_set`, run criteria, `State` push/pop.
- `winit`, `gpu-allocator`, `egui`, `slotmap` (generational handles), `kira` (sound).
- Sound files: WAV and Ogg Vorbis.
- `tracing` for logging; `tracing-tracy` behind cargo feature `profile`.
- Config in TOML. Room files (rooms and their events) in RON.
- Use current stable crate versions and verify APIs against their docs.

## Workspace
`crates/core`, `crates/platform`, `crates/assets`, `crates/scene`, `crates/events`, `crates/audio`, `crates/gameplay`, `crates/gpu`, `crates/render`, `crates/ui`, `crates/devtools`, `crates/editor`, `crates/engine` (with `crates/engine/dylib`), `programs/editor`, and `game/` (the project of this repository).
- `scene`: the components that describe the world, the room data format, load and unload of rooms.
- `events`: flags and variables, event definitions, the event interpreter, the registry of event commands.
- `audio`: the sound output (`kira`), sound assets, music, the sounds at a place in the room, the audio event commands. No type of `kira` in its public API.
- `gameplay`: generic gameplay modules (camera follow, walking controller, start room, debug controls), each with its own settings.
- `devtools`: tools that a game opts into (benchmark runner, room list, gameplay panel).
- `ui`: egui, the debug panels of a game, and the shell of a tool (docks with tabs, the panel and command registries, the widgets, the theme).
- `editor`: the room editor, a library. It edits a document (room, events, comments) and builds the world from it; it never saves the world.
- `engine` (`maki-engine`): the engine as one crate, and the only one that a project names. The module list of a game and of the editor, the command line, the log setup, the project file (`project.toml`), the two entry functions (`run_game`, `run_editor`), the loader of the script DLL. `engine/dylib`: all engine crates as one DLL.
- `programs/editor`: the editor program `maki-editor`: engine and editor, no code of a game. It opens a project.
- `game/`: the project: `project.toml`, `assets/`, the script crate `scripts/` (the Rust code of this game; `scripts/dll` is the same crate as a DLL for the editor), and the program of the game (`maki-game`: engine and script crate linked statically, no editor). Generic code goes into an engine crate.
- Two builds. Static: `cargo build`, `cargo test`, `cargo clippy` (the engine crates, the game, its script crate). Dynamic, for the editor: `--profile dynamic -p maki-editor-app -p maki-game-dll`, always the two packages in one call (`editor.bat`; `maki_engine::BUILD`).

## Architecture rules (do not break)
1. Dependencies flow downward only: core -> platform -> assets -> scene -> events -> audio -> gameplay -> gpu -> render -> ui -> devtools -> editor -> engine -> game. A crate depends only on crates to its left. `scene`, `events`, `audio`, and `gameplay` must not depend on `gpu`, `render`, or `ui`.
2. No `ash` or Vulkan type in any public API outside the `gpu` crate.
3. The renderer reads only data copied into it during the Extract stage, never game state directly.
4. Gameplay code reads input actions, never raw keys.
5. Every feature is a module that registers its own systems and resources. Adding a module must not require editing another module. A module owns its config table (`Settings`), its input actions and their keys (`add_bindings`), and its event commands (`CommandRegistry`). A module adds its panels and its commands of the editor to the shell (`maki_ui::add_tool_panel`, `add_command`), with no change in the editor crate.
6. Long-lived resources (GPU resources, assets) are referenced by generational handles.
7. Gameplay state that must survive a room change (flags, variables, player state) lives in resources outside room entities.
8. Game content (rooms, objects, prefabs, events) is data, loaded at run time. The Rust code of a project is in its script crate, and of the engine it names `maki-engine` only. Neither a change of content nor a change of a script needs a new build of the editor. No engine crate has a constant of a game: the project file has them.

## Conventions
- Test fixtures (meshes, glTF files, config files) are generated by code, not hand-written and committed.
- Benchmark and timing output is CSV.
- Vulkan validation layers are on in debug builds. A stage is not done while validation reports errors.
- The user is learning from this code: write clear, readable code; explain non-obvious decisions in short comments.

## Documentation
- Write all documentation (READMEs, doc comments, reports) in lean, technical English. No verbose or flowery prose.

## Skills
- Use the `ponytail` skill proactively on every coding task (write, fix, refactor, review, choose dependencies).

## Working agreement
- Work in stages, in the order of the stage table (`docs/part6/README.md`, 3.1). Implement only the stage at hand.
- At the end of each stage, always, in this order:
  1. Sanity check: tests and clippy (each build form), a validation run, the benchmark against the stage before, each line of "Done when" and each point of "What to watch for" of the stage file, and a run by hand of what the stage changed.
  2. Fill the gaps and fix the bugs that the check finds, in the same stage. Then check again.
  3. Report (`docs/partN/stage-NN.md`):
     1. What was built, in a few lines.
     2. Files created or changed, one line each on role.
     3. Deviations from the spec and why.
     4. Decisions made where the spec was silent.
     5. Anything uncertain.
  4. Commit on `main` as `Stage NN: <name>` and push.
  5. Go on with the next stage. Do not wait for the owner.
- A next stage with no stage file: write the file first, in the form of the others, from the plan and the code as it is then. A proposal of a stage file that the owner did not change is taken as given.
- Stop and ask: a gate that cannot be met; a decision of the plan that the code shows to be wrong; a regression of GPU time or picture that is the code; a dependency that the plan does not name; a stage that the plan marks "in discussion".
- Where the spec is silent, choose the simplest option that respects the rules, and list it in the report.
