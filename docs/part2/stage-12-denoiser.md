# Stage 12: denoiser

Replace whole-image accumulation resets with a real-time denoiser in the spirit of SVGF, adapted to the low internal resolution and the stylized look.

Needs stage 11: motion vectors, stable `normal` and `albedo`, and a player and a door that move.

State: done. Numbers: `docs/perf-improve.md`, section 0.31.

Changes against this plan:

- Steps 2 and 4 are one change. `denoise_temporal` is the accumulation: the mean of each pixel has up to `max_samples` samples, so the `accumulate` pass and the `accumulated` image are gone.
- The history of a pixel counts samples, not frames, so the automatic sample count needs no special case.
- The renderer still gets a hint from `Extract` when the lighting changes (`RenderWorld::soften_accumulation`): each pixel then keeps `light_history_samples`. A fixed blend limit cannot give a converged picture at rest and no shadow trail in motion. `light_history_samples` stays in the config.
- Step 3: the variance is the larger one of the two measures, not the 3x3 measure only below 4 frames. Lower error in each case.
- Step 5: no on/off key. `denoise_passes = 0` is "no filter"; the mean over time is always on, because it is the accumulation. The debug panel changes `denoise_passes`, `light_history_samples`, and `max_samples` in the config resource.
- Glass is a surface of its own for the denoiser.
- New: benchmark mode `--bench-walk` (scenario `walk_720p`). In benchmark mode each frame is one fixed step of game time.
- Bug found by this stage: the blend of two equal poses (`Transform::blend`) changed the last bit, so an entity at rest was "moved" in some frames. In stage 11 that started the picture again.

Removed paths (step 4):

- `extract_camera` called `reset_accumulation`: now `wake_accumulation` (new samples, full history).
- `extract_instances` called `reset_accumulation`: now `soften_accumulation`.
- `Accumulation::soft_frame`, the `soft_reset` push constant, and the light mark in the alpha of `radiance` (`saw_light`): a pixel that shows the light itself has no history at all.
- The pass `accumulate`, its shader, and the image `accumulated`.
- Left: `reset_accumulation` for the manual action, a config change (the resolution), a window size change, and a change of the mesh store. `soften_accumulation` for a light change.

Not met: `relmse` at `max_samples` at rest is 0.00103, before 0.000924.

## Already there (perf round 3)

- [x] The trace pass outputs `albedo`. `denoise_prepare` demodulates, the last filter pass remodulates.
- [x] Spatial pass: `denoise_passes` (4) edge-aware a-trous passes, guided by normal, distance, and a noise measure.
- [x] Benchmark CSV has a GPU column for each pass of the graph, so new passes show with no edit.
- [x] `FrameGraph::add_history` (stage 10): two images that change places in each frame. Unit test only.

## Steps

### 1. Baseline

- [x] A benchmark scenario with motion: the player walks and a door turns, scripted. Record `relmse` and GPU ms with the current code.
- Check: the numbers are in `tools/bench_history.csv`, to compare each later step.

### 2. Temporal pass

- [x] History images (`add_history`): color, moments (mean and mean of squares of luminance), surface (normal and distance), history length.
- [x] `denoise_temporal`, between `trace` and the spatial passes: reproject with `motion`, reject where distance or normal disagree, blend with a weight from the history length (limits from config).
- [x] The passes of the denoiser run in each frame, so `previous` is always one frame old.
- Check: validation is clean, also with `MAKI_RENDER_SELFTEST=1` (the history is new after a size change). Static view: `relmse` at `max_samples` does not rise against step 1.

### 3. Variance from history

- [x] The spatial filter takes the variance from the moments over time. Where the history is short (below 4 frames), it uses the 3x3 measure that exists.
- Check: `relmse` with motion is lower than with the 3x3 measure alone, at equal GPU time.

### 4. Remove the reset paths

- [x] The global reset stays for the manual action and for a resolution change.
- [x] Remove the reset where the temporal pass now does the work: instance change, camera change, light change (`reset_accumulation` and `soften_accumulation` callers, `light_history_samples`, the light mark in the alpha of `radiance` if it has no other use).
- [x] Decide what `Accumulation` and the `accumulate` pass still do. If the temporal pass with a long history limit gives the same static result, remove the pass.
- [x] The report lists each removed path.
- Check: static views converge as before. A moving light gives no trail.

### 5. Config and debug panel

- [x] Config: denoiser on/off, number of spatial passes, temporal blend limits.
- [x] Debug panel: the same values live, and the views history length, variance, raw against denoised (in the view mode of stage 11).
- Check: each control changes the picture in the frame after the change.

### 6. Measure

- [x] Full benchmark run. GPU cost of the denoiser at the default resolution, in the report.
- [x] Update `docs/perf-improve.md` (D3 is done) and `OPEN_ITEMS.md`.

## Repo state that this stage meets

- The noise measure is the variance of a 3x3 block in one frame, not a variance over time.
- One signal: direct and bounce light are filtered together (`OPEN_ITEMS.md`, "Denoiser, one signal"). Not a task of this stage unless it shows as a defect.
- The spatial passes stop when the accumulation is complete (`Accumulation::sampling`). With the temporal pass they run in each frame, or the history is two frames old.
- Cost now: 0.95 ms at 1080p for prepare and four filter passes.

## What to watch for

- Ghosting: a faint trail behind the player or the door. In the history length view, the history is near zero where the player just left.
- Too much blur: soft shadows become flat, fine detail smears. Compare with the raw view.
- Flicker in a static scene: the temporal blend limits are too aggressive.
- Cost: the denoiser is small next to the trace pass at the internal resolution. If not, find out why.

Background: Schied et al., "Spatiotemporal Variance-Guided Filtering", 2017 (overview and figures).

## Done when

- [x] Player and light movement give a stable, low-noise picture with no smear and no ghost behind moving objects.
- [x] Static views converge as before (`relmse` at `max_samples` does not rise).
- [x] The benchmark runs.
- [x] Validation is clean.
- [x] Stop and report, with the GPU cost of the denoiser at the default resolution.
