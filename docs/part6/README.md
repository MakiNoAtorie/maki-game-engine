# Part 6: plan (draft)

State: in discussion with the owner. Stages 41 to 47 are done; the object model is stages 47 to 53 (2.14). This file holds what is decided.

## 1. Direction

The engine is a general engine in which a game in the style of RPG Maker can be made. It is not a copy of the RPG Maker workflow. Parts 4 and 5 made the event (pages, triggers, command rows) the only way to write logic, and the item kinds of a room a closed set. Part 6 corrects the two.

## 2. Decided

### 2.1 Object model (approved 2026-10-12)

The model of engines made for one game (typed objects, placed and tuned in the editor), not free composition as in Unity, and no node tree as in Godot.

- An object is a name, a pose, and a list of components.
- A component is a Rust type that a module registers, as a module registers its event commands today.
- A room file lists objects. The lists `props`, `lights`, `doors`, and the others go.
- A kind is a prefab: "player", "door", "lamp" are prefab files with their components and default values. The player is data, not a special case.
- A script is one more component, with hooks: start, step, interact, enter area.
- One set of commands: each registered event command is also a function that a script calls. Events and the command rows of the editor stay, as a shortcut over the same functions.
- Objects are flat, with an optional parent.
- The properties panel shows the components of the selected object, each from its registered type.

Cost: a new room file format with a migration of the rooms, and the item kinds of the editor (outliner, properties, pick, gizmo) made generic.

### 2.2 Editor and game are two things (owner, 2026-10-12)

As in Unreal and Godot: the editor is a program of its own that opens a project. It is not a mode of the game. Scripts are Rust; a script change must not need the editor to close. Rule 8 of `CLAUDE.md` gets new words.

### 2.3 How the editor gets the Rust code of a project (proven in stage 41)

- `maki-editor.exe`: engine and editor, no code of a game.
- A project: a folder with `assets/` and one Rust crate of scripts (components, behaviours, commands of that game).
- Development: the engine crates build as one DLL (`maki-engine`), the script crate as a DLL against it, with the profile `dynamic`. The editor loads a copy of the script DLL. After a change it builds the DLL (1 s) and loads the new copy when no game runs, into a new app. An old copy is not unloaded.
- Export: one program with the engine and the script crate linked statically, no editor.
- A change of an engine crate needs a new start of the editor: the engine DLL is in use.

Report and limits: `stage-41.md`.

### 2.4 No code editor in the engine (owner, 2026-10-12)

Scripts are written in an external code editor; the default is VS Code. The engine editor has the glue:

- "Open in editor": the file of a script or a component, at its line (`code -g file:line`). The command is a setting.
- The errors of a build of the script crate in a panel; a click opens the file at the line.
- A build and a load after a script file is saved.
- "New script" from a template.
- A log panel with the log lines of the scripts while a game runs.

### 2.5 A changed asset file shows at once (owner, 2026-10-12)

The owner saves a texture, a model, or a sound in another program, and the editor has the new file with no restart: by itself, and by a menu item. Today a file is read one time, and only a restart (or a trip to a room that does not use it) reads it again.

- The way: the store forgets a changed file and the room loads again (`stage-45-asset-reload.md`).
- Source files (`.blend`, `.obj`, `.psd`): the editor converts them to a glTF or PNG file next to them; the engine reads only those (`stage-46-source-files.md`).

### 2.6 Animation in steps (owner, 2026-10-12)

The animation system (skinned characters, and the timeline) has a mode with no interpolation, for the look of drawn animation (Arc System Works):

- A key holds its value until the next key. A track or a clip has the mode `Step`; glTF has it as the sampler mode `STEP`, and the loader keeps it.
- A clip can have a frame rate of its own (for example 12 or 8 in a second): the time of the clip is cut to that rate, so a clip with curves plays in steps too ("on twos", "on threes").
- A change from one clip to another can be a cut, with no blend.
- The renderer: a pose that jumps is a change of each pixel of the character. The denoiser and the anti-aliasing must not smear the pose before into the new one. This is a gate of the animation stage.

