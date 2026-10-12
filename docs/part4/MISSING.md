# After part 4: what is missing

Input for the plan of the next part. Each item is something that the work on the editor (stages 26 to 28) showed is missing. The order is the proposed order of implementation: what a person who makes rooms hits first, and what other items need.

Sources: `stage-26.md`, `stage-27.md`, `stage-28.md` (section 8), and `OPEN_ITEMS.md` (the row names are in quotes).

## Order

| # | Item | Size | Needs | Why at this place |
|---|---|---|---|---|
| 1 | Event checks in the room loader | Small | - | A wrong name loads today. Cheap, and all later content gets the check |
| 2 | Event format: `If` with a condition, more condition kinds | Small | - | A format change costs more with each room that exists |
| 3 | Editor window: view sized to the window, narrow panels | Medium | - | The window does not fit on a 1920 display at the default size |
| 4 | Key chords and saved editor settings | Small | - | Daily friction; a platform seam that other modules need too |
| 5 | Play signal for modules | Small | - | Needed by sound (7) and by save and load (11) |
| 6 | Message box with choices | Medium | 2 | The message window is a debug window; a dialogue needs answers |
| 7 | Sound | Large | 5 | Horror lives on sound; events are where sound starts |
| 8 | Marks with depth, missing gizmo handles | Medium | 3 | Layout of a full room |
| 9 | Reload cost: one mesh, not the room | Medium | - | Shows with the first model file from a modelling tool |
| 10 | Model and texture preview, model files in the editor | Medium | 9 | Real art |
| 11 | Save and load | Medium | 5 | A game longer than a vignette |
| 12 | Prefabs, copy of items between rooms | Medium | - | The same furniture in many rooms |
| 13 | Benchmark scripts that name places | Small | - | A changed room breaks `trip_720p` with no warning |
| 14 | The rest of the stage 28 gate | Small | - | Rooms A and B made again from empty rooms, by hand |

## 1. Event checks in the room loader

- **Now**: `maki_editor::problems` (`crates/editor/src/events.rs`) finds a light, door, or mover that the room does not have, a `Transfer` to a room file or spawn point that does not exist, and two events with one name. The loader (`maki_scene::check_room`, `RoomValidators`) makes none of these checks. A room file that a person wrote loads, and the event logs a warning when it runs. No one finds a flag that nothing sets.
- **Build**: move the checks into `events` as a `RoomValidator`. It needs the names of the room: the validator gets the text only, so it parses the room too, or `RoomValidator` gets the `RoomDef`. A module can add a check for its own commands (the editor knows only `Builtin`). In "Where used": mark a flag with reads and no write.
- **Decide**: error or warning at load. A `Transfer` to a missing file as an error stops a room that is made before its neighbor.
- **Done when**: `room_a.ron` with `OpenDoor("dor")` gives a message with the line at load; the editor shows the same text from the same code; the three room files of the game load.
- `OPEN_ITEMS.md`: "Checks that only the editor makes", "Events with the same name".

## 2. Event format

- **Now**: `If(flag, then, else)` tests one flag. A page condition is one of four kinds (`Flag`, `NotFlag`, `VariableAtLeast`, `VariableEquals`). No "less than", no "or", no "else if" but by an `If` in the second list. An event with no area is an area of size 0.
- **Build**: `If` takes a `Condition` (one widget in the editor for the two). Add `VariableLess` or a general comparison. `EventDef::area` as `Option<Area>`. A converter in code for the room files that exist (the pattern of `tools/convert_config.py`).
- **Decide**: how far. A list of conditions in `If` ("all hold") is one more step; an expression language is out.
- **Done when**: an event opens a door when a counter is 3, made in the editor with no text field; the old room files load after the conversion; `trip_720p` is byte-equal.
- `OPEN_ITEMS.md`: "Limits of the event format", "Autorun runs one time".

## 3. Editor window

