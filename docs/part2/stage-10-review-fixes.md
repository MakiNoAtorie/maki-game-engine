# Stage 10: review fixes

Fixes from the stage 9 review (`docs/perf-improve.md`, Appendix A). State: done, but fixes 4 and 5 were not made (see below).

## Fixes

- [x] 0. Commit the perf rounds. Commit `fb55fc5` on the branch `part2`.
- [x] 1. Instance extraction in the render crate (A.1). `MeshInstance` and `extract_instances` moved from `game/src/scene.rs` to `crates/render/src/meshes.rs`. A module spawns an entity with `Transform` and `maki_render::MeshInstance`; no extract code in the module.
- [x] 2. Stable id, previous transform, previous camera (A.1, A.2). `ExtractedInstance` has `id` (the entity bits) and `previous_transform`. `ExtractedCamera` is `{ current, previous }`. No pass reads them yet.
- [x] 3. History images in the frame graph (A.2). `FrameGraph::add_history(desc, previous_name)` returns `(current, previous)`: two images that change places at the start of each frame, each with its barrier state.
- [ ] 4. Buffers in the frame graph. Not made: no pass shares a buffer with another pass, so no frame can test the barrier. The per-frame buffer that the CPU writes already has a pattern with no graph support: one buffer for each frame slot (`LightBuffers` in `crates/render/src/light.rs`). Stage 11 uses that pattern for the previous camera. Stays in `OPEN_ITEMS.md`.
- [ ] 5. Trait between the renderer and `maki_gpu::Gpu` (A.3). Not made: large (each pass, `GpuMeshes`, the UI pass), and the review says that this seam is too low for a software tracer. Needs a decision of the owner.

## Checks

- `cargo test --workspace`: 44 tests pass. New: instance id and previous transform, previous camera, history swap with barrier plan.
- `cargo clippy --workspace --all-targets`: clean.
- Validation (debug build, `--bench-frames 300`, and `MAKI_RENDER_SELFTEST=1`): clean.
- Benchmark, run `stage10` against `loop4`: `gpu_total` 0.905 to 0.906 ms (`test_720p`), 2.394 to 2.378 ms (`heavy_1080p`, 1000 instances). `bench.py same`: the pictures are equal bit for bit.
- CPU frame on `test_720p`: 1.127 ms without the fixes (run `base10`), 1.141 ms with them (run `stage10b`). `loop4` had 0.893 ms in an earlier session: compare only runs of one session.

## What to watch for

- Scope creep ("while I was there, I also changed X"). A change that is not on the list is reverted or justified.
- Benchmark before against after (`bench.py compare`, `bench.py same`). A fix does not make a scenario slower without a reason.