### 2.7 Templates of "New script" (owner, 2026-10-12)

"New script" of the editor asks for a name and a template, writes the file into the script crate, and opens it in the code editor (2.4).

- A template is a `.rs` file with a place for the name, in a folder of the engine. A folder `templates/` of a project adds its own to the list.
- Each template builds as it is, and has short comments on what to change.
- A template that needs a feature comes with the stage of that feature.

| Template | Starts with | Needs |
|---|---|---|
| Empty | The component, its fields, the empty hooks (start, step, interact, enter area) | Object model |
| Interactable | On interact: a message, and a flag for "one time only" | Object model |
| Trigger area | On enter and on leave of an area, with "one time only" | Object model |
| Pickup | On interact: add an item, remove the object, keep that it was taken | Inventory (item 7 of 3.2) |
| Door | Open, closed, locked; a key item; a sound for each change | Object model; the key: inventory |
| Movement | Moves the object from input actions; the speed is a field | Object model |
| Patrol | Walks between points and waits at each | Object model |
| Chaser | Rest, see the player (distance, line of sight), follow, give up | Path search and line of sight (item 8) |
| Talker | On interact: lines and choices | Object model |
| State machine | An enum of states, with enter, step, and leave for each | Object model |
| Flickering light | Changes the strength of a light by a pattern or by chance | Object model |
| Mover | Moves or turns an object between two poses on a signal | Wires (item 3) |
| Sound emitter | Plays a sound at its place on a signal or at random times | Wires (item 3) |
| Timer | Waits, then sends a signal; one time or again | Wires (item 3) |
| Cutscene trigger | Takes the control from the player, plays a timeline, gives it back | Timeline (item 6) |
| Event command | A command for the command rows: handler, row text, check (as `shake.rs`) | Stage 43 |
| Engine module | A module with a settings table, a resource, and a system | Stage 43 |

### 2.8 Events stay, on the object model (owner, 2026-10-12)

The events (pages, triggers, command rows, flags and variables, the text box, choices) stay as the way to make logic with no code. They are made again on the object model: one system, two ways to use it. As they are, they would be a second system next to the scripts (two ways for "on interact", two kinds of area, two lists in a room, rows that cannot reach a component of a project).

One system:

- An event is a component of an object, as a script is. Its area is the trigger area of the object. Its triggers are the hooks of a script (interact, enter, start, step).
- One set of commands, two callers (2.1): a command is a function for a script, and a script can run a command list.
- Flags and variables are the state of the game: rows and scripts use the same store, and the save file holds it.
- A wire (item 3 of 3.2) can start a command list.

Rows with no wall. Three general commands:

| Command | What it does | Needs |
|---|---|---|
| Set a field | Changes a field of a component of an object | Item 1 of 3.2 |
| Send a signal | Starts a wire | Item 3 of 3.2 |
| Call a script function | One row calls Rust code of the project | Stage 43 |

What the events lack (from `OPEN_ITEMS.md` and the stage reports; check each against the command set when the stage is planned):

| Gap | What it is |
|---|---|
| Command lists for many users | A list in a file of its own, called from rows and from scripts ("common events") |
| State of one object | A flag that is of one object ("this chest is open"), with no name in the global list ("self switches") |
| Flow | "Or" and "else if" in conditions, loops, wait |
| Numbers | Add, subtract, a random number, compare two variables |
| Autorun again | A page that runs again until its condition changes; now it runs one time |
| Any object as a target | Move, show, hide, animate an object by its name; now lights, doors, and movers only |

Later, if the owner wants it: "Convert to script" writes the Rust code of a command list.

### 2.9 Physics, particles, fog, fluids (owner, 2026-10-12)

Now: collision with boxes, and the walking controller. Nothing falls or is pushed.