- **Now**: the picture is not scaled. The window is the picture and the panels beside it: 240 + 1280 + 440 = 1960 pixels. All tools are sections of one scroll column. The event panel is in that column with the properties.
- **Build**: in the editor, the render target has the size of the room that the panels leave (the game keeps the fixed size). Panels with a width that the user sets. The event panel as a window of its own, or a second column. A search field in the pick lists.
- **Watch**: the benchmark runs with no editor: its numbers must not move. `View` (`crates/editor/src/view.rs`) and the pointer offset use the picture size and `WindowMargin::left`: one place to change.
- **Done when**: the editor fits a 1920 x 1080 display with the default config; a window resize changes the view, not the panels; the pick and the gizmo are exact after a resize.
- `OPEN_ITEMS.md`: "Size of the editor window", "Display scale other than 100 %".

## 4. Key chords and editor settings

- **Now**: a binding is one key (`maki_platform::Binding`). Ctrl+Z, Ctrl+Y, Ctrl+D, Ctrl+S are two actions each, tested in `Editor::keys`. The `[editor]` settings and the open panels are not saved. A click uses the pointer place of the frame, not of the press.
- **Build**: `Binding` gets modifiers; the help panel shows "Ctrl+Z". The mouse press carries its place (`RawInput`). The settings resources are written back to the config file (`ConfigFile` has the text only today).
- **Done when**: the editor has no `MODIFIER` action; a changed grid step is there after a restart; a script that presses and moves in one millisecond selects at the press place.
- `OPEN_ITEMS.md`: "Key chords", "Config values at run time", "Place of the pointer at a click".

## 5. Play signal for modules

- **Now**: on play and stop the editor sets back what it knows (`reset_play_state` in `crates/editor/src/lib.rs`): game state, event runner, message, fade, `MoveCapture`, the room. A module with state of its own (`CameraShake` in `game/src/shake.rs`) keeps it.
- **Build**: a message or a schedule in `core` or `scene` ("a game starts", "a game stops") that each module with play state reads. The editor sends it; the game sends it at start. The same hook is "new game" and "load game" later.
- **Done when**: a shake that runs at a stop does not go on in the next play; the editor has no list of the resources of other crates.
- `OPEN_ITEMS.md`: "Play state of a module".

## 6. Message box with choices

- **Now**: `Message` shows in an egui debug window (`crates/ui/src/events.rs`). No choice of the player.
- **Build**: a styled text box (the interface `MessageBox` stays). A `Choice` command with one command list for each answer: the format holds it as it holds `If` (`Step::open`). The event panel shows its lists as rows: `rows` and `edit_list` in `crates/editor/src/events.rs` know only `Builtin::If` today, so make "a command with lists" a trait of the command set.
- **Done when**: an event asks a question with two answers, made in the editor, and each answer runs its list.
- `OPEN_ITEMS.md`: "Message window", "Dialogue with choices, a graph view, scripts".

## 7. Sound

- **Now**: none.
- **Build**: a crate `audio` (after `assets`, before `scene` or beside it; no dependency on `gpu`, `render`, `ui`). Sound assets with handles. Commands `PlaySound`, `PlayMusic`, `StopMusic` through `CommandRegistry`, from a module, as `ShakeCamera` is. A sound at a place in the room (a prop or a light that hums). In the editor: a file picker as that of the model files, and `add_argument_editor`.
- **Decide**: the crate for output and decoding (check the version against the docs); files for music are streamed or loaded.
- **Done when**: the switch of room B clicks, the lamp of room A hums and stops with the power, both set in the editor; stop in the editor ends all sound (item 5).

## 8. Marks with depth, missing handles

- **Now**: the marks are egui lines with no depth test: the grid shows on top of props, a mark behind a wall shows through it. No handle for the direction of a cone light or the size of an event area. A rotation of many items turns each at its place. The gizmo of a mover moves one pose.
- **Build**: cut the marks with the depth image of the trace pass (it exists as a guide of the denoiser), or a line pass with a depth test (`GraphicsPipelineDesc` gets a second state). A direction handle for a cone light; corner handles for an event area and a doorway on its wall; a group turn around the gizmo; "move the two poses" for a mover.
- **Done when**: the grid does not show on a prop; a cone light is aimed with the mouse; an event area is sized with the mouse.
- `OPEN_ITEMS.md`: "Marks behind objects", "What the gizmo does not change", "Graphics pipeline state".

