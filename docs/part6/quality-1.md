# Quality and performance session 1

Branch `opt/quality-perf`, from `main` at `97583fe` (stage 65 merged). The work order is the list "for the quality session" of `HANDOVER.md`.

## 1. What was done

- **`test_1080p` changes from run to run: cause found, gate made exact.** The NVIDIA driver changes the result of the most used compute pipeline (`denoise_filter.comp`, 7 dispatches in each frame) after a few thousand dispatches, by one step of a 16-bit float, at a frame that depends on timing. New option `--bench-serial` (`Gpu::set_serial`): each frame waits until the GPU has finished it. `bench.py gate` uses it. 6 of 6 and 24 of 24 runs then give the same picture.
- **A pose jump keeps no indirect history** in its frame. `bench.py jump` at 4 frames: 0.093 to 0.0017.
- **A change of the brightness of a light limits the indirect history** to the frames over which the brightness changed by at most 5 % in sum. A light at rest keeps all of it. `bench.py lightanim` (pulse): 0.0042 to 0.00097. The lamp step of `bench.py jump` at 4 frames: 0.074 to 0.0080.
- **Skinned BLAS: a refit** (`ALLOW_UPDATE`, mode `UPDATE`) in place of a full build, a full build after 30 refits. New scenario `skin_crowd_720p` (16 figures, an arm swings in each frame): skin pass 1.296 to 0.406 ms, GPU total 3.343 to 2.435 ms. Trace time and picture unchanged.
- `animate` asks `DataAssets` for its clip without a change mark.
- Bench bug: the bench moved the first light of the query, not of the room file. With a pulse on the second lamp, `light_anim_720p` moved the other lamp, and `bench.py lightanim` measured 177 (a different picture). The reference `light_still_720p` was made again.

## 2. Files

- `crates/gpu/src/lib.rs`: `Gpu::set_serial`, the wait after the submit.
- `crates/gpu/src/accel.rs`: dynamic BLAS with `ALLOW_UPDATE`; `rebuild_blas` refits, a full build each `MAX_REFITS + 1` builds; scratch for the larger of build and update.
- `crates/devtools/src/bench.rs`: `--bench-serial`; the bench light is the lowest entity with a light.
- `crates/render/src/accumulation.rs`: `request_history_limit` (frames, for one frame); a jump asks for 0.
- `crates/render/src/light.rs`: the size of a change of a light (`Change`), the sum of the recent brightness changes (`frames_within_lag`, `LAG`).
- `crates/render/src/lib.rs`: `RenderWorld::limit_accumulation`.
- `crates/scene/src/animation.rs`: the clip lookup bypasses change detection.
- `tools/bench.py`: `run --serial`, `base --serial` (only for a commit that has it), the gate with serial frames, scenario `skin_crowd_720p`.

## 3. Results

Quality (`relmse` against the reference, lower is better):

| Measure | Before | After |
|---|---|---|
| `jump`: cube crop, 4 / 8 / 28 frames after the jump | 0.093 / 0.036 / 0.0024 | 0.0017 / 0.00051 / 0.00023 |
| `jump`: lamp step, same crop, 4 / 8 / 28 frames | 0.074 / 0.044 / 0.0091 | 0.0080 / 0.0043 / 0.0027 |
| `jump`: control crop, 4 frames | 0.00017 | 0.00032 |
| `lightanim`: pulsing lamp, frame 120 / 240 | 0.0042 / 0.0042 | 0.00097 / 0.00095 |
| `lightanim`: still lamp, frame 120 | 0.00037 | 0.00037 |
| `skinjump`: arm crop, 4 frames | 0.015 | 0.0128 (floor 0.0100) |

"Before" of `lightanim` is with the bench fix and the new reference: the old numbers (0.00356, 0.00042 in `stage-64.md`) were of another reference.

The control crop is noisier for some frames after a jump: the indirect light starts again in the full picture.