- **Physics crate** (`physics`), on the library Rapier: bodies that fall, hit, and are pushed; joints; ray and shape queries; a character controller; trigger areas. No type of Rapier in its public API. It runs in `FixedUpdate`. Check the current version and API of Rapier before use. A body and a collider are components: the stage is with the object model or after it.
  - It gives the line of sight of item 8, the trigger area of 2.8, and it closes "Open door leaf" of `OPEN_ITEMS.md`.
  - Watch: each object that moves changes the TLAS in its frame. Measure a room with many bodies.
- **Particles**, two kinds: small meshes in the ray-traced scene (sparks, debris: with light and shadow), and flat sprites in a pass after the path tracer (smoke, steam, spray: cheap, with rough light).
- **Fog volumes** in the path tracer (fog, haze, light shafts). A render stage with a gate on the GPU time: the heavy scene is at 7.3 ms of 8 ms now.
- **Fluids are not simulated.** Water is a flat surface with ripples (needs normal textures: `OPEN_ITEMS.md`, "Material textures"); blood and puddles are decals and particles; a thing that floats gets a force from the physics crate.

Not planned: fluid simulation, cloth, soft bodies, destruction.

### 2.10 Lights, effects, post effects (owner, 2026-10-12)

Now:

- Lights: a sphere light and a cone light, and meshes that glow. The editor places them and shows the path-traced picture: no bake step. An event can switch a light.
- Passes: trace (three forms), denoise, anti-aliasing, bloom, tone map (exposure, tone curve, vignette), UI, in a frame graph. A pass is Rust code (`FrameGraph::add_pass`, `add_pass_before`); its shader is compiled with the engine (`crates/render/build.rs`).
- Post effects as settings: exposure, tone curve, bloom, vignette, one value for the whole game. Camera shake.
- No particle effect, no effect file, no effect editor, no decal, no shader of a project.

In the plan:

| # | Item | What it is |
|---|---|---|
| 19 | Post effects as files of a project | A shader file and the list of its settings. The editor compiles it with `glslc` (a converter, as in 2.5) and loads it again after a save. The game that ships has the compiled file and no compiler |
| 20 | Effect stack | A list of post effects with their order and values, for a room or an area, as a component; the values blend between areas; scripts and the timeline change them. The stack shows the GPU time of each effect |
| 21 | Stock post effects | On 19 and 20. Film and lens: grain, chromatic aberration, lens distortion, depth of field. Color: color grading, sharpen. Old looks: pixels with dither, scanlines and tube curve, tape noise. From gameplay: fade, flash, tint (commands for rows and scripts) |
| 22 | Helper images for post effects | The normals, the distance, and the base color of the renderer as inputs of a post effect: outlines, fog by distance |
| 23 | Load of the shaders of the engine while it runs | A change of a built-in pass with no restart |
| 24 | Effect files and effect editor | A particle effect is a data file: emitters, rates, lifetimes, colors over time, the sprite or the mesh. A panel with a live picture. An effect is placed in a room as an object. On the particles of 2.9 |
| 25 | Decals | Blood, cracks, stains, writing on a surface |
| 26 | Light animation | Flicker, pulse, fade as fields of a light. Color by temperature. Light presets |
| 27 | More light kinds | A rectangle light (window, screen, panel). A mesh that glows as a real light source (`OPEN_ITEMS.md`, "Light of a mesh") |
| 28 | Baked lighting | An option of a room, for this game or a later one, not only a rescue of the budget. See below |

Baked lighting (item 28; owner, 2026-10-12): a room chooses how its light is made.

| Mode | Direct light | Bounce light |
|---|---|---|
| Live (now) | Path traced | Path traced |
| Baked bounce | Path traced, with its shadows: a light still moves and switches | From the bake |

