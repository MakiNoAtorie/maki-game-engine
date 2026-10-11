# Stage 17: history confidence for each pixel

State: done, with one part of the plan not met (section 4, first item). Plan: `README.md`, section 5.

## 1. What was built

- A fast mean of the direct light (4 frames, image `denoise_fast_direct`).
- A test for each pixel in the temporal pass: the mean of the fast means and the mean of the long means of a 5x5 block must agree within `history_clamp` (2) standard deviations of the noise of the block mean. Where they do not, the history of the direct light counts as 4 frames.
- A sample count for each kind of light (`denoise_direct.a`, `denoise_indirect.a`).
- A pixel with no valid history looks in the 3x3 block around its old place.
- Variance: the indirect light uses the larger one of "over time" and "over space" in all frames; the direct light for 16 frames (was 4, for the two).
- `--bench-light-frames N` and `bench.py quality --light-frames N`: the light moves, then stops.

## 2. Numbers

1 sample for each pixel and frame.

| Run | Stage 16: `relmse` / sharpness / noise | Stage 17 |
|---|---|---|
| Rest, 4 frames | 0.00799 / 1.013 / 24.8 | 0.00104 / 0.996 / 1.53 |
| Rest, 64 frames | 0.000252 / 1.005 / 1.49 | 0.0000985 / 1.006 / 1.09 |
| Rest, 1024 frames | 0.0000207 / 1.003 / 1.02 | 0.0000207 / 1.003 / 0.99 |
| `walk_720p`, frame 180 | 0.00127 / 0.987 / 1.10 | 0.000633 / 0.977 / 1.05 |
| Moving light, frame 240 | 0.0952 / 0.857 / 2.08 (`history_clamp = 0`, 32 samples) | 0.0164 / 0.859 / 1.80 |
| 6 frames after the light stops | 0.0660 | 0.00430 |
| 20 frames after | 0.0282 | 0.00103 |

The moving light of the benchmark is fast: 3.8 m/s.

Gate:
- `relmse` with a moving light better than with `light_history_samples = 32`: 0.0164 against 0.0952. Met.
- No trail longer than about 6 frames: 6 frames after the light stops, the shadows are at their place (`s17u_k6.png`), and the `relmse` is that of a picture that started 4 to 6 frames before (0.0043; rest after 4 frames: 0.0010, with the indirect light still short). Met for the direct light. The indirect light follows in about 16 frames.
- At rest: no rise. Met (1024 frames: equal; 64 frames: better, from the variance change).
- GPU time: no change (`s17` against `s16`: `test_1080p` 3.32 ms, `heavy_mixed_lights8_1080p` 6.04 ms). The temporal pass is 5 to 12 % slower, the prepare pass of the indirect light 20 % faster.

`OPEN_ITEMS.md`: "Shadow trail" is closed. "Flicker behind a moving object" stays, with its new state. New: "Indirect light follows late".

## 3. Files

- `crates/render/shaders/denoise_temporal.comp`: fast mean, `direct_confidence`, 3x3 search, two sample counts.
- `crates/render/shaders/denoise_prepare.comp`: variance rule for each kind of light.
- `crates/render/src/denoise.rs`: image `denoise_fast_direct`, limits for the temporal pass.
- `crates/core/src/config.rs`, `app.rs`, `crates/render/src/lib.rs`, `game/src/debug.rs`: `history_clamp`; `light_history_samples` is 16 and is for the indirect light.
- `crates/render/src/accumulation.rs`: comment only.
- `game/src/bench.rs`, `main.rs`, `tools/bench.py`: `--bench-light-frames`.

## 4. Deviations

- Step 4 is not done: `soften_accumulation`, `light_history_samples`, and the `Extract` hint stay, for the indirect and the specular light. The test for each pixel works for the direct light only. For the indirect light, the same test (ReLAX form, 5x5 range of fast means) raised the `relmse` at rest from 0.000021 to 0.000057 and changed nothing in motion: at one sample, its noise is larger than most changes. Without the hint (`light_history_samples = 1024`) the old bounce light stays for seconds.
- The test is not the clamp of the plan. Three forms were measured with the moving light (frame 240, `relmse`):
  1. ReLAX: clamp the long mean to mean +/- 2 deviations of the 5x5 fast means. 0.028. The range is wide in each soft shadow (the fast means there differ by noise), so the test does nothing where a shadow moves.
  2. Clamp to the fast mean of the pixel, range from the variance over time. 0.35: that variance holds the change itself.
  3. Block means, as built. 0.021 with 32 indirect samples, 0.016 with 16. The long mean is not moved to a value of the block, so nothing blurs at rest.
- Step 6 (more filter passes for young pixels): not built. Three direct passes with a low `reach` (young pixels only) changed no number. The longer use of the 3x3 variance did (table, 4 frames).

## 5. Decisions

- `history_clamp = 2`. Measured 1, 2, 3, 4: moving 0.0147, 0.0164, 0.0180, 0.0196; rest at 1024 frames 0.0000251, 0.0000207, 0.0000202, 0.0000201. 1 starts means again from noise alone.
- `light_history_samples = 16` (was 32). Measured 8, 16, 32: walk 0.00062, 0.00063, 0.00072; 6 frames after the stop 0.0025, 0.0043, 0.0083. 8 is better by these numbers, but 8 samples can show as flicker between frames, which the metric of one frame does not see.
- The hold after a change (`fade_frames`) stays: without it, 60 frames after the stop the `relmse` was 0.0013 against 0.00046.
- The specular light has no test; its mean has 64 samples at most.
- The block has no surface test: the two means use the same pixels, so another surface in the block is in the two.

## 6. Uncertain

- Flicker between frames is not measured. A metric over two frames of a sequence would show it.
- Sharpness with the moving light is 0.86: the shadow edges are 4 frames behind and their penumbra has a young history. At the speed of a walking player (`walk_720p`) it is 0.98.
