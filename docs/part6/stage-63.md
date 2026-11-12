# Stage 63: skinned characters

State: done. Stage file: [stage-63-skinning.md](stage-63-skinning.md). Tickets #95 to #100 (pull request #94). Plan: `README.md` 2.6, 2.15, 3.2 item 9.

Lanes: #95 (A), #96 (B), #97 (C) at the same time; then #98 (D) with the generator of #100 (F); then #99 (E); then the rest of #100 (F). Then one code review (standards and spec), one lane that searched the cause of a picture that changes from run to run, and one lane of fixes.

## 1. What was built

- **The skin in the model asset** (#95): `Model.skeleton` (`Skeleton { joints, inverse_binds }`, joints parent-first, named by the glTF node name or `joint<node index>`); a skinned mesh keeps its vertices in bind space with up to 4 joints and weights (`Mesh::with_skin`); `maki_assets::skin::skin_vertex`, the CPU reference of the shader.
- **Clips from glTF** (#96): `Track.target` (a joint by name; none: the object). `gltf_clips`: `STEP` is a step track, `LINEAR` linear, `CUBICSPLINE` linear on its values with a warning; values relative to the joint rest. A clip of a model file is `models/figure.glb#wave` (`DataAsset::read_parts`).
- **A BLAS that is built again** (#97): `Gpu::create_dynamic_blas` (several geometries in one vertex buffer of the caller, 3 corners for each triangle) and `rebuild_blas` with its barriers; `Gpu::compute_barrier`.
- **The pose of the joints** (#98): `SkinPose { previous, current }` and `SkinPalette` (written only when it differs); `animate` writes target tracks into the joints; a step write counts one `PoseJumps`; `ResetPlay` gives back the rest; on a cut a joint with no track goes back to its rest. A model with a skeleton keeps its object entity, so an `animator` is on it.
- **GPU skinning** (#99): `extract_skins`, the pass `skin` (before `build_tlas`) with `skin.comp` (one thread for each triangle corner), its own mesh table block, normals, and dynamic BLAS for each skinned instance; the TLAS is built again after a skin rebuild. The instance buffer is 64 bytes: the bindless index of the corner positions of the frame before, so a bent vertex has its own motion; a rigid instance names none.
- **Content and measure** (#100): `tools/gen_models.py` writes `game/assets/rooms/models/figure.glb` (joints `root`, `arm_l`, `arm_r`; clips `wave` LINEAR, `hop` STEP); the test room `room_skin.ron` (`game/skin.toml`); the scenario `skin_jump_720p` and `bench.py skinjump`; the input script `tools/editor_scripts/stage63.txt`.
- **Review fixes**: one function for the name of an unnamed joint; `#` names a part only after a `.glb` or `.gltf` file; a full mesh table is one error for each entity; no warning of a missing joint before the model has loaded; one-line comments in the shaders; `skin_jump_720p` in the exceptions of the rule of the benchmark rooms.

## 2. Files

- `crates/assets/src/skin.rs` (new), `model.rs`, `mesh.rs`, `lib.rs`: the skeleton, the skin of a mesh, `skin_vertex`.
- `crates/assets/src/clip.rs`, `data.rs`: `Track.target`, `gltf_clips`, `read_parts`, `file#part`.
- `crates/gpu/src/accel.rs`, `lib.rs`: the dynamic BLAS, `compute_barrier`.
- `crates/scene/src/skin.rs` (new), `animation.rs`, `room.rs`, `lib.rs`: `SkinPose`, `SkinPalette`, target tracks, the object entity of a file prop.
- `crates/render/src/skins.rs` (new), `shaders/skin.comp` (new), `passes.rs`, `motion.rs`, `meshes.rs`, `lib.rs`, `shaders/trace.glsl`, `scene.glsl`: GPU skinning and its motion.
- `tools/gen_textured.py`, `tools/gen_models.py`: skins and animations in a generated glTF; `figure.glb` and its id file.
- `tools/bench.py`: `skin_jump_720p`, `skinjump`.
- `game/assets/rooms/room_skin.ron` (new), `game/skin.toml` (new), `game/scripts/src/lib.rs`, `tools/editor_scripts/stage63.txt` (new).
- `CODING_STANDARDS.md`: `skin_jump_720p` in the exceptions.
- Tests: `crates/assets` (skin, clips, parts), `crates/gpu/src/accel.rs` (`corner_offsets`), `crates/scene/src/tests/l63d.rs`, `crates/render/src/skins.rs`, `motion.rs`.

## 3. Results

| Check | Result |
|---|---|
| Gate in code: a generated glTF with three joints and two animations loads its joints, inverse binds, and weights; `STEP` is a step track on the joint by name, `LINEAR` a linear one; a step clip snaps the joint pose (`previous == current`, one `PoseJumps`); the CPU skin of a vertex of a joint turned 90 degrees | `crates/assets` tests, `crates/scene/src/tests/l63d.rs` |
| Gate of the picture (`bench.py reference skin_jump_720p`, `reference jump_720p`, `skinjump`) | `ok`: the crop that the arm left, 4 frames after the jump: relmse 0.0153 against 0.0928 of the cube of `jump_720p`; the same in two runs, and again on the tree with the review fixes |
| Tests of "Done when" | Each line has a test, but `skin.comp` against `skin_vertex` (no GPU readback harness; the shot and the gate show the bent mesh) |
| Tests and clippy | `tools/check.sh` on the merged tree: all pass |
| Validation run | Debug, sync validation on, two figures (`#wave`, `#hop`): no error |
| Trip run (debug) | Rooms A, B, A; no error |
| Picture gate (`bench.py gate`) | `trip_720p`, `walk_720p`, `materials_720p`, `textured_walk_720p` equal; `test_1080p` noise in most runs, and in some runs 4 pixels change by up to 0.70 (section 6) |
| Benchmark against `s62` | Run `s63`. GPU total -10.2 % to +0.6 %: no step up; the steps down (eight scenarios over 3 %) are the machine (no change in their code) |
| Run by hand in the editor | `stage63.txt` in `room_skin.ron`: the waving arm at another angle in two shots 8 frames apart, with no ghost; the hop sharp in both shots; the trigger area cuts the first figure to `#hop`; no validation error |

## 4. Deviations from the stage file

- The joint index range is checked in the loader, not in `Mesh::new` (a mesh does not know the count of joints). Joints are sorted parent-first; the inverse binds and the indices follow.
- A dynamic BLAS takes several geometries (a model group has several meshes; `trace.glsl` finds the mesh by the geometry index).
- One BLAS and one buffer of positions for each skinned instance, no swap by frame slot: a BLAS keeps the address of its vertex buffer. `skin.comp` copies the old current position into the buffer of the frame before, then writes the new one.
- `skin_jump_720p` uses a clip of one step key on `arm_r`, not `#hop` (it snaps back at 2 s, so the reference would end with the arm down).
- `figure.glb` is in `game/assets/rooms/models/`, beside `cabinet.glb`.

## 5. Decisions where the stage file was silent

- A non-joint ancestor of a joint (an armature node) is folded into the rest of the joint below it; clip values use the local rest of the node, which gives the same pose for rigid ancestors.
- A mesh with a second skin is read as a rigid mesh, with one warning for the file; a joint missing from the scene, or a skinned primitive with no `JOINTS_0`/`WEIGHTS_0`, is a load error. A zero-weight slot has joint 0, so the shader reads each index with no range check.
- The object entity of a file prop is its load entity (also for a prop with no skin and with object components); a model with no skeleton still spawns one entity for each mesh.
- A skinned instance names the positions of the frame before only in a frame that bent it; else its current positions (zero motion of a pose at rest).
- A full mesh table: one error, and the instance is not skinned until it is despawned.
- On a cut, a joint with no track in the new clip goes back to its rest (as a channel of an object).

## 6. Uncertain

- **`test_1080p` changes from run to run** (rarely, 4 pixels up to 0.70). A lane searched the cause: it is there before stage 63 too (1 of 12 runs of `83a8fc5`, the last commit of stage 62); the recorded commands of the frames are bit-identical in runs whose pictures differ; it starts when the scene is bound by the GPU (frame about 525); a `queue_wait_idle` after each submit makes 30 of 30 runs equal. So an overlap of the CPU and the GPU between frames, in our frame sync or in the driver. For the quality session (`HANDOVER.md`).
- `skin.comp` is not tested against `skin_vertex` on the CPU (no GPU readback harness).
- The editor picks a skinned model by its first mesh in the bind pose: a raised arm cannot be picked where it shows.
- The collider of a skinned model is the box of its bind pose.
- A joint track on a model with no skin gives no warning.
- Four test writers of a skinned glTF (assets, clips, scene tests): one test helper would serve them.