- The bounce light is the part with the most noise and cost: its rays, and the denoiser passes of `indirect`. The mode "baked bounce" takes them out of the frame.
- The baker is the path tracer of the engine, with many samples, run by the editor. No second renderer.
- Where the bake is kept: a grid of light probes in the room (no second set of texture coordinates; it lights things that move too; soft detail), or light maps (sharp detail; each mesh needs texture coordinates for it). Proposal: probes first.
- What the bake does not follow: the bounce of a light that switches or moves, and a door that opens. To decide in the stage: a bake for each state that an event can set, or live bounce for the lights that are marked "moves".
- Gate of the stage: a room in the mode "baked bounce" against the same room live: the picture difference (`relmse`), and the GPU time.

Not planned: surface shaders as free code. A surface of the path tracer must keep the rules of light, or the denoiser and the bounce light fail. The way: more texture inputs (normal, roughness, glow: `OPEN_ITEMS.md`, "Material textures") and a new built-in surface kind when a look needs one.

Each post effect is measured: the heavy scene is at 7.3 ms of 8 ms.

### 2.11 Asset browser and drag and drop (owner, 2026-10-12)

Now: file lists with small pictures where a field needs a file, "Add > Model file", "File > Open". No panel with the files of the project. No reaction to a file that is dropped on the window. No file operation in the editor.

Item 13 of 3.2, in full:

- **The panel**: the folder tree and a grid of the files, with small pictures, a search field, and a filter by kind.
- **A drag into the picture**: a model is a prop at the place of the drop, on the floor and on the grid; a prefab is an instance; a sound is a sound emitter; a texture goes on the surface under the pointer.
- **A drag onto a field** of the inspector: a file on a texture, model, or sound field.
- **A drag from Windows**: a file that is dropped on the editor is copied into the folder that the panel shows. A source file (`.blend`, `.obj`, `.psd`) then goes to its converter (stage 46).
- **File operations**: new folder, new name, move, copy, delete (with the list of the rooms that use the file), "show in Explorer". A new name and a move need item 12: the rooms that use the file stay correct.
- **Double click**: a room opens in the editor; a script opens in the code editor (2.4); a prefab opens for a change.

In two parts:

| Part | Content | Needs |
|---|---|---|
| A | The panel, search and filter, the drag of a model and of a prefab into the picture, the drag from Windows, the file operations with item 12, double click | Stage 43 |
| B | The drag of a texture onto a surface, of a sound into the picture, and of a file onto a field of the inspector | Object model |

### 2.12 Log, console, and debug tools of the editor (owner, 2026-10-12)

Now: the log goes to the terminal of the program (`MAKI_LOG`). The editor has a status row with the last error and the count of the problems of the events, marks on the command rows, and the debug panels (Stats, Debug, Gameplay, Game state, Bench, Stages). A game in the editor has stop, steps of the events, and the name of the event that runs. Tracy by a build feature.

In the plan:

| # | Item | What it is | Needs |
|---|---|---|---|
| 29 | Log panel | Each log line in the editor: filters by level and by source, search, equal lines as one with a count, empty at "Play". A click opens the script at its line (2.4), or selects the object or the command row. This is the log panel of 2.4, in full | Stage 43 |
| 30 | Command console | A text field in the log panel. A module and a script crate add their commands to a registry, as settings and event commands are added: set a flag, go to a room, give an item, load again | Stage 43 |
| 31 | An error of a script does not end the editor | A panic in the code of a project is caught: the game stops, the error shows with its place, the editor stays. The world of that game is dropped and made again from the document | Stage 44 |
| 32 | Log file | In the project folder; the file of the session before is kept | Stage 43 |
| 33 | Pause and one frame | For the game in the editor: pause, one frame, "pause at an error" | Stage 43 |
| 34 | Debug shapes | Lines, boxes, and labels in the picture of the game, from a script (`OPEN_ITEMS.md`, "Graphics pipeline state") | Object model |

Also here: the errors of a build in a panel (stage 44), and the values of a game that runs (item 14).

