# Stage 14: benchmark and review

Needs stages 12 and 13. Do the tasks in order: the benchmark first, so the review has numbers.

State: done.

## Result

### Benchmark

`--bench-frames N --bench-script trip` plays the full loop with actions (`TRIP_SCRIPT` in `game/src/bench.rs`): room A, the east passage, room B, the switch and its message, back to room A, the door that was locked, through it. 820 frames. Each frame is one fixed step, so each run is the same. `bench.csv` has the column `room_load_ms`.

Run `stage14` (`bench.py run --only trip_720p --frames 900 --warmup 0`), 1280 x 720, one sample for each frame:

| Value | Median | p99 | Max |
|---|---|---|---|
| `gpu_total` | 1.00 ms | 1.43 ms | 1.71 ms |
| `gpu_trace` | 0.37 ms | 0.46 ms | 0.72 ms |

| Frame | Room | `room_load_ms` | `cpu_ms` of the frame |
|---|---|---|---|
| 1 | room A, first load | 28.7 | 157.6 (startup) |
| 205 | room B | 2.2 | 16.1 |
| 548 | room A | 2.0 | 2.6 |

No transfer frame is longer than a frame with no transfer. The rooms have generated meshes only: a room with a model file will show the cost of the synchronous load and of the BLAS build (`OPEN_ITEMS.md`, "BLAS build stops the game").

`cpu_ms` in this session: each second frame waits 16 ms in the present call (`OPEN_ITEMS.md`, "CPU frame time in the benchmark"). Use the GPU columns.

Against the stage 9 benchmark (run `loop4`, the last perf round) and run `stage14full`, `gpu_total` median in ms. The scene has the player, the door, and two doorways more than then.

| Scenario | Stage 9 | Now | Trace now | Denoiser now |
|---|---|---|---|---|
| `test_360p` | 0.278 | 0.292 | 0.111 | 0.132 |
| `test_720p` | 0.905 | 1.198 | 0.376 | 0.764 |
| `test_1080p` | 2.227 | 2.208 | 0.814 | 1.347 |
| `heavy_1080p` | 2.394 | 2.137 | 1.112 | 0.980 |
| `heavy_mixed_1080p` | 3.516 | 3.318 | 2.319 | 0.953 |
| `idle_1080p` | 0.047 | 0.056 | 0.001 | 0.009 |
| `walk_720p` | - | 1.284 | 0.405 | 0.817 |
| `trip_720p` | - | 0.946 | 0.238 | 0.612 |

The vignette fits in the frame budget: at the default resolution (640 x 360) one sample for each pixel costs 0.11 ms, so the automatic sample count fills `trace_budget_ms` with 64 samples.

Picture quality with the final rooms (`bench.py quality`, new references): `walk_720p` at frame 180: `relmse` 0.00487. `test_720p` at rest: 0.00433 after 64 frames, 0.000891 at 1024 samples.

### Review against the rules

| Rule | State | Notes |
|---|---|---|
| 1. Dependencies flow downward | Holds | `scene`: core, assets. `events`: core, platform, scene. `render`: core, platform, assets, scene, gpu. `ui`: core, platform, scene, gpu, render. No crate names one to its right. |
| 2. No Vulkan type outside `gpu` | Holds | No `ash` or `vk` name outside `crates/gpu`. `render` shows `maki_gpu::Gpu`, its handles, `Access`, and `Format`: types of the gpu crate, not of Vulkan. That seam is still the concrete `Gpu` (stage 10, fix 5, not made). |
| 3. The renderer reads only extracted data | Holds | The passes get the render world. The systems of `Render` and `Present` read `RenderGpu`, `FrameGraph`, `RenderWorld` only. New `Extract` readers: `CurrentRoom` and `ScreenFade` in `extract_frame`. |
| 4. Gameplay reads actions | Holds | No `RawInput`, `Key`, or winit name in `game`, `scene`, `events`, `render`. The benchmark scripts hold actions (`ScriptedActions`). The message closes on the `Interact` action. |
| 5. A feature is a module | Holds, with three weak points | (a) `Action` is one enum in `platform`: each new action is an edit there (accepted in stage 2). (b) `Config` is one struct in `core`: the player and camera keys of the game are in the core crate. (c) Systems of two modules that write the same component need an order, and the order names a system of the other module: `move_player`, `control_lights`, and `toggle_movers` name `maki_events::run_events`. The names go one way, from `game` down. |
| 6. Generational handles | Holds | Assets and GPU resources: handles. Entities: `Entity`. Events find doors and lights by the `Name` of the room file in each step: a name is data, and a name that no entity has gives a warning, not a stale access. |
| 7. State that survives a room change is in resources | Holds | `GameState`, `EventRunner` (a running event goes on in the next room), `MessageBox`, `ScreenFade`, `MoveTarget`. The player and the camera are entities with no `RoomEntity` marker. Not kept: the state of a room entity (a door that was opened, a light that was turned off). Content sets it again from flags (room B, event "lamp is dead"). |
| 8. Content is data | Holds for rooms and events | Checked: other text and another flag name in the room files, no build. In code and tied to content: the benchmark scripts (frame numbers that fit `room_a.ron` and `room_b.ron`), the room template in `tools/bench.py`, and constants of the player (radius, interact reach). |

