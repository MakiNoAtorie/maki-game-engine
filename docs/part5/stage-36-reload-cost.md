# Stage 36: reload cost

Plan: [README.md](README.md). Report: `docs/part5/stage-36.md`. Item: M9 of [`../part4/MISSING.md`](../part4/MISSING.md).

Goal: a change of a room costs what changed, and no load makes the game wait for the GPU or for a file.
Gate: a drag of a prop color and of the room width follows the mouse. A model file of 100 000 triangles changes with no long frame.

Written after the report of stage 35, from the code at `db815ca`.

## Context in the code

### The room loader

- `load_requested_room` (`crates/scene/src/room.rs`) removes each generated mesh of the old room (`CurrentRoom::meshes`) and makes each mesh of the new room. A model file that the new room names again stays (`RoomModels`).
- The editor loads the room again for each change that `patch_room` cannot make (a color, the room size): in each frame of a drag.
- `Spawner::add` gives a mesh to the store; `room`, `prop`, `mover`, and `door` call it.

### The renderer and the GPU

- `GpuMeshes::sync` (`crates/render/src/meshes.rs`) uploads each mesh whose handle is new, in groups: one BLAS for a mesh that is in no model, one for all meshes of a model. A handle that stays costs nothing.
- `Gpu::create_blas` (`crates/gpu/src/accel.rs:103`) records the build with `run_now`: a command buffer of its own, and `queue_wait_idle`. The wait is for all GPU work, also the frame in flight.
- `Gpu::build_tlas` records the TLAS build into the frame, with a barrier before it. The pass `build_tlas` is the first pass of a frame.
- `destroy_buffer` frees a buffer when the frames in flight have ended; `destroy_unused_buffer` frees at once.

### The model loader

- `load_gltf` (`crates/assets/src/model.rs`) reads the file on the calling thread. `read_gltf` uses the texture store while it reads (`Images::textures`): it cannot run on another thread as it is.
- A handle exists before its data (`Assets::reserve`, `finish`). `spawn_loaded_models` (scene) and `attach_player_model` (gameplay) wait for `LoadState::Ready`.
- The mesh table entry of a mesh is written one time (`GpuMeshes::sync`, the `ponytail` comment): a texture that arrives after its mesh does not show.
- The benchmark must do the same work in each frame of each run (`tools/bench.py`).

## Prompt

```text
Stage 36: reload cost.
Read docs/part5/stage-36-reload-cost.md and item 9 of
docs/part4/MISSING.md first.

The loader keeps a mesh
- Each generated mesh of a room has a key: the numbers that it is made
  from. The next room takes a mesh of the old room with the same key
  and makes only the others. Two things of a room with one key have a
  mesh each, as before.

The BLAS build in the frame
- Gpu::create_blas waits for nothing. The build is recorded into the
  next frame, before the TLAS build. Its buffers go away after that
  frame.

Loading on a thread
- A model file is read and its images are decoded on another thread;
  a system stores the result. The room loader does not change but for
  the call.
- A setting [assets] background. False: on the thread of the game, in
  the frame that asks. The benchmark and the tests that count frames
  use false.
- A model that is removed while it is read leaves nothing.

Out of scope: an image file of a generated mesh on a thread (the mesh
table entry is written one time), a thread pool, a cooked model
format, mip levels on the GPU.

Done when: a test loads a room again and one mesh is new; a test reads
a model on a thread; the validation layer is silent with the builds in
the frame; the time of a reload in the editor and of a room change
with a model of 100 000 triangles is measured, before and after;
trip_720p is byte-equal. Update OPEN_ITEMS.md ("BLAS build stops the
game", "Asynchronous loading"). Report.
```

## What to watch for

- **The picture.** `trip_720p`, `walk_720p`, and `textured_walk_720p` byte-equal: the same BLAS, built at another time.
- **A BLAS that is never built.** A mesh that is made and removed before a frame draws: its build and its buffers go away.
- **The order in the frame.** The BLAS builds are before the TLAS build, with a barrier between them. No validation error.
- **The benchmark.** `assets.background = false` in each scenario: the model of a scenario is there in the first frame, in each run.
- **A model that fails.** A file that is not a model, read on a thread: the error is logged, the prop does not show, the count of the files that are read goes back to 0.
- **Two props with the same numbers.** A mesh each, as before: the pictures of the rooms of the game do not change.
- **The collider of a prop with a kept mesh.** It is made from the mesh in the store, at the new place of the prop.