### 2.13 The shell of the editor comes first (owner, 2026-10-12)

The plan adds many panels. Before the first of them, the editor gets a layout that can hold them (`stage-42-editor-shell.md`). It adds no feature.

Rules, from the guidelines of Blender and the editors of Unity, Unreal, Godot, and Unigine:

1. The picture has the most room, and no panel is over it.
2. Nothing stops the work: no window that must be answered, but for a thing that cannot be undone. A long job (a build, an import, a bake) runs behind and shows in the status row.
3. One mode: a game that runs. It is marked by the color of the frame of the picture.
4. Each panel is a tab of a dock. The layout is saved, can be reset, and has named forms.
5. The places that the other engines have: objects left, inspector right, files and log at the bottom, play in the middle of the tool row.
6. One widget for one job: one property grid, one tree, one picker, one tool button.

The default layout, and where each later panel goes:

| Dock | Now | Later |
|---|---|---|
| Top | Menu (File, Edit, View), tool row | Menus Assets, Project, Window; command search; pause and one frame (item 33) |
| Left | Outliner | Rooms of the project |
| Middle (documents) | The room | Prefab, particle effect (item 24), timeline documents |
| Right | Inspector | Look: the effect stack (item 20) |
| Bottom | Events; Stats, Debug, Gameplay, Game state, Bench | Assets (2.11), Log and console (items 29, 30), Build errors (stage 44), Timeline (item 6) |
| Status | Last message, count of problems | Jobs that run; frame time |

Docks: the crate `egui_dock`. Not in the shell stage: a tab with no content, a panel in a second window of the OS, the log panel.

### 2.14 How the object model is built (owner, 2026-10-12)

The model is 2.1. This is how it is built, from what Unreal, Unity, Godot, and Bevy have in common.

What they have in common:

- One description of a field, many users: the inspector, the file, the undo, and the changes of a prefab instance read the same list (`UPROPERTY`, the serialized fields of Unity, `@export`, `Reflect`).
- The editor changes the saved form, not the live objects. Play makes a copy. The editor has this since part 5 (the document, then the world).
- A component that the code does not have any more stays as data ("missing script" of Unity). A script DLL that does not build must not lose the values of a room.
- An instance of a prefab stores only what it changed.

The build:

- An object is an entity of `bevy_ecs`, and a component of an object is a component of `bevy_ecs`. No second layer at run time.
- In the document and in the file, a component is its name and a tree of values (field name to value), not a Rust type. The undo, the changes of a prefab instance, and a component with no code are operations on the tree, written one time.
- A component type registers three things: its name, its table of fields, and the function that builds it from the tree.
- The table of fields comes from a derive of the engine, on top of `serde`: a proc-macro crate of the workspace (`syn` and `quote` are in the tree). Stage 47 built it: the tree is `serde_json::Value` (in the tree by `gltf`), the derive writes the tree, and `serde` reads it back. A field has a kind (number, switch, text, vector, color, choice, file, object, list, group) and its hints (range, step, kind of file). Not `bevy_reflect`: it is a new dependency, more general than the inspector needs, and the engine DLL is at 56,599 of its 65,535 symbols.
- A script is a component with a trait for the hooks (start, step, interact, enter area). The engine runs one system for each registered type.
- A wire is as in Source: a component names its outputs, and an input is a registered command. Wires come after the hooks.

The order is data, then behavior, then the way with no code. Each stage leaves the game as it runs, with the benchmark against the stage before. The stages are in 3.1.

### 2.15 The plan after the object model comes from a vertical slice (owner, 2026-10-16)

The owner makes a vertical slice of a game in the style of RPG Maker, in 3D, path-traced, with the step mode of 2.6. The stages build the engine and the tools with which the owner makes the slice; no stage makes the slice. Each system is made for a full game, not only for the slice.

The slice:

