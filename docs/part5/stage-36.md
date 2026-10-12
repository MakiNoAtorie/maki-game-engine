# Stage 36: reload cost

State: built. Gate: met by the numbers of section 2. A reload in the editor makes the one mesh that changed; a room change with a model file of 100 000 triangles takes 0.35 ms on the thread of the game (stage 35: 8 ms).

## 1. What was built

- **The loader keeps a mesh** (`crates/scene/src/room.rs`): each generated mesh of a room has a key, the text of the numbers that it is made from (the room size and doorways, `MeshDef` and `LookDef`, the size and color of a door leaf, and the directory of the room file). `Spawner::mesh` takes a mesh of the room before with the same key, or makes one. What is left of the old room is removed. Two things with one key have a mesh each: one mesh of the old room goes to one thing.
- **The BLAS build in the frame** (`crates/gpu/src/accel.rs`): `create_blas` makes the structure and copies the triangles, and waits for nothing. `build_tlas` records the builds that wait, in one call, before the TLAS build; its barrier is between the two. The work memory and the triangles of a build are freed when the frame has ended. A BLAS that is destroyed before its build takes the build with it.
- **Memory of its own for the buffers of a build**: the work memory and the triangles are each one allocation, not a part of a shared block. See section 4: without this the heavy scenes were slower.
- **Model files on a thread** (`crates/assets/src/model.rs`): `read_gltf` uses no store now; it gives the meshes and the decoded images of the file. `ModelLoads::load` starts a thread for a file and gives the handle at once; `finish_model_loads` (in `Update`, from `AssetsModule`) stores the images, then the meshes, then the model. A model that was removed while it was read leaves nothing. `load_gltf` is the same on the calling thread.
- **`[assets] background`** (default true). False: on the thread of the game, in the frame that asks. `tools/bench.py` sets false. Without `AssetsModule` (some tests) the scene module has the same resource with false.
- The log line `room loaded` has the count of the meshes that were kept and the time of the load.

## 2. Numbers

- `cargo test --workspace`: 140 tests, 141 with `--features editor` (2 new). `cargo clippy --workspace --all-targets` clean with and without `--features editor`. `cargo fmt --check` clean.
- New tests: `a_reload_keeps_each_mesh_that_is_made_from_the_same_numbers` (scene: the same room again, another color of one of two equal props, a wider room, a prop that moved); `a_model_that_a_thread_reads_is_ready_some_frames_later` (assets: a model, a model that is removed while it is read, a file that is not there).
- Debug build with the validation layer: `--bench-frames 300`, the trip script, a room change with the large model on the loader thread (4 loads), and a drag of the room width in the editor: no error, no warning.
- Pictures, `python tools/bench.py same SCENARIO s35 s36`: `trip_720p`, `walk_720p`, `textured_walk_720p`, `heavy_1080p`, `stress_720p`: `ok`, byte-equal.

### A room change with a model file of 105 984 triangles

Two rooms that go to each other each second, release build, 1280 x 720, `room_load_ms` of `bench.csv` (the time of the loader on the thread of the game):

| | Into the room with the model | Into the room with no model |
|---|---|---|
| Stage 35 | 8.0 ms | 7.0 ms |
| Stage 36, `background = false` | 6.3 ms | 0.35 ms |
| Stage 36, `background = true` | 0.35 ms | 0.35 ms |

- The 7 ms of stage 35 for a room with one generated mesh was the wait for the GPU (`queue_wait_idle`: the frame in flight).
- The read of the file takes 3.5 ms; storing the model that the thread read takes 3 microseconds (the meshes are moved, not copied).
- The frames after the change are not longer than the frames around them (the frame times are those of the display: two frames in 16.6 ms).

### A drag in the editor

A drag of the room width of room A, in the running editor on a display of 3840 x 2160. Each change of the value loads the room and keeps 6 of 7 meshes.

| Build | Time of the loader | Two frames with a reload each |
|---|---|---|
| Release | 0.085 ms | 3 ms apart |
| Debug, validation layer | 0.54 ms | 16 ms apart |

No number of stage 35 for the same drag: there the loader made 7 meshes and waited 7 times for the GPU (the table above has 7 ms for one such wait).

### GPU time

`python tools/bench.py compare s35 s36`, 26 scenarios, with the code as it is committed: `gpu_total` from -0.3 % (`test_1080p_spp4`) to +1.2 % (`stress_merged_720p`); the nine heavy scenes from -0.2 % to +0.3 %. Section 4 has the first run of the stage, which was slower.