### Places most likely to need rework

Not fixed, as the stage asks.

Skinned animated characters:

1. A BLAS is built one time and never changes (`GpuMeshes::upload`, `Gpu::create_blas`, which waits for the GPU). A skinned mesh has new vertex positions in each frame: the gpu crate needs a BLAS update that is recorded in the frame, and the normal buffer (`NormalBuffer` in `scene.glsl`) needs the new normals.
2. The mesh asset and the loader. `Mesh` has positions, normals, and indices. `read_gltf` applies the node transforms to the vertices and reads no skin, no joint weights, and no animation. A `Model` is a flat list of meshes, `Transform` has no parent, and the player model is "an instance of the first mesh draws the full model" (`attach_player_model`).
3. Motion vectors. `previous_pixel` in `trace.comp` moves a hit point with one previous transform for each instance. A skinned vertex moves on its own: the shader needs the previous positions of the triangle, or the denoiser loses the history on each animated character.

An editor that edits rooms and events visually:

1. A room goes one way: file to `RoomDef` to entities (`Spawner` in `crates/scene/src/room.rs`). An entity does not know the entry that made it, a change is a full reload (which starts the picture again and stops parallel events), and `RoomDef` cannot write a file back: it skips the events (`IgnoredAny`) and RON drops the comments. The editor needs one document for a room with its events, a link from each entity to its entry, and a change that touches one entity.
2. The ui crate is debug windows (`add_panel`). No menu, no dock, no picking, no handles in the picture, and egui runs without clipboard. Picking needs a ray against the scene on the CPU: the scene has only the boxes of the colliders; the triangles are in the BLAS on the GPU.
3. No "edit" against "play". The simulation, the event interpreter, and the camera follow run in each frame, and the first `Camera` entity is the view. The editor needs a state that gates the gameplay systems, and its own camera (`OPEN_ITEMS.md`, "Two cameras").

A baked-lighting mode:

1. No place to store baked light. A mesh has no texture coordinates, the loader reads no textures, and the mesh table entry (32 bytes: normals, material, color) has no field for a light map or a probe. The gpu crate has images only as frame graph transients plus the font image: no texture asset.
2. The frame is one path: `trace`, the denoiser passes, `tonemap`, all driven by `Accumulation`. A bake is the same tracer with another target (points on surfaces, not pixels) and a result that goes to disk; the runtime mode is a tracer that reads the result in place of its bounce ray. `trace.comp` is one shader with the camera in it.
3. Nothing says what is static. Each light, door, and mover can change, and events do change them (`LightOff`, the flicker). A bake needs "static" and "baked" marks in the room file, and a rule for a baked light that an event turns off.

## Tasks

- [x] `--bench-frames` runs a script: the player walks a fixed path through room A, transfers to room B, and comes back. Room load and the denoiser both run.
- [x] CSV for each frame as before, plus a column for the room load time on transfer frames.
- [x] Review the code against rules 1 to 8. Report rule by rule.
- [x] List the three places most likely to need rework for each of:
  - skinned animated characters,
  - an editor that edits rooms and events visually,
  - a baked-lighting mode.
- [x] Do not fix them.

## Repo state that this stage meets

- `tools/bench.py` reads the CSV columns and keeps `tools/bench_history.csv`. A new column must not break `run`, `same`, and `quality`.
- The scripted run drives actions, not raw keys (rule 4).
- `--bench-static` and `--bench-dump` exist. The script is a third mode of the same benchmark module.

## What to watch for

- Frame time spikes on transfer frames. Some cost is normal. A long stop points at synchronous asset loading or at the BLAS build with a GPU wait (`OPEN_ITEMS.md`, "BLAS build stops the game").
- Compare with the stage 9 benchmark. The vignette fits in the frame budget.

## Done when

- [x] The scripted benchmark runs and writes the CSV.
- [x] The report has the rule review and the three rework lists.
- [x] Stop and report.