1. A title screen: new game, continue (the save), quit.
2. Three or four rooms with doors and a locked door.
3. A character to talk to: messages with choices, and a state of its own that changes after the talk.
4. Items to pick up, an inventory menu, the use of an item (a key on the door).
5. A short cutscene: the camera moves, a character walks and plays a clip, a light flickers.
6. An enemy that follows the player and loses the player out of its line of sight.
7. A game-over screen and an end screen.

Decided for it:

- **Movement**: free, and on a grid, as a setting of the walking module.
- **No battle system.** A battle system of a later game is not the RPG Maker system.
- **UI of the game**: egui behind a module of the game UI. Menus are data; a script and a command talk to the module, not to egui.
- **Text in tables (item 11)**: each text names a key into a table for each language, made for a full game.
- **All before the slice**: each item of 3.2 that the slice can use (physics, gamepad, particles, fog, water, post effects, decals, baked lighting) is built before the owner makes the slice, not after it.
- **Object state**: a component of any object, with named switches and numbers ("self switches"). Its values live in a resource, by room and object, so that they stay over a change of room and go into the save. Events, their conditions, and scripts read and write it.
- **Routine**: a command list in a file of its own (`.routine.ron`), with arguments, called from a row of an event and from a script. Not "common event".
- **Data asset**: a file of a type that a module registers with `#[derive(Fields)]` (as the ScriptableObject of Unity, the Resource of Godot). The inspector edits it; the asset browser lists it. Item kinds, text tables, and later the values of enemies are data assets; a game registers its own types in its script crate.
- **Animation**: one core of keys and tracks with the step mode of 2.6 first, then skinned characters (item 9), then the timeline (item 6). Light animation (item 26) is on the tracks of the timeline.
- **Grid**: a cell size for each room (1 m if not set). Movement on the grid, the path search of enemies (on a grid made from the collision of the room at its load), and the snap of the editor use it. A navigation mesh later, if a room needs it.
- **Object id**: each object has an id that does not change (a new name, a move, a copy). Object state, wires, the targets of events, and the parent name the id; the name is for people and for a search by name in a script. Same stage as files with an identity (item 12). `docs/adr/0001-object-ids.md`.
- **Physics (Rapier)**: `area` is made on a sensor of Rapier (the same component, outputs, and hooks); the walking controller is made on the character controller of Rapier (its settings, free and grid). One system of collision.
- **Pause of the editor (item 33)**: pause, one frame, and "pause at an error" use `Time::paused` of the editor, not `PlayState::Pause` of the game.
- **A script that panics (item 31)**: each hook runs in `catch_unwind`; a panic stops the play, logs the message with the file and line, and drops the world of the play. The editor stays open. The game program panics as before.
- **Dependencies**: `rapier3d` (physics), `gilrs` (gamepad). After the measure of the symbol limit.
- **The gaps of the events (group A), one stage**: object state as a condition and as a command; routines (`CallRoutine`); "or", "else if", a loop with a break, "wait until" a condition; a random number, a compare of two variables; an autorun that runs again while its condition holds (a flag of the page); show, hide, and move any object by its id. "Animate" comes with the timeline; "Convert to script" only when the owner asks.
- **Menus**: the module of the game UI has the widgets (message box, choices, a list of items, a stack of menus with focus). A menu is a data asset of the type `Menu`: its rows, and for each row a command, a routine, or a script function. The title, pause, inventory, and game-over screens are menus; a script can push a menu that it makes.
- **Baked lighting (item 28)**: first a way to get GPU time back (the gate: `relmse` and GPU time against the live room), last in the renderer lane. A look of its own is valid too: a room can choose the bake where the live light fits the budget.
- **First**: the measure of the symbol limit (`opt-level = 2` for the engine crates in the profile `dynamic`; a check in CI if the gain is large), and a stage for what the object model left (`PLANNING-HANDOVER.md`, group F).

## 3. Open

### 3.1 Stages