## 3. Files

New: `docs/part5/stage-36-reload-cost.md` (the stage file).

Changed:
- `crates/scene/src/room.rs`, `lib.rs`: the keys, `Spawner::mesh`, `ModelLoads` in the loader.
- `crates/gpu/src/accel.rs`, `lib.rs`: `BlasBuild`, `record_blas_builds`, `create_buffer_in` with memory of its own.
- `crates/assets/src/model.rs`, `texture.rs`, `lib.rs`: `ReadModel`, `ModelLoads`, `finish_model_loads`, `AssetsSettings`, `decode_file`.
- `tools/bench.py`: `assets.background = false`.
- `README.md`, `OPEN_ITEMS.md`.

## 4. Deviations

- **An image file of a generated mesh stays on the thread of the game** (`Textures::load`: the texture of a wall, a floor, a prop). The plan says "model files and textures". The mesh table entry of a mesh is written one time: a texture that arrives after its mesh would not show. The images of a model file are decoded on the thread of the model. The row "Asynchronous loading" of `OPEN_ITEMS.md` has what is left.
- **A regression that this stage made and removed.** The first full benchmark of the stage had the heavy model scenes slower, with the same pictures: `heavy_merged_1080p` +12.3 %, `heavy_1080p` +5.4 %, `heavy_mixed_1080p` +3.8 %, in the image passes (`denoise_temporal` 0.352 to 0.494 ms), not in the trace. Checked back to back with the binary of stage 35, two times each: the same numbers. With the BLAS change taken out: the old times. Cause: the work memory and the triangles of a build now live until the frame has ended, in the memory blocks that all resources share; what is made after them (the images of the frame) gets another place. With an allocation of its own for each of these buffers the times are those of stage 35 (`heavy_merged_1080p` 4.465 ms, `heavy_1080p` 4.514 ms). Why the place of an image changes its speed on this GPU is not known.

## 5. Decisions

- A key is a text (`Debug` of the definitions): no hash and no order for floats is needed, and a room has some tens of meshes.
- The directory of the room file is a part of each key: a texture path starts there.
- No shared mesh for two things with one key: the mesh count and the pictures of the rooms of the game stay as they are.
- The builds are recorded in `build_tlas`, not at the start of the frame: a BLAS is of use only in a TLAS, and the barrier of the TLAS build is the one that the builds need.
- One thread for each model file, and a channel. No pool.
- A model that failed on a thread is `Failed`, as on the calling thread, with one line in the log.
- The benchmark and the tests that count frames read on the thread of the game: a model is there in the same frame in each run.

## 6. Uncertain

- Why the place of the build buffers changed the time of the image passes (section 4). The fix is measured, not explained.
- An allocation of its own for each build buffer is three calls of the driver for each new mesh. In a drag in the editor that is in each frame. The limit of allocations of a driver (4096 on some) is far: the buffers live for some frames.
- The upload of a model that a thread read is still one step on the thread of the game (normals, texture coordinates, the copy of the triangles, the mip levels of its textures). For the model of 105 984 triangles and no texture, no frame was longer; a model with large textures was not tried.
- The drag of a prop color was not tried by hand: it is the case of the test (one new mesh), and the drag of the room width is the same path.

## 7. What to watch for: results

| Point | Result |
|---|---|
| The picture | Five scenarios byte-equal, two of them with a model file (section 2). |
| A BLAS that is never built | `destroy_blas` removes the build and frees its buffers at once. By the code; the reload in each frame of the editor drag runs it with the validation layer. |
| The order in the frame | The validation layer is silent in each run of section 2. |
| The benchmark | `assets.background = false` in the config of each scenario; `heavy_1080p` and `stress_720p` byte-equal with stage 35. |
| A model that fails | The test: a file that is not there is `Failed`, and the count of the files that are read is 0 after it. |
| Two props with the same numbers | The test: two equal balls have a mesh each, and a new color for one makes one new mesh. |
| The collider of a prop with a kept mesh | The spawner makes it from the mesh in the store, at the pose of the prop. The test moves a prop and keeps all meshes; the collider itself is not asserted. |

## 8. Open items

- "BLAS build stops the game": removed.
- "Asynchronous loading": what is left (an image file of a generated mesh, the mip levels, the upload).