Picture gate against `main` (`97583fe`): `walk_720p`, `textured_walk_720p`, `materials_720p` equal, `test_1080p` noise (the base build has no serial frames). `trip_720p` changed: max 4.6 at a lamp edge, mean 0.2728 to 0.2732. The trip ends in room A after the switch: the lamp flickers by steps of its brightness, the case of the new rule. No reference: the scene moves.

GPU time (`q1` against `s65`, all scenarios): no change outside of the drift. Three scenarios out of 1 % went through `bench.py confirm 97583fe`, two rounds: `heavy_mixed_lights8_1080p` -6.1 / +5.0 %, `trip_720p` -1.5 / -0.1 % (a third round -3.0 %), `heavy_mixed_lights8_1080p_bounces2` +0.2 / -1.3 %. `skin_crowd_720p` (new): 3.343 to 2.435 ms in alternating runs of the two builds.

Validation (debug build, 300 frames of `skin_crowd_720p`, `test_1080p`, `light_anim_720p`): no message. `sh tools/check.sh`: all steps pass.

## 4. How the cause of the changing picture was found

8 to 24 runs of `test_1080p` (530 to 700 frames) for each try; the table gives the number of distinct pictures. The rate was higher than the 1 in 12 of stage 63.

| Try | Distinct pictures |
|---|---|
| none | 12 in 24 |
| wait for the frame before, before each submit (no two frames on the GPU at one time) | 7 in 16 |
| each CPU write into a GPU buffer waits for the GPU | 6 in 16 |
| the two together | 3 in 12 |
| a full barrier (all stages, all memory) before each pass | 6 in 16 |
| no user interface pass | 4 in 12 |
| `queue_wait_idle` after each submit | 1 in 24 (all equal) |

- The push constants, the TLAS instances, and the bytes of each buffer write (the UI vertices excluded) were the same in all frames of runs whose pictures differed.
- The pictures are equal up to frame 516 and differ from 517 on, in every run in which they differ. All images of the frame graph dumped at frame 530: the trace, the temporal pass, the filter input are equal; the work images of the indirect filter differ. Each difference is one step of a 16-bit float, up and down in equal numbers: a different rounding of the same math, not a lost write.
- 3 indirect filter passes: equal pictures to frame 560, different from about frame 590. 4 passes: from 517. The onset moves with the number of dispatches of the filter pipeline (about 3600).
- `precise` on all math of the filter shader: no change. A 5 s sleep at the start: more different pictures (11 in 12).
- Not a shared memory: the memory ranges of all 43 images are disjoint.
- So the driver runs a second compiled form of the hot pipeline from some frame on (a background optimization). It is outside the code; a serial run gives it the same frame in each run. A frame in flight of the game is not changed.

## 5. Deviations and decisions

- `bench.py skinjump` now measures the error that a jump adds: the error at 4 frames over the error of the settled frame 150 of the same crop (the owner, 2026-11-13). The arm crop has a floor of 0.0100 (an edge of the body against the reference), the cube crop 0.0002. The arm adds 0.00275, the cube 0.00147. The owner set the bar: the arm adds at most 2 times what the cube adds. `ok`. Both are noise of the restarted indirect light; neither has the old shadow. Before this session: arm 0.015, cube 0.093 (raw).
- `LAG` = 0.05 by measurement: 0.01 gave 0.0028, 0.025 gave 0.0016, 0.05 gave 0.00097, 0.1 gave 0.00125, 0.2 gave 0.0022 (`light_anim_720p`, frame 120).
- A switch of a light (off to on) is a brightness change of 1.0: in that frame the indirect light starts again, then grows. Before: a fade of 16 samples.
- A light that moves, or changes its color with the same brightness, keeps the soft reset of before.
- `MAX_REFITS` = 30, not measured: no scene has a pose far from its last full build for long.

## 6. Open

- The test of `Gpu::set_serial` is the picture of runs, not a unit test.
- The window of brightness changes counts drawn frames of the extract, not frames of the GPU: a frame that is not drawn is counted.