| Stage | Content | State |
|---|---|---|
| 41 | Spike: the Rust code of a project as a DLL | Done |
| 42 | The shell of the editor: docks, registries of panels and commands, widgets, theme (`stage-42-editor-shell.md`) | Done (`stage-42.md`) |
| 43 | The editor program, and the game as a project (`stage-43-editor-program-and-project.md`) | Done (`stage-43.md`) |
| 44 | The editor loads a new build of the script DLL while it runs: build after a save, the build errors in a panel, "Open in editor" (2.4) (`stage-44-script-reload.md`) | Done (`stage-44.md`) |
| 45 | A changed asset file shows at once (`stage-45-asset-reload.md`) | Done (`stage-45.md`) |
| 46 | Source files of other programs: `.blend`, `.obj`, `.psd` (`stage-46-source-files.md`) | Done (`stage-46.md`) |
| 47 | Fields that describe themselves (item 1 of 3.2): the derive, the registry of component types, the properties panel from the table of fields. On the item kinds as they are: no change of the room file (2.14) (`stage-47-fields.md`) | Done (`stage-47.md`) |
| 48 | The room file is a list of objects (2.1): the new format, the migration of the rooms, the outliner, the pick, and the gizmo for any object (`stage-48-objects.md`) | Done (`stage-48.md`) |
| 49 | Prefabs: a kind is a prefab file, the player is data, an instance with changes (item 2) (`stage-49-prefabs.md`) | Done (`stage-49.md`) |
| 50 | Script components with hooks, each command as a function of a script, "New script" with its templates (2.7), the log panel (2.4) (`stage-50-scripts.md`) | Done (`stage-50.md`) |
| 51 | The events as components, and the general commands set a field and call a script function (2.8) (`stage-51-events-as-components.md`) | Done (`stage-51.md`) |
| 52 | Wires between objects (item 3), and "Send a signal" (`stage-52-wires.md`) | Done (`stage-52.md`) |
| 53 | Who is the player (item 4), and the states of the game (item 5) (`stage-53-player-and-states.md`); merged before stage 52 (a parallel lane) | Done (`stage-53.md`) |
| 54 | What the object model left (`PLANNING-HANDOVER.md`, group F, and the fixes of the run by hand). Beside it, outside the stages: the measure of the symbol limit (2.15) | Planned |
| 55 | Object ids (`docs/adr/0001-object-ids.md`), and files with an identity (item 12) | Planned |
| 56 | The gaps of the events (2.8, 2.15): object state, routines, flow, numbers, autorun again, any object as a target | Planned |
| 57 | Data assets (2.15) (`stage-57-data-assets.md`) | Done (`stage-57.md`) |
| 58 | Text in tables (item 11) (`stage-58-text-tables.md`) | Done (`stage-58.md`) |
| 59 | The UI of the game (item 7): the module, menus as data assets (`stage-59-game-ui.md`) | Done (`stage-59.md`) |
| 60 | Physics on Rapier (item 15): `area` on sensors, the walking controller on the character controller | Planned |
| 61 | Movement on the grid, the path search, the line of sight (item 8) | Planned |
| 62 | The core of animation: keys, tracks, the step mode (2.6) | Planned |
| 63 | Skinned characters (item 9) | Planned |
| 64 | The timeline (item 6), and light animation (item 26) | Planned |
| 65 | Gamepad (item 10) | Planned |
| Editor lane, after 55 | A script that panics (31), pause and one frame (33), the log file (32), the console (30), the values of a game that runs (14), debug shapes (34), the asset browser (13). A stage number for each when it starts | Planned |
| Renderer lane, after 54 | Post effects (19 to 22), load of the shaders while the engine runs (23), particles and effect files (16, 24), fog (17), water (18), decals (25), more light kinds (27), baked lighting (28). A stage number for each when it starts; one lane runs the benchmark at a time | Planned |

All of it comes before the owner makes the vertical slice (2.15).