## 9. Reload cost

- **Now**: `patch_room` changes a light, the pose of a prop, and a mover in place. Each other change loads the room again: each generated mesh is made again and each BLAS is built while the game waits (`Gpu::create_blas`). A drag of the room width does that in each frame, and the picture starts again.
- **Build**: the loader keeps a mesh whose `MeshDef` and `LookDef` did not change (a key for each generated mesh). The BLAS build is recorded in the frame. Loading on a thread for model files and textures.
- **Done when**: a drag of a prop color and of the room width follows the mouse with no stop; a room with a model file of 100 000 triangles changes with no long frame.
- `OPEN_ITEMS.md`: "BLAS build stops the game", "Asynchronous loading".

## 10. Models and textures in the editor

- **Now**: a prop from a file is a path from a list of the `.gltf` and `.glb` files at the room file. The path was never used in the editor with a real file. A texture is a typed path. No preview: egui user textures are off.
- **Build**: user textures in the UI pass (`crates/ui/src/overlay.rs`), with the sampler of the texture. A picker with a small picture for textures and models. One room of the game with a model from a modelling tool.
- **Done when**: a model and a texture are picked by their picture; the model shows in two rooms with one load; a click selects it.
- `OPEN_ITEMS.md`: "Room file props from files", "egui features that are off", "Material kinds from glTF", "Player model from glTF".

## 11. Save and load

- **Now**: flags and variables are in memory. `Step::memory` of a waiting command is not saved.
- **Build**: a save file with the room, the spawn point or the player pose, and `GameState`. A save point as an event command. The play signal of item 5 as "load".
- **Decide**: a save in the middle of an event (needs `Step::memory` as data) or only between events.
- **Done when**: the game is closed after the switch of room B, and the next start is in room B with the lamp off.
- `OPEN_ITEMS.md`: "Game state on disk", "Memory of a waiting command".

## 12. Prefabs and copy between rooms

- **Now**: "Copy" makes one copy in the same room. Commands have a buffer that stays between rooms; items do not.
- **Build**: a buffer for items, as `EventUi::clipboard`. A prefab file (a list of items with one origin) that a room names; the loader puts its items into the room.
- **Decide**: a prefab is copied into the room (simple, no later change) or named by the room (a change of the prefab changes each room).
- **Done when**: a table with two chairs is in two rooms, and one change of it shows in the two.

## 13. Benchmark scripts

- **Now**: `TRIP_SCRIPT`, `WALK_SCRIPT`, `SHAKE_SCRIPT` (`game/src/scripts.rs`) hold frame numbers that fit the room files. The editor has no word on them.
- **Build**: a script names places ("walk to the area of `switch`", "interact") and the runner steers the player there; or the benchmark rooms are copies that the game content does not touch.
- **Done when**: the switch of room B is moved by 1 m in the editor, and the trip script still presses it.
- `OPEN_ITEMS.md`: "Benchmark scripts and room files".

## 14. The rest of the stage 28 gate

- **Now**: the 7 events of rooms A and B were made again by hand in the editor. The boxes, props, lights, doorways, and spawn points were kept. The comments of the events that were made again were not typed again, so the files of the game were not replaced.
- **Do**: one session in the editor, from "New room": room A and room B with all items and events, saved under new names; then `bench.py run --only trip_720p` with them in place of the files of the game and `bench.py same`. List each place where a text editor would have been faster.
- **Done when**: the picture of `trip_720p` is byte-equal with the two rooms from the session, or each difference has its reason.

## Not in this list

Part 3 items that the editor did not touch stay in `OPEN_ITEMS.md`: anti-aliasing in motion, light of a mesh, caustics, skinned characters, the export build.
