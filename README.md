# maki-game-engine

Top-down 3D horror game engine with path-traced lighting, written in Rust.

## Required tools

- Rust (stable), installed with [rustup](https://rustup.rs).
- [Vulkan SDK](https://vulkan.lunarg.com) 1.3 or later. It supplies `glslc` and the validation layers.

## Build and run

```
cargo build
cargo run -p maki-game                 # the game; uses ./config.toml
cargo run -p maki-game -- my.toml      # uses the given config file
cargo run -p maki-game --release -- --bench-frames 300   # benchmark, see below
editor.bat                             # the editor with the project `game`, see below
```

Two programs. `maki-game` is the game: the engine and the Rust code of the project in one program, with no editor (`run.bat` builds and starts it). `maki-editor` is the editor: the engine and the editor, with no code of a game; it opens a project folder (see "Project").

If the config file does not exist, the game writes one with the default values of all modules.

- Log level: set `MAKI_LOG` (default `info`). Example: `MAKI_LOG=debug`.
- Tracy profiler: `cargo run -p maki-game --features profile`. Each schedule is a zone with the name of the schedule (`Input`, `FixedUpdate`, `Update`, `Extract`, `Render`, `Present`). Each frame graph pass is a zone with the name `pass`; the zone text has the pass name. The zones show CPU time. With `MAKI_LOG=trace`, the log lines show the same spans.
- Render self-test: set `MAKI_RENDER_SELFTEST`. The internal resolution changes every 7 frames; validation must stay clean.
- Scene counts: the log shows each mesh upload (vertices, indices) and a `render scene` line (meshes, instances, triangles) each time the counts change.
- Frame graph barriers: with `MAKI_LOG=debug`, the log shows the barrier list each time it changes.
- Accumulation: with `MAKI_LOG=debug`, the log shows `accumulation complete` when the sample count reaches `max_samples`, and `accumulation starts again` when a count of two or more frames starts again. With `MAKI_LOG=info,maki_render=trace`, it shows the cause of each change (`the camera changed`, `a light changed`, `an instance changed`).
- Events: with `MAKI_LOG=debug`, the log shows each event that starts and each message.

## Controls

The help panel in the game lists all controls, with the key that hides and shows the panel. Its section is folded at start. The list is made from the binding table (`maki_platform::Bindings`). A binding is a key or a chord (`Chord::ctrl(Key::Z)`, shown as "Ctrl+Z"); a chord wins over the plain key of the same mode, and a key with no chord works with a modifier down. Each module adds its actions and their keys to it (`maki_platform::add_bindings` in `Module::build`): to change a key, change the table of the module.

The move keys walk the player; forward is away from the camera. Shift runs. Z or Enter is "interact": it starts the event that the player faces, and it closes a message. While an event runs (a message, a fade), the player does not move.

Debug keys: L gives the move keys to the first light of the room (and back), Q and E move that light down and up, one wheel step changes its intensity by 10 %. O opens or closes all doors. F5 reads the room file again. Page down and Page up go to the next and the previous stage (see the Stages panel). R starts the picture again.

## Debug panels

The picture has a fixed size (`width` and `height` of `[window]`) and is at the top of the window, not scaled. The panels are the sections of a column at the right of the picture: no panel hides the picture. Click a title to unfold a section. The window is as large as the picture and the column; in a larger window the column gets the rest. The first row of the column is the menu "Panels", with one checkbox for each panel: a panel that is not checked has no section.

| Panel | Content |
|---|---|
| Help | Key and description of each binding |
| Frame counter | Not a section: green text in the top left corner of the picture. The frame rate and the frame time that the game would have with no limit: the time of a frame without the wait for the display (vsync) or the GPU, and not less than the GPU time of the passes. A mean over some frames |
| Stats | Frame time (mean) and FPS as the display shows them, sample count, internal resolution, player position, position and intensity of each light, GPU memory, GPU time of each frame graph pass |
| Debug | The resolution (the picture size, from 640 x 360 to 2560 x 1440: the window follows). The view (final, one raw frame, motion vectors, depth, normals, surface color, history length, variance, direct light, indirect light before and after its filter, specular light) and the settings of `[window]` and `[render]` that have an effect during a run: vsync, render scale, exposure, look, path tracer, denoiser. Not saved to the config file |
| Gameplay | The settings of `[camera]` and the values of the player that have an effect during a run: field of view, follow time, wall margin, view direction, speeds. Not saved to the config file or the prefab file |
| Bench | Starts a benchmark run (frames, script, light, picture dump) in the game that runs. Writes `bench.csv`; the game goes on. For numbers to compare between runs, use the command line |
| Stages | Each room file that the game can show: the files in the directory of the start room, and `target/bench/<scenario>/room.ron` (the generated scenes; `python tools/bench.py run` makes them). A click loads the room. The config stays: set the render size or the sample count of a scenario in the Debug panel |
| Game state | Flags and variables of the events, with a field to add one; the events that run |
| Text box | Not a section: the text of a `Message` command, or the question and the answers of a `Choice`, in a box at the bottom of the picture (`crates/ui/src/events.rs`), with the keys that go on. Open only while a text waits |

While the mouse pointer is on a panel, the mouse (buttons, wheel, motion) starts no action. While a text field of a panel has the keyboard focus, keys start no action.

To add a panel from a module, call `maki_ui::add_panel(app, "Name", true, |ui, world| { ... })` in `Module::build`. The closure gets the egui `Ui` of the section and the game world. `crates/ui/src/stats.rs` is an example. `maki_ui::add_left_panel` adds a panel at the left of the picture: the picture then starts at the right of it, and the window is wider by it.

A tool has another layout (`Panels::set_tool_layout`, the editor): panels at the top, the left, the right, under the picture, and a status row (`maki_ui::add_tool_panel` with a `ToolPlace`). The user drags the width of the side panels and the height of the bottom panel. The debug panels of the table are then one window, closed at the start. With `maki_platform::PictureFit::Window` the window starts maximized, the user sets its size, and the picture has the room that the panels leave.

`maki_platform::Picture` is the one place that tells where the picture is in the window and how large it is, in pixels. `maki_platform::update_picture` makes it in `Extract` from the `[window]` settings (the game) or from the window size (a tool) and the room that the panels take (`WindowMargin`). The renderer, the UI, and the editor read it. In a tool, a new size waits until it did not change for 8 frames: a drag of a window edge or a panel edge does not make the images of the renderer again in each frame.

## Editor

`editor.bat` builds and starts the editor program `maki-editor` with the project of this repository (`game`), with the start room open. `maki-editor [PROJECT [CONFIG]]`: the project folder (not given: `game`), then the config file. The editor is the crate `crates/editor`; the program of a game does not have it. Its layout is a shell of `crates/ui`: a module adds a panel (`maki_ui::add_tool_panel`) and a command (`maki_ui::add_command`) with one call each, with no change in the editor crate.

- The window starts maximized. The picture is the tab in the middle, with the name of the room file (` *` for changes that are not saved): it has the room that the docks leave, and the shape of that room, not that of the game. In the view of the game camera (G), a thin frame shows the picture of the game (`[window]` size). The title bar has the room file, the name of the project, and "maki editor".
- At the top: the menus "File" (new room, open a room file of the project, save, save as, quit), "Edit" (undo, redo, duplicate, delete, copy, paste, rename, the prefab commands), "Add" (each kind of item, a model file, a prefab file), "View" (frame, the view from the top, the view of the game camera), and "Window" (each panel, "Reset layout", the named layouts). Each row of a menu has the key of its command and is grey while the command cannot run. Below them the tool row: at the left the tools, "Local", and the snap; in the middle "Play" and the spawn point where the game starts; at the right the list of the layouts.
- Each panel is a tab of a dock: the outliner at the left, the inspector at the right, the events under the picture. Drag a tab to another dock (the middle target groups it with the tabs there, a side target splits the dock), or out of the docks (it floats in the window of the editor). The button of a tab closes it; "Window" opens it again. Drag the edge between two docks to change their sizes. The layout is saved in a file beside the config file (`config.layout.ron`) and read at the start. The named layouts: "Build" (the default), "Logic" (more room for the events), "Look" (no event panel), "Animate", and "Debug" (the debug panels of the game as tabs under the picture: Help, Stats, Debug, Gameplay, Game state).
- The command search (Ctrl+P, or the button beside the menus): type a part of the name of a command, a panel ("window inspector"), or a layout ("layout debug"); the arrow keys and Enter, or a click, run the row.
- At the left: the outliner: the room, its doorways, doors, props, movers, lights, spawn points, and events. The field above it filters the rows by a part of the name. A double click on a row, or F, puts the item in the middle of the view. The right mouse button on a row: rename, frame, duplicate, delete. F2 makes the name of the selected item a text field in its row: Enter takes the new name, Esc does not. An event with a problem is red (an error) or yellow (a warning), with the problem as hover text. An item with no name shows its kind and its index; a new prop gets a name (`cube`, `cube_2`).
- At the right: the inspector with the values of the selected item, and its notes. A number changes by a drag or by a click and typed text; a text field changes the room when it loses the focus. A value that the room file can leave out (a pose, "on", the volume of a sound) has a small button at the end of its row while it is not the default: "back to default". For an event: its name and its area; the rest is in the panel "Events" (below). "Where used" is a fold at the end.
- At the bottom: the status row, in four parts: what the last command did, the jobs that run behind the work, the count of errors and warnings in the events (a click lists them), and the frame time. Why a command did not do its work is a notice at the bottom right of the window: it stops no work, and stays until its button closes it.
- While the game is played (Play), the picture has a frame in the play color, the tool row has "Stop" (and "Step", "Run" in a test), the outliner has the events that run, and the inspector has the game state.
- The view: drag with the right mouse button to turn it, with the middle button to move it, the wheel goes nearer and farther. T is the view from the top, G the view of the game camera. The help panel lists the keys of the editor while the room is edited, and those of the game while it is played.
- Selection: a click in the picture selects the item under the pointer (a prop, a door, a mover, a light, a spawn point, the area of an event); a click on a wall, the floor, or nothing selects nothing. Shift and a click adds an item or takes it out. The outliner has the same selection. The pick is against what the picture shows: a click goes through the near walls as the view does, and a mirror or a glass ball is selected itself, not what shows in it.
- The gizmo is at the last selected item. "Move" (W): a handle for each axis, and the square at the origin moves the item over the floor. "Rotate" (E): a ring for each axis. "Scale" (R): a handle for each axis of the item. "Local" (Q): the handles follow the axes of the item, not those of the room. A light, a spawn point, and an event area only move. A turn of more than one item turns the group around the gizmo: each item goes around it. A mover has two poses: a move and a turn take the two, or only pose `b` with the checkbox in the inspector; the marks show the mesh at pose `b` and the way between the two.
- Handles of the item itself, white squares, with each tool. A cone light: at the point of the floor that the middle of its cone meets; a drag aims the light at the point of the floor under the pointer. An event area: at each corner; a drag moves the corner, and the corner across stays. A doorway: at each top corner; a drag on the wall changes the width and the height, and the other side stays. They snap to the grid as a move does.
- Snap: a move snaps to the grid step and a turn to the angle step of the `[editor]` settings (the two fields beside "Snap"); a scale snaps to 0.05. Hold Ctrl in a drag for no snap.
- The "Add" menu puts a new item on the floor at the point that the view looks at: a cube, a cuboid, a sphere, a model file (the `.gltf` and `.glb` files at the room file), a light, a spawn point, a doorway, a door (in the first doorway with no door), a mover, an event area. "Delete" (Delete key) removes the selection; a doorway takes its door with it, and the doors of the other doorways keep their doorway. "Copy" (Ctrl+D) makes a copy of each selected item, half a meter beside it.
- Names: a light, a door or mover, a spawn point, and an event get a name that no other one of its kind has (`lamp_2`). A new name of a light, a door, or a mover goes into the built-in commands of the events of the room that name it (`LightOff`, `OpenDoor`, ...). A new name of a spawn point: the status line names each other room file whose `Transfer` still has the old name; the editor does not change other files.
- Undo (Ctrl+Z) and redo (Ctrl+Y): each change of the document is one step, also a full drag of a handle or of a number. The steps go away when another room is opened; a save keeps them. Undo and redo also put back the selection of the state that they go to: the undo of a delete selects the item again.
- A file that another program saves shows at once, while a room is edited and while a game runs in the editor: a texture (PNG, JPEG), a model (glTF, with its image files), a sound. The editor looks at the times of the files that are loaded, 4 times in a second; a file whose time changed is read again when it did not change for 0.3 s more, and the room is loaded again with the player in place (about 0.6 s from the save to the picture). A sound at a place that plays starts again with the new file, and music from its start. A file that does not read (half written, broken) changes nothing: the status row says so. "Assets > Load all again" does the same for each file, for a program whose save does not change the time of the file. Not watched: the room files and the prefab files.
- Source files of other programs: a `.blend`, `.obj` (with its `.mtl`), or `.psd` file in the folder of the room files (or a folder in it). The engine does not read them: the editor converts each one to a file next to it with the same name (`chair.blend` gives `chair.glb`, `wall.psd` gives `wall.png`), at its start and each time the source is saved, and a room names the result file. Blender does `.blend` and `.obj`, with no window (`[editor] blender`; not set: `blender` of the path, then the usual folder); the result has what Blender shows (modifiers applied), +Y up, its images in the one `.glb` file, and no camera and no light of the Blender file. A `.psd` file gives its flat picture (RGB or gray, 8 or 16 bits): the layers are not read, so save with "maximize compatibility". In a file list, a result file has a mark that names its source, and a source that is not converted yet shows by the name of its result. A source that does not convert gives a notice and one line in the status row, and the old result stays; the log has the output of Blender. A result file that is newer than its source is not written again. About 3 s from a save in Blender to the picture, 1 s for a `.psd` file.
- Marks on the picture, in the editor only: the floor grid (1 m), the names and places of the spawn points and event areas, the hinge and the swing of each door, the second pose of each mover, the box of each selected mesh, the radius and the cone of a selected light, the opening of a selected doorway. What is in front of a mark hides it: the grid does not show on a prop. The marks of the selection and of the item under the pointer, and the gizmo, show through each object. The editor cuts the lines with the distance of each pixel, which it reads from the GPU after the picture changed (`maki_render::read_distances`); while the view moves the cut is one frame late.
- Play (the button or F5) runs the game in the room as it is in the editor, from the selected spawn point (a room with none: from the point that the view looks at), with no question for changes that are not saved. While the game runs, a load of the open room file (a `Transfer` back to it) gets the room of the editor, not the file (`maki_scene::RoomOverride`). Stop (the button, F5, or the quit key of the game) goes back to the editor: the room and the game state are those of before the play. What events did (flags, lights, doors, another room, a fade, a message) is gone.
- New, open, and quit ask first if the room has changes that are not saved; "Save as" asks before it writes over the file of another room. The question is in the middle of the window, and nothing else takes input while it waits. Enter is the first answer, Esc is "Cancel".
- The item or the handle under the pointer shows it: a thin white box around the item, a thicker and brighter handle. Esc in a drag of a handle puts the values of the start of the drag back, with no step of the history; Esc with no drag selects nothing.
- Delete, Ctrl+D (duplicate), Ctrl+C, and Ctrl+V go to the list that had the last click: the items (the picture, the outliner) or the command rows of the event panel. Copy and paste are for command rows only. The hover text of a button has the key of its action, from the binding table.
- Inspector: the rows of an item (the room, a doorway, a door, a prop, a mover, a light, a prefab) come from the table of the fields of its type (`maki_core::fields`, the derive `Fields`): the label, the kind of widget, the range, the step, and the default of a field are written at the field, in the crate of the type, and the editor has no code for an item kind. The doc comment of a field is the hover text of its label. A value that is not its default has "back to default". A spawn point and an event have rows of their own.
- Inspector: the labels are in one column and the values in the next one, also for a group in a group; a length has `m`, an angle `deg`, a time `s`, and X, Y, Z have the colors of the gizmo. A texture and a glTF file are picked from the list of the files at the room file. The direction of a light is in the group of its cone.

The panel "Events" shows the selected event (selected in the outliner or by its area in the picture); its tab comes to the front when an event is selected. At its left the pages with their conditions and trigger, in the middle the command rows, at the right the arguments of the selected row:

- In the inspector: the name, and the area on the floor (also by the gizmo). With the checkbox off the event has no area: for `Autorun` and `Parallel`.
- A name (a flag, a variable, a light, a door, a room file, a spawn point, a command of "Add") is picked from a list with a search field: type a part of the name, Enter takes the first one. The lists of flags and variables have the entry "New...": type the name, then Enter.
- Pages as tabs, with add, copy, delete, and the two arrows for the order. A star marks the page that the game would run with the flags and variables of the "Game state" panel (the last page whose conditions hold): set a flag there to see another page get the star, and to play with that state. Stop gives the values of before the play back.
- Conditions as rows: a kind (flag is set, flag is not set, variable is at least, variable is), a name, a value. The trigger is a choice of the four.
- Commands as rows, one line each, as text for a reader (`Message: The door is locked.`, `If power_off and count >= 3`); the text of the room file is the hover text. A module gives the row text and one line of help for its commands with `CommandRegistry::add_texts` (`game/scripts/src/shake.rs`); without it the row is the text of the room file. The two lists of an `If` are under it, with `Else` and `End`. "Add" has each command of the game, with its help as hover text; the new command goes after the selected row (after `If`: into its first list; after `Else`: into the second). Drag a row to another row to move it: a line shows where it goes. Shift and a click selects the rows from the row of the click before to this one, in one list; "Delete", "Duplicate", "Copy", and "Paste" take the range. The buffer of "Copy" stays when another event or another room is opened.
- The arguments of the selected command: a light, a door, or a mover is picked from those of the room; a flag or a variable from each name that an event of any room file beside the open one uses, or typed as a new one; a `Transfer` picks a room file, then one of its spawn points. The audio commands pick a sound file of the directory of the room file (and one level below it), and a light that has a sound. A module gives the widgets of its commands with `maki_editor::add_argument_editor` (`maki_editor::widgets` has a number, a file, and a name of a list), and the new command of "Add" with `CommandRegistry::add_defaults`: `crates/editor/src/audio.rs` is the example. A command with none of the two is a text field with the command as a room file has it (`ShakeCamera(0.6, 0.06)`), checked with each key.
- Checks, on the row and with `(!)` at the event in the outliner. Red, an error: a light, a door, or a mover that the room does not have; a `SoundOn` or `SoundOff` for a light with no sound; two events with one name. Yellow, a warning: a `Transfer` to a room file or a spawn point that does not exist, a sound file that does not exist. The room loader of the game makes the same checks with the same code (see "Rooms and events"): it does not load a room file with an error. A room with such a problem opens in the editor and can be saved; the status line gives the count.
- A flag or a variable that an event reads and no event of any room file writes has the yellow mark "no event sets it", in the pick lists and in "Where used": a name with a wrong letter, or a flag that only a test sets.
- "Test this page" plays the room and starts the page, whatever its trigger and conditions are, in step mode: each event waits before each command, "Step" (in the tool row) runs the next one, and the outliner shows the command that waits. "Run" ends the step mode.

The panel "Where used" lists each flag and each variable of the game with each place that reads or writes it: room file, event, page, and the condition or command.

The editor changes a document (the room, its events, its comments), not the world. The world is made from the document: a change of a light or of the pose of a prop changes the entities in place, with no stop of the picture; each other change loads the room again from the text of the document (each mesh of the room is built again on the GPU, some milliseconds). Only the document is saved.

A saved file is RON as a person writes it: the field order of `RoomDef`, no field that has its default, one item on one line if it is not longer than 110 characters, and the `//` comments of the file. A comment before an item of a list (`props`, `lights`, `events`, ...) or in it is the note of that item and stays with it; each other comment is a note of the room and goes to the top of the file. `maki-editor game --resave ROOM` writes a room file again in this form, with no window.

## Project

The result file of a source file of another program (`chair.glb` of `chair.blend`, `wall.png` of `wall.psd`) is a part of the project: the game reads the result, and a machine with no Blender needs nothing. For git: a result file is binary and changes with each save of its source; commit the source and the result together, or keep the sources out of the repository.

A project is a folder with the data and the Rust code of one game. `game/` is the project of this repository:

| Part | Content |
|---|---|
| `project.toml` | What the engine must know of the game: its name, the directory of the room files, the start room and its spawn point, the save file, and the cargo package of its script DLL. A path starts at the project folder |
| `assets/` | The rooms, with their models, textures, and sounds |
| `scripts/` | The script crate: the Rust code of the game (its event command `ShakeCamera`, its benchmark scripts). Of the engine it names one crate, `maki-engine`. Its entry function adds its modules to the app |
| `scripts/dll/` | The script crate as a DLL, for the editor: one line |
| `Cargo.toml`, `src/main.rs` | The program of the game: `maki_engine::run_game` with the entry function of the script crate |

The two builds:

| Build | Command | Engine crates | Script crate |
|---|---|---|---|
| Static: the game, the tests | `cargo build`, `cargo test` | In the program | In the program |
| Dynamic: the editor | `cargo build --profile dynamic -p maki-editor-app -p maki-game-dll` | One DLL (`maki_engine_dylib.dll`) | A DLL that the editor loads |

- The editor builds the script DLL of the project at its start, with the build command above, and loads a copy of it. A build error: the editor starts, shows the error as a notice and in the panel "Build", and the project has no code until a build is good (a room file that names a command of the project does not read).
- While the editor runs, it watches the Rust files and the `Cargo.toml` files of the project folder (not `assets/`). A saved change starts the build command behind the work; the status row shows the job. A build that wrote a new DLL is loaded with no action of the user: the editor makes a new app in the same window with the modules of the new copy, and the open room goes to it with its changes that are not saved, its selection, and the view (the undo steps do not). About 2 seconds from the save to the new code. While a game runs, the load waits for "Stop". A room that the new code cannot read (a command is gone): no load, and a notice says why.
- The panel "Build" (it opens when a build fails; "Window" has it): the state of the last build, and one row for each error and warning. A click on the file of a row runs the command of the code editor with the file and the line (`[editor] code_editor`). "File" has "Build the scripts" and "Open the project in the code editor".
- A change of an engine crate cannot be built while the editor runs: the engine DLL is in use. The panel says so; start the editor again.
- The build command always has the two packages: with another package set, cargo can give some crates other features and build the engine DLL again, and the editor that runs has that file.
- The loader refuses, with a message and with no call into it: a DLL that is older than the engine DLL (it is of another build of the engine), a DLL that has its own copy of the engine crates, and a file that is not the DLL of a project.
- A program of the dynamic build needs the DLL of the standard library, which is in the folder of the compiler: `editor.bat` puts that folder into `PATH`.
- Tests of the dynamic build: `cargo test --profile dynamic -p maki-editor-app -p maki-game-dll` (the loader), and `python tools/reload_test.py` (a change of the script crate reaches a program that runs, and the build does not write the engine DLL).
- Not there yet: a project outside this repository (the script crate is a member of the workspace of the engine).

## Benchmark

```
cargo run -p maki-game --release -- [CONFIG] --bench-frames N [--bench-dump FILE [--bench-shot]] [--bench-static | --bench-light-frames M] [--bench-script NAME]
```

The game runs N frames (1 to 1000000), writes `bench.csv` in the working directory, and stops. In this mode:

- Each frame counts as one fixed step of game time (1/60 s), whatever the real time was: the simulation is the same in each run.
- The first light of the room moves on a circle around the room center, one turn in 240 frames. The position depends only on the frame number, so each run does the same work. The keyboard does not move the light.
- The light moves in each frame, so each frame is a change of the lighting: the indirect light of each pixel keeps `light_history_samples` old samples. `--bench-static` holds the light at the start of the path, so the picture gets all its samples. `--bench-light-frames M` moves the light in M frames, then holds it: the frames after that show how fast the picture follows.
- `--bench-script NAME` holds the light and plays the game with a script of actions. The scripts belong to the project (`game/scripts/src/bench.rs` adds them with `maki_devtools::add_bench_script`); the runner is a module of the engine (`maki_devtools::BenchModule`). The editor program has no benchmark. `walk`: in room A the player walks and a door opens; the last motion shows in frame 180. `trip`: the full loop, 820 frames: room A, room B, the switch, back to room A, through the door that was locked. `shake`: a check of the `ShakeCamera` command; it needs room B as the start room.
- `tools/bench.py` sets `render.samples_per_frame = 1` if the scenario does not set it: the automatic count would give each run another load.
- `--bench-dump FILE` writes the picture of frame N (the picture before the tone curve, at the output resolution) to FILE: two little-endian `u32` (width, height), then four `f32` (R, G, B, A) for each pixel. The picture depends only on the config, the scene, and N. `python tools/bench.py same` compares two dumps.
- `--bench-shot` (with `--bench-dump`): the file gets the picture as the window shows it (after the tone curve), and the run hides all panels. `python tools/shot.py OUT.png [--room room_b.ron --spawn west] [--set table.key=value ...]` runs the game that way and writes a PNG.
- `vsync` is off, whatever the config file says.

`bench.csv` has one row for each frame:

| Column | Content |
|---|---|
| `frame` | Frame number, from 1 |
| `cpu_ms` | Time from the start of the frame to the start of the next one. It has all schedules, and the time that the CPU waits for the GPU and for the display |
| `room_load_ms` | For a frame that loaded a room: the time of the load (the room file, the generated meshes, the upload to the GPU). Else 0 |
| `gpu_<pass>_ms` | GPU time of each frame graph pass, in frame order (GPU timestamp queries). The time of a pass has the image barriers before it |

The first frames are slow (startup work): drop them when you compute a mean. An empty cell means that the value did not arrive (a skipped frame, or a GPU without timestamp queries). A debug build has the validation layers on, which makes `cpu_ms` much larger.

## Config

The config file is TOML, with one table for each module. A module declares its table and its defaults as a type (`maki_core::Settings`) and reads it when it is added (`App::add_settings`): the type is a resource, for example `Res<CameraSettings>`. `crates/core` knows no key of another crate. A module that adds its table with `App::add_saved_settings` gets it written back to the file when a system changes the resource (0.4 s after the last change): `[editor]` does. Only the lines of the changed keys change; the other tables and the comments stay. The debug panels do not write the file.

- A key that the file does not have takes its default. A key that the table does not have, a value of the wrong type, and a value out of its limits stop the game at start: the error names the file, the table, the line, and the key.
- A table that no module of the build reads gives a warning in the log.
- A file of the old format (all keys in one list, before part 4) stops the game with the command that converts it: `python tools/convert_config.py config.toml`. The old file stays as `config.toml.old`.

`[window]` (`crates/platform`):

| Key | Default | Meaning |
|---|---|---|
| `title` | `maki` | Window title |
| `width`, `height` | 1280, 720 | Size of the picture, in pixels. The window is wider by the panel column, and in the editor by the panel at the left |
| `vsync` | `true` | Vertical sync |

`[render]` (`crates/render`):

| Key | Default | Meaning |
|---|---|---|
| `scale` | 1.0 | Internal path-tracer resolution: the picture size (`[window]`) multiplied by this, 0.1 to 1.0 |
| `width`, `height` | 0, 0 | A fixed internal resolution that does not follow the picture size. 0: use `scale` |
| `max_samples` | 1024 | The mean of a pixel has at most this many samples. While nothing changes, the path tracer stops when each pixel has them |
| `samples_per_frame` | 0 | Light paths for each pixel in one frame, 1 to 64. More samples cost more GPU time for each frame and give a clean picture in fewer frames. 0: automatic, the renderer fills `trace_budget_ms` |
| `trace_budget_ms` | 6.0 | GPU time of the path tracer in each frame, in milliseconds, for the automatic sample count. Keep it well below the time of one display refresh (16.7 ms at 60 Hz) |
| `light_history_samples` | 16 | When the lighting changes (a light, or an object that moves), the indirect light of each pixel keeps at most this many old samples for some frames, and new samples replace them. More: less noise while something moves, and the bounce light follows later. 0: a change of the lighting starts the indirect light again |
| `history_clamp` | 2.0 | History test of the direct light, in standard deviations of the noise. The denoiser keeps a second, short mean of the direct light (4 frames). Where the short and the long means of a block of pixels differ by more than this, the lighting changed there, and the long mean starts again from the short one: a shadow follows its object in few frames. 0: no test; a change of the lighting then cuts the direct light to `light_history_samples` too |
| `taa` | true | Temporal anti-aliasing. Each frame moves all camera rays by a part of a pixel, and the picture is the mean of some frames: smooth edges, and with `scale` below 1 more detail than one frame has. False: stairs at each edge |
| `taa_blend` | 0.1 | The part of a new frame in the picture of the anti-aliasing, at rest. Lower: smoother edges. 0: the plain mean of all frames since the last change, for a reference picture of a scene at rest |
| `max_path_rays` | 8 | The largest number of rays in one light path, 2 to 32. Only mirror and glass make long paths. A lower limit costs less GPU time and gives less noise; the last mirror or glass surface of a cut path shows as a diffuse surface, so deep reflections lose detail. Measured in `docs/perf-improve.md`, section 0 |
| `max_bounces` | 2 | A light path ends after this many diffuse bounces, 1 to 16. More: nearer to the light of a real room, for one more ray and more noise for each bounce. 2 with `bounce_light` is 20 times nearer to a picture with 8 bounces than 1 alone, for 16 % more GPU time |
| `bounce_light` | true | Light of many bounces for the cost of one: the last surface of a path also counts the bounce light that the same point had in the picture of the frame before, if that picture showed it. Corners that no lamp sees get their light. False: such corners stay too dark |
| `blue_noise` | true | The random numbers for the point on the light and for the bounce direction are spread evenly over the pixels (a blue-noise pattern), so the noise of one frame has no clumps. Measured: 6 to 10 % less error for a young picture, 1 % in motion |
| `denoise_passes` | 4 | Filter passes of the denoiser for the indirect light (bounce light), 0 to 6. Each pass doubles the area that the filter takes its mean from. 0: no filter, only the mean over frames |
| `denoise_direct_passes` | 1 | The same for the direct light. It has little noise and sharp shadow edges. A scene with large lights (wide soft shadows) can need more |
| `denoise_specular_passes` | 2 | The same for the specular light: the reflection of a rough surface (metal, plastic). The reflection of a smooth surface and of glass gets no filter |
| `denoise_reach` | 4096.0 | The filter gets narrower as the mean of a pixel gets more samples: a pass with `s` pixels between its points runs for a pixel while `s * s * samples` is at most this. 0: each pass runs at each pixel |
| `indirect_clamp` | 8.0 | The largest light that one bounce ray adds to a pixel, in units of the light at the surface that the pixel shows. It removes the bright speckle of rare paths, and it makes light that comes by way of a mirror or through glass (caustics) darker. 0: no limit |
| `texture_bias` | 0.0 | Added to the mip level that the renderer selects for a texture, -16 to 15. Below 0: sharper, and a fine pattern can sparkle (-16: always the full texture). Above: softer |
| `exposure` | 1.0 | The picture is multiplied by this before the tone curve. Manual: an automatic exposure would take away the darkness that the game wants |
| `tone_curve` | `"agx"` | The curve from light to display color. `"agx"`: the look of film, soft colors and contrast; a bright colored light goes to white with no change of its hue. `"neutral"` (Khronos PBR Neutral): colors as the scene has them, more contrast |
| `bloom` | 0.04 | The part of a wide blur of the picture that is blended into it, 0 to 1: a soft glow around the lamp and its reflections. 0: none |
| `vignette` | 0.25 | How dark the corners of the picture get, 0 (not at all) to 1 (black) |
| `grade_lift`, `grade_gamma`, `grade_gain` | `[0, 0, 0]`, `[1, 1, 1]`, `[1, 1, 1]` | The color grade, each for R, G, B: lift moves black toward a color, gamma bends the middle tones, gain scales white. The defaults change nothing |

`[camera]` (`crates/gameplay`, `CameraModule`):

| Key | Default | Meaning |
|---|---|---|
| `position` | `[0.0, 9.0, 7.0]` | With `target`: the view direction and the distance of the camera. The camera keeps them when it follows the player |
| `target` | `[0.0, 0.5, 0.0]` | The height of the point that the camera looks at is the Y of this point |
| `fov_degrees` | 45.0 | Vertical field of view |
| `follow_time` | 0.25 | The camera target moves most of the way to the player in this time, in seconds. 0: no delay |
| `margin` | 3.0 | The camera target stays this far from the walls of the room, in meters |

The player (`crates/gameplay`, `PlayerModule`) is a prefab file that the project file names (`player = "prefabs/player.prefab"`, from the folder of the rooms). Its object has the component `player` (`walk_speed`, `run_speed`: meters per second; `radius`: the circle for collisions, in meters) and `prop` (the mesh: `Capsule(radius, height)`, or `File("...")`, a glTF file with the feet at its origin, from the folder of the rooms).

`[rooms]` (`crates/gameplay`, `RoomsModule`):

| Key | Default | Meaning |
|---|---|---|
| `start` | not set | The room file that the game starts in. Not set: the start room of the project (`project.toml`). A path that is not absolute starts at the working directory |
| `start_spawn` | not set | The spawn point of that room where the player starts. Not set: that of the project |

`[assets]` (`crates/assets`, `AssetsModule`):

| Key | Default | Meaning |
|---|---|---|
| `background` | true | A model file is read, and its images are decoded, on another thread: the model shows some frames after the room. False: on the thread of the game, in the frame that asks for the file; each run then does the same work in each frame (the benchmark sets it) |
| `watch` | false | True: a file of a texture, a model, or a sound that another program writes again is loaded again, with no restart (see "Editor"). False: no file is looked at after its load. The editor watches with each value |

`[audio]` (`crates/audio`, `AudioModule`):

| Key | Default | Meaning |
|---|---|---|
| `volume` | 1.0 | The volume of all sound. 1.0: as loud as the files are. 0.0: silent |
| `output` | true | False: the game opens no sound device and does the same work with no sound (the benchmark, a test). With no device that can be opened, the game logs a warning and runs the same way |

`[editor]` (`crates/editor`, in the editor program only):

| Key | Default | Meaning |
|---|---|---|
| `snap` | true | A drag of a gizmo handle snaps. Ctrl turns it around for one drag |
| `grid_step` | 0.25 | Step of a position, in meters. Also the grid for a new item |
| `angle_step` | 15.0 | Step of a rotation, in degrees |
| `code_editor` | `code -g "{file}:{line}"` | The command that opens a file at a line in the code editor (Visual Studio Code). `{file}` and `{line}` are replaced; the first word is the program that opens the project folder |
| `blender` | not set | The program file of Blender, for the `.blend` and `.obj` files of the asset folder. Not set: `blender` of the path of the system, then the newest one in `Program Files\Blender Foundation` |

`[ui]` (`crates/ui`):

| Key | Default | Meaning |
|---|---|---|
| `scale` | 1.0 | Size of the text and the panels, 0.5 to 3.0, multiplied with the scale of the display. 1.5 on a display at 100 % shows as a display at 150 % does |

`[time]` (`crates/core`):

| Key | Default | Meaning |
|---|---|---|
| `fixed_hz` | 60.0 | Rate of the simulation steps (`FixedUpdate`), in steps per second |
| `max_fixed_steps` | 8 | Limit of simulation steps in one frame |

## Picture

The picture is path traced. Each frame traces one random light path for each pixel. The path starts with a camera ray. What happens at a surface depends on the material of the mesh:

| Material | The ray |
|---|---|
| Diffuse | One shadow ray goes to a random point of the sphere light (direct light, soft shadows). Then one bounce ray goes in a random direction; the path ends at the second diffuse surface (the light that fills the shadows). |
| Mirror | Continues in the mirror direction. |
| Glass | Glass that the camera sees (also in a mirror) splits the path: one path goes through and bends, one is reflected, each with its part of the light from the Fresnel law. Other glass on a path: the ray takes one of the two ways at random. |
| Metal (roughness) | If the pixel shows the surface: the soft reflection of many small mirrors (GGX). One shadow ray gives the bright spot of a light, and the path continues in a reflected direction; the two share the light of a lamp by their chances, so a small lamp on a smooth surface and a large one on a rough surface both have little noise. The base color tints the reflection. Later on a path: as diffuse. |
| Plastic (roughness) | A diffuse surface under a clear rough coat: the reflection of metal, with no tint and some percent of the light (more at a flat angle), plus the diffuse light. |

A path has 12 rays at most. The base color of the mesh is the color of a diffuse surface and the tint of a mirror or of glass. The light shows as a white sphere, also in a mirror and through glass.

A mesh made in code gets its material with `Mesh::with_material`. A mesh from a glTF file gets it from the numbers of its glTF material:

A mesh can have a light of its own (`Mesh::with_emissive`, `look: (emissive: (r, g, b))` in a room file, the emissive factor of a glTF material): it shows with that color, and bounce rays that find it bring its light to the room. No shadow ray aims at it, so it gives soft, noisy light: for a strong light use a `Light`.

A `Light` is a sphere. With `direction` and `cone` (degrees from the direction to the edge) in the room file it sends its light only in that cone: a flashlight. `game/assets/rooms/room_c.ron` shows the rough materials, a mesh with its own light, and a cone light (`start_room` of the config).

| glTF material | Material |
|---|---|
| Transmission factor above 0.5 (`KHR_materials_transmission`) | Glass. The index of refraction is the `ior` of `KHR_materials_ior`, or 1.5 |
| Else: roughness factor 0.9 or more (also the glTF default, a rough metal) | Diffuse |
| Else: metallic factor 0.5 or more | Mirror with a roughness factor of 0.1 or less, else Metal with that roughness |
| All others | Plastic with that roughness (0.05 at least) |

### Textures

A material can have a color texture and a texture for its own light (emissive). The color and the emissive color of the material multiply them.

- From a glTF file: the base color texture and the emissive texture of each material, with the first set of texture coordinates. The filter is that of the glTF sampler (`magFilter` nearest: pixel art). Alpha mode `MASK` gives a texture with holes: where the alpha is below the cutoff there is no surface, for the camera, for shadows, and for bounce light. `BLEND` shows as `MASK` with the cutoff 0.5. Not read: metallic, roughness, normal, and occlusion textures, texture transforms, other sets of texture coordinates, wrap modes (a texture always repeats).
- From a room file: `floor` and `walls` of the room, and `texture` in the `look` of a prop or a mover: `(path: "bricks.png", scale: 2.0, filter: Nearest)`. The file is a PNG or JPEG, from the directory of the room file. The texture coordinates of the room box, a cube, and a cuboid are meters, so a tile keeps its size in a room of any size; `scale` multiplies them. A sphere and a capsule have the texture one time around.
- An image file that two materials, two models, or a room and a model name is loaded one time. A room change removes the textures that no mesh uses any more (`maki_assets::Textures`).
- On the GPU a texture is an sRGB image with mip levels (`crates/render/src/textures.rs`), made at load. The path tracer selects the mip level from the size of the piece of surface that a ray stands for (a ray cone): one pixel of the picture for a camera ray, also behind a mirror and with a render scale below 1; a wide piece after a diffuse bounce. `texture_bias` moves the level.
- The surface color image of the denoiser (`albedo`) has the texture, so the filter does not blur it.
- The camera sees a surface with one side (a leaf card) from the two sides. Only the room box is not seen from behind.
- The trace shader exists in three forms: with no texture code, with textures, and with textures that have holes. A frame runs the smallest one that its meshes need. Measured in `test_1080p`: the texture code costs 1.9 % of the trace pass, the holes 3.4 % more, for all rays of the scene (`docs/part4/stage-25.md`).

`python tools/gen_textured.py DIR` writes test textures and a textured model. The benchmark scenarios `textured_720p`, `textured_1080p`, and `textured_walk_720p` use them; after one run, the room is in the Stages panel.

Known limit: light that reaches a diffuse surface by way of a mirror or through glass (a caustic) is too dark. The shadow ray finds only light that comes straight from the light, so such light comes from a bounce ray that finds the light by chance: a rare, very bright sample. `indirect_clamp` limits it. With `indirect_clamp = 0` the light is correct in the mean, and shows as bright speckle that goes away very slowly.

One frame is noisy. The denoiser makes the picture from the frames (`crates/render/src/denoise.rs`). The path tracer gives the light of each pixel in parts, and the picture is `surface color * (direct + indirect) + specular + emission`:

- Direct light: straight from the lights. Little noise, sharp shadows.
- Indirect light: by way of a bounce. Much noise, soft.
- Specular light: the reflection on glass.
- Emission: a lamp that the pixel shows (also in a mirror, through glass, or as a reflection on glass). No noise: it does not go through the denoiser.

For a pixel that shows a mirror or glass, the surface is the one that shows there: the denoiser treats the room in a mirror as it treats the room.

The steps:

1. The mean over time. Each pixel keeps the mean of its samples, up to `max_samples`. The renderer knows where the surface point of each pixel was on the screen one frame before (the motion image), so the mean follows a moving object and a moving camera. Where the frame before showed another surface, the pixel starts again.
2. The filter, for the direct light (`denoise_direct_passes`) and for the indirect light (`denoise_passes`), each alone: the noise of the bounce light does not blur the shadow edges of the direct light. Each pixel gets the mean of the pixels around it that show the same surface (equal normal, on the same plane) with a similar brightness. The less noise a pixel has left, the less the filter does. The filter works on the light without the surface color, so surface detail stays sharp. The specular light gets the mean over time only.

A change of the camera keeps the samples of each pixel. A change of the lighting (a light, an object that moves and so its shadow) is handled for each kind of light. Direct light: the denoiser tests each pixel (`history_clamp`), so only the pixels where the light changed start again, and a shadow follows its object in some frames. Indirect light: its noise is too large for such a test at one sample for each frame, so its mean is cut to `light_history_samples` in the full picture for some frames. A change of the internal resolution, the window size, the room, or the mesh store starts the picture again, and so does R. While nothing changes and each pixel has `max_samples`, only the tonemap pass and the copy to the window run.

3. Anti-aliasing and scaling (`taa`, `crates/render/src/taa.rs`). The trace pass moves all camera rays of a frame by one part of a pixel. The pass `taa` keeps the mean of the pictures of some frames at the window resolution: an edge gets smooth, also that of the lamp and of the background, which have no mean in the denoiser. With `render_scale` below 1 it puts each sample at its true place in the window grid. A point that moves on the screen takes more of the new frame, and its history counts only if it shows the same surface. This pass is the one step between the internal resolution and the window resolution: an upscaler (FSR, XeSS, MetalFX, DLSS) takes its place with the same inputs.
4. The look (`bloom`, `tonemap`): a soft glow around the bright parts (`bloom`), darker corners (`vignette`), the exposure and the tone curve (`tone_curve`), the color grade, the sRGB curve, and dither: noise of one 8-bit step, so a dark gradient shows no bands.

The camera rays do not hit the back side of the room box, so the camera looks into the room through the near walls. For all other rays the walls are solid. Each other mesh shows from the two sides.

## Rooms and events

A room is a RON file in `game/assets/rooms`: the size of the room, its doorways, props, lights, doors, movers, spawn points, and events. `RoomDef` in `crates/scene/src/room.rs` is the format of the room, `EventDef` in `crates/events/src/model.rs` that of an event. A change of a room file needs no build: start the game again, or press F5. The editor (above) writes the same files.

- A bad room file: the error names the file, the line, and the column. A bad start room stops the game at start; a transfer into a bad room does not happen.
- The room box, a cube, a cuboid, and a sphere can have a texture: see "Textures" above.
- A generated mesh (the walls, a cube, a sphere) belongs to its room and goes away with it. A room that is loaded again (F5, each change in the editor) keeps each mesh that is made from the same numbers, and makes only the others: a new color of one prop is one new mesh.
- A new mesh does not make the game wait for the GPU: its acceleration structure is built in the next frame. A glTF file (`File("path")`, from the directory of the room file) goes away with its room too, if the next room does not name it.
- A light can have a sound (`sound: (file: "sounds/hum.wav", volume: 0.5, reach: 12.0, on: true)`): the file plays as a loop at the place of the light, louder near the player, silent beyond `reach` meters, from the side where the picture shows the light. It starts and stops with a fade of half a second. `SoundOn` and `SoundOff` find it by the name of the light.
- A sound file is WAV or Ogg Vorbis, from the directory of the room file. The two files of the game (`game/assets/rooms/sounds`) are placeholders that `python tools/gen_sounds.py` makes.
- The player, the camera, the game state (flags and variables), and the music are not part of a room: they stay when the room changes.

An event has pages. A page has conditions, a trigger, and commands. The active page is the last one whose conditions all hold. An event with no `area` has no place: for `Autorun` and `Parallel`.

| Condition | Holds when |
|---|---|
| `Flag("name")`, `NotFlag("name")` | the flag is set, is not set |
| `Variable("name", AtLeast, 3)` | the variable compares so with the number: `Equals`, `NotEquals`, `Less`, `AtLeast` |

| Trigger | The commands start when |
|---|---|
| `Interact` | the player is at the area of the event, faces it, and presses interact |
| `Touch` | the player walks into the area of the event |
| `Autorun` | the page becomes the active page (also when the room loads). One time. The player has no control while it runs |
| `Parallel` | as `Autorun`, and again each time the commands end. The player keeps control |

| Command | Effect |
|---|---|
| `SetFlag("name", true)`, `SetVariable("name", 3)`, `AddVariable("name", 1)` | Change the game state |
| `Wait(0.5)` | Seconds |
| `If([Flag("name"), Variable("count", Equals, 3)], [...], [...])` | The first list if all conditions hold, else the second one |
| `OpenDoor("name")`, `CloseDoor("name")` | Send a door or a mover to its other pose, and wait until it is there |
| `LightOn("name")`, `LightOff("name")`, `LightIntensity("name", 4.0)` | Change a light of the room |
| `Message("text")` | Show the text until the player presses interact |
| `Choice("question", [("Yes", [...]), ("No", [...])])` | Show the question and the answers. The player moves the mark (up, down) and presses interact; the list of that answer runs, then the commands after the question |
| `FadeOut(0.4)`, `FadeIn(0.4)` | To black and back, in seconds |
| `Transfer("room_b.ron", "west")` | To a spawn point of another room. The event goes on in the new room |
| `PlaySound("sounds/click.wav", 1.0)` | Play a sound file one time, with this volume (1.0: as loud as the file). The event does not wait for its end. A command of the audio module (`crates/audio`), as the next four |
| `PlayMusic("sounds/theme.ogg", 0.8)` | Start a file as music: a loop, read from the file while it plays, in place of the music that plays. The file of the music that plays: no change |
| `StopMusic` | The music fades out in one second |
| `SoundOn("name")`, `SoundOff("name")` | Turn the sound of a light of the room on or off |
| `ShakeCamera(0.6, 0.06)` | A command of the game, not of the engine (`game/scripts/src/shake.rs`): shakes the camera for this many seconds, by at most this many meters |

The interpreter runs the commands in fixed steps and never stops the frame loop: a command that waits is tried again in the next step.

The commands are an open set. A module writes its commands as one enum with the serde derives and adds it, with the function that runs one step of a command, to `maki_events::CommandRegistry` in `Module::build`. The room file then names the variants as it names the built-in ones; a name that no module added, or a bad argument, is an error with the file and the line when the room loads. The built-in commands are added the same way (`crates/events/src/builtin.rs`). `game/scripts/src/shake.rs` is the example. A command that holds lists of commands (`If`, `Choice`) tells so with `CommandRegistry::add_lists`: the checks of the loader and the event panel of the editor then walk and show the lists of each module in the same way, with the label of each list as a row (`Else`, `Answer: Yes`).

The loader checks the events of a room file (`crates/events/src/check.rs`). An error has the file and the line of the command, and the room does not load: a light, a door, or a mover that the room does not have, a `SoundOn` or `SoundOff` for a light with no sound, and two events with one name. A warning is logged, and the room loads: a `Transfer` to a room file or a spawn point that does not exist, a sound file that does not exist. The commands of a page after a `Transfer` run in the other room and are not checked. A module adds the check of its own commands with `CommandRegistry::add_check`.

`python tools/convert_rooms.py FILES` converts a room file of before stage 29 (`If("flag", ..)`, `VariableAtLeast`, `VariableEquals`, an area of size 0). Comments stay.

## Crates

Dependencies flow in one direction: each crate can use only the crates above it in this list.

| Crate | Role |
|---|---|
| `crates/macros` | The derive macros of the engine (`Fields`). Code of the build: `crates/core` shows them under its own name, and no other crate names this one |
| `crates/core` | Config file and the settings trait, app and schedules, time, transform, interpolation between fixed steps. Fields that describe themselves (`fields`): the derive `Fields` gives a type the table of its fields and its value as a tree; a module gives a type a name, and a tool finds the table by that name |
| `crates/platform` | Window, input actions (an open set: a module adds its own, with their keys and mouse buttons), input modes (the bindings of the game, of the editor), the pointer, input capture |
| `crates/assets` | Asset stores and handles, mesh asset with its material and texture coordinates, texture asset and its store, glTF loader, mesh generators |
| `crates/scene` | The components that describe the world (mesh instance, light, sound source, camera, player, collider, mover, door), collision, the room file format, load and unload of rooms |
| `crates/events` | Game state (flags, variables), event definitions, the event interpreter with its step mode, the command registry and the built-in commands |
| `crates/audio` | The sound output (`kira`; no type of it in the public API), sound assets with handles, music from a stream, the sounds at a place in the room with the player as the listener, the audio event commands. With no device it does the same work with no sound |
| `crates/gameplay` | Gameplay that games of this kind share, each a module with its own settings: camera that follows the player, walking controller, start room and reload, move actions and quit, debug control of a light and of the movers |
| `crates/gpu` | Vulkan backend (resources, images with mip levels, bindless set, acceleration structures, compute dispatch, 2D draw calls); the only crate that shows Vulkan types |
| `crates/render` | Render world, frame graph, render passes (path tracer, denoiser, anti-aliasing, bloom, tonemap), extraction of camera, lights, and mesh instances, acceleration structures, motion vectors |
| `crates/ui` | egui integration, panel registry, the overlay render pass; the help, stats, and renderer panels; the text box of the game (message, question) and the game state panel of the events. The shell of a tool: docks with tabs, the layout file, the command list with its menus, keys, and search, the widgets (property grid, tree, tool button, notice), the theme table |
| `crates/devtools` | Tools that a game opts into: benchmark runner with its command line options and panel, list of the room files, panel of the gameplay settings |
| `crates/editor` | The room editor: the document (room, events, comments), the writer of room files, the free camera, the panels (outliner, inspector, events) and the commands of the editor in the shell of `crates/ui`, the pick, the gizmo, the marks on the picture, add, delete, duplicate, names, undo and redo, the event panel, the checks of the events |
| `crates/engine` | The engine as one crate, the only one that a project names: the engine crates by their short names, the project file, the module list of a game and of the editor, the command line, the log setup, the two entry functions, the loader of the script DLL |
| `crates/engine/dylib` | All engine crates as one DLL, for the dynamic build |
| `programs/editor` | The editor program `maki-editor`, and the reload test of the loader |
| `game` | The project of this repository: `project.toml`, the rooms (`assets/rooms`), the script crate (`scripts`: the `ShakeCamera` command, the benchmark scripts), and the program of the game |