Stages 45 and 46 need stage 43 only. They can come before stage 44 or after the object model.

### 3.2 What other engines have and this one does not (owner, 2026-10-12: all in the plan)

From Unreal, Unity, Godot, and Bevy. "Not there" is from a search of the code, not a full check. A stage gets its number and its file when it is planned.

With the object model (stages 47 to 53; 2.14). Item 1 is stage 47: the others need it. Item 2 is stage 49, item 3 is stage 52, items 4 and 5 are stage 53.

| # | Item | What it is | Now |
|---|---|---|---|
| 1 | Fields that describe themselves | One mark on a field of a component: it shows in the inspector, is in the room file and in the save file, and has undo (`UPROPERTY`, `@export`, `Reflect`) | Panel code by hand for each item kind |
| 2 | Prefab with changes | An instance of a prefab changes some values; the others follow the prefab file | A prefab is placed as it is |
| 3 | Wires between objects | "When this button is pressed, open that door", set in the editor with no code (signals of Godot, outputs and inputs of Source) | Event rows |
| 4 | Who is the player | The thing in the world, what controls it, and the rules of the game are three things (the game framework of Unreal, small) | The player is a special case |
| 5 | States of the game | Title, play, pause, cutscene, end; the game changes between them | Play and stop of the editor only |

For a game (after the object model):

| # | Item | What it is | Now |
|---|---|---|---|
| 6 | Timeline | Tracks that change any field of any object over time, for a cutscene (AnimationPlayer, Timeline, Sequencer). With the step mode of 2.6 | Command lists |
| 7 | UI of the game | Menus, inventory, HUD | The text box; debug panels |
| 8 | Enemies that move | Path search, line of sight | Not there. Collision is boxes |
| 9 | Skinned characters | Skeleton and clips from glTF. With the step mode of 2.6 | Not there (`OPEN_ITEMS.md`) |
| 10 | Gamepad | Buttons and sticks as keys of the binding tables | Not there |
| 11 | Text in tables | A message names a key; the text is in a table for each language | Text in the room file |
| 15 | Physics | 2.9: the crate on Rapier | Boxes |
| 16 | Particles | 2.9: small meshes, and sprites | Not there |
| 17 | Fog volumes | 2.9: in the path tracer | Not there |
| 18 | Water | 2.9: no simulation | Not there |
| 19 to 28 | Lights, effects, post effects | 2.10 | See 2.10 |

For the editor (after stage 43, in any order):

| # | Item | What it is | Now |
|---|---|---|---|
| 12 | Files with an identity | A file that is moved or gets a new name does not break the rooms that use it: an id for each file, or a rename in the editor that corrects each use | Paths |
| 13 | Asset browser and drag and drop | 2.11 | File lists with small pictures |
| 14 | Values of a game that runs | An object is selected while the game plays; its values show and can be changed | Not there |
| 29 to 34 | Log, console, debug tools | 2.12 | See 2.12 |

What not to take from them:

- A fully general editor (Bevy has none after years): this one stays as narrow as the game of the owner lets it.
- Free composition with no kinds (Unity): 2.1 has kinds as prefabs.
- A load of new code while a game runs, with its state kept (Unreal): 2.3 loads between games only.

### 3.3 Build times

Measured 2026-10-12, debug build, `cargo build -p maki-game` after `touch` of one file, 3 runs each:

| File touched | No editor | `--features editor` |
|---|---|---|
| Nothing | 0.3 s | 0.3 s |
| `game/src/main.rs` | 2.5 to 2.7 s | 2.7 to 3.3 s |
| `crates/gameplay/src/lib.rs` | 3.4 s | 3.9 to 4.0 s |
| `crates/events/src/lib.rs` | 4.1 to 4.3 s | 4.8 to 5.3 s |

Not in these numbers: the start of the game after the build, and a real edit (a `touch` changes no text, so the compiler reuses more).
