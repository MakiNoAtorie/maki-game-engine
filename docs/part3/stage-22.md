# Stage 22: ReSTIR GI

State: built, measured, gate not met, taken out again. The code is commit `cb76c46`; commit `3ab46a9` removes it. Plan: `README.md`, section 5.

## 1. What was built (for the measurement)

- Trace pass: with `restir_gi`, the bounce light of the first path of a pixel is not added to `indirect`. The pass writes the sample: the point that the bounce ray found, its normal, and the light from there. A bounce ray that finds a mirror, glass, or nothing counts as a black sample (its light stays in `indirect`).
- Pass `restir_gi` (`shaders/restir_gi.comp`, `src/restir.rs`): one reservoir for each pixel in three history images. Each frame: the new sample, the reservoir of the same point in the frame before (20 samples at most; 4 while the lighting changes), the reservoirs of 2 random pixels within 16 pixels of that place, with the Jacobian of the change of the surface point and one visibility ray for the chosen sample. The light of the chosen sample, times its weight, is added to `indirect`. The denoiser did not change.

## 2. Numbers

`bench.py quality`, 1280 x 720, `relmse` / sharpness / noise.

First the ceiling: what more samples for each frame gain with the pipeline of stage 21, with no new code.

| Run | 1 sample | 2 samples | 4 samples | 16 samples |
|---|---|---|---|---|
| Rest, 4 frames | 0.00120 | 0.00075 | 0.00056 | 0.00043 |
| Rest, 16 frames | 0.000337 | 0.000257 | 0.000221 | 0.000201 |
| Rest, 64 frames | 0.000201 | 0.000186 | 0.000178 | 0.000174 |
| `walk_720p`, frame 180 | 0.00224 | 0.00214 | 0.00208 | 0.00201 |
| `materials_720p`, 16 frames | 0.000515 | 0.000377 | 0.000323 | 0.000287 |
| `gpu_total`, ms | 2.07 | 2.47 | 3.31 | 8.52 |

After stages 15 to 18, the sample noise is a small part of the error that is left: 16 samples in place of 1 take away 14 % at rest (64 frames) and 10 % in the walk. The rest is the anti-aliasing (the picture against the mean of all jitter points) and, in motion, light that follows late. Noise is most of the error only for a young picture (4 frames: 64 %).

ReSTIR GI at 1 sample, against 1 and 2 samples without it:

| Run | 1 sample | 2 samples | ReSTIR GI, time and 2 pixels | ReSTIR GI, time only |
|---|---|---|---|---|
| Rest, 4 frames | 0.00120 | 0.00075 | 0.00123 | 0.00141 |
| Rest, 16 frames | 0.000337 | 0.000257 | 0.00117 | - |
| Rest, 64 frames | 0.000201 | 0.000186 | 0.00108 | 0.000205 |
| Rest, 1024 frames | 0.000184 | - | 0.000838 | 0.000171 |
| `walk_720p`, frame 180 | 0.00224 | 0.00214 | 0.00260 | 0.00228 |
| `materials_720p`, 64 frames | 0.000308 | 0.000281 | 0.00126 | - |

Sharpness: 0.92 against 0.93 at rest, 0.86 against 0.88 in the walk. Cost: +0.05 ms (720p, rest) to +0.25 ms at 1 sample; with the switch off, the trace pass was 17 % slower (`s22gi`: the loop has more values that stay alive over each ray).

Gate: lower `relmse` and no lower sharpness than more samples at equal GPU time, in both rooms, at rest and in `walk_720p`. Not met in any run.

- With reuse over pixels, the picture converges to a wrong value (`relmse` 0.00084 at 1024 frames, 4.5 times that of the mean alone). This is the known bias of the cheap form: a sample of another pixel counts as if this pixel could have made it, with one visibility ray after the choice.
- With reuse over time only, there is no bias and no gain: the denoiser already keeps a mean of up to 1024 frames for each pixel, and a reservoir that keeps one sample for 20 frames gives that mean the same sample 20 times.

Row added to `docs/perf-improve.md`, section 11.

## 3. Files

In the repository after this stage: this report, the row in `docs/perf-improve.md`, the runs in `tools/bench_history.csv`. The code is in commit `cb76c46` only (`crates/render/shaders/restir_gi.comp`, `crates/render/src/restir.rs`, and changes in `trace.comp`, `passes.rs`, `lib.rs`, `config.rs`).

## 4. Deviations

- The form that was built is the cheap one: reservoirs of the frame before for time and space in one pass, 2 pixels, weights by sample count (`1/M`), one visibility ray. The paper also has a form without bias (a visibility ray for each pixel that takes part); it costs 2 to 3 more rays for each pixel, which is more than 2 samples for each frame cost, and 2 samples is what the gate compares with.
- The code is not in the tree, not behind a switch that is off: off, it cost 17 % trace time and 8 images at the internal resolution.

## 5. Decisions

- A bounce ray that finds no diffuse point is a black sample for the reservoir. Without this the picture was 20 % too bright in the test room (open at the top: many bounce rays leave it).
- A sample is made only for a pixel that shows its surface with no mirror or glass on the way.

## 6. Uncertain

- A young picture (the 4 frames after a pixel comes into view) is the one case where the noise is most of the error. Reuse over pixels helps there in theory; this form did not (0.00123 against 0.00120), probably because the reservoirs of the pixels around are as young. `OPEN_ITEMS.md`, "Flicker behind a moving object", stays.
- The trigger of the plan for a new try: a scene where the bounce light is most of the picture's error at 1 sample (large dark rooms lit by a small bright area), or a GPU budget for the form without bias.
