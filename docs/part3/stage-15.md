# Stage 15: metric, and denoiser fixes with one signal

State: done. Plan: `README.md`, section 5.

## 1. What was built

- `bench.py quality` prints `relmse`, `sharpness`, and `noise`, and writes `<label>.png` (picture, reference, 4 x difference). `--crop X,Y,W,H` measures a part.
- Trace: `indirect_clamp` limits the light of a bounce ray. Each path of a pixel goes through the pixel center.
- Denoiser: variance over time after 4 frames; plane distance in place of the camera distance; a filter pass does nothing for a pixel with many samples (`denoise_reach`).
- Debug views: filter input, filter variance, filter output.

## 2. Numbers

`test_720p` and `walk_720p`, 1 sample for each pixel and frame. `sharpness`: gradient energy at the edge pixels of the reference, picture over reference. `noise`: the same ratio at all other pixels.

| Run | Before: `relmse` / sharpness / noise | Stage 15 |
|---|---|---|
| Rest, 64 frames | 0.00498 / 0.918 / 2.12 | 0.000333 / 0.921 / 1.75 |
| Rest, 1024 frames | 0.00150 / 0.917 / 1.95 | 0.0000353 / 0.987 / 1.08 |
| Rest, 1024 frames, crop of the glass ball | - | 0.000216 / 0.974 / 1.08 |
| `walk_720p`, frame 180 | 0.00686 / 0.891 / 2.14 | 0.00481 / 0.837 / 1.44 |

The two columns do not use the same reference (see section 4): compare the sharpness and the noise, not the `relmse`.

Gate: sharpness at rest at 1024 samples is 0.987 (0.98 or more: met). `relmse` of `walk_720p` did not rise. The fixed number 0.000924 of the plan belongs to the old reference; against the new one the picture at 1024 samples has 0.0000353, and the mean alone (`denoise_passes = 0`) has 0.000249.

Section 2 of the plan, confirmed: items 3 and 6 (rest sharpness 0.917 with a filter that never turned off), items 2 and 5 (crop of the glass ball and of the spot below it, `base15_f1024.png`). Items 1, 4, 7, 8 show in `walk_720p` (sharpness 0.84): stages 16 and 17.

GPU time, 1080p, run `s15` against `stage14full`: a filter pass costs 0.21 to 0.25 ms (before 0.14 to 0.19): the plane test computes a camera ray for each of the 25 points. `gpu_total`: `test_1080p` 2.21 -> 2.48 ms, `heavy_mixed_lights8_1080p` 4.14 -> 4.42 ms.

## 3. Files

- `tools/bench.py`: sharpness and noise, crop, side-by-side picture, `reference --set --name`.
- `crates/core/src/config.rs`, `app.rs`: `denoise_reach`, `indirect_clamp`.
- `crates/render/shaders/trace.comp`: `limited()`, pixel center for each path.
- `crates/render/shaders/denoise_prepare.comp`: variance rule.
- `crates/render/shaders/denoise_filter.comp`: plane test, `reach`.
- `crates/render/shaders/tonemap.comp`, `src/lib.rs`: three debug views.
- `crates/render/src/denoise.rs`, `passes.rs`: image `denoise_input`, push constants, `frame_ray_basis`.
- `game/src/debug.rs`: two sliders.

## 4. Deviations

- The reference has `indirect_clamp` of the default config and no filter (`denoise_passes = 0`). The plan compares with the old reference (no clamp, 4 filter passes). The clamp changes what the picture converges to; a reference without it would count that as an error of the denoiser. The cost of the clamp alone: `relmse` 0.00745 between the two references, 2.8 % less light in the picture. The spot of light under the glass ball is almost gone, and the floor in front of a mirror is darker (`OPEN_ITEMS.md`, "Light by way of a mirror or through glass").
- The second number of the gate is two numbers. One ratio over the full picture was 1.02 for the old, soft picture: blur at the edges and noise in the flat parts cancel. `sharpness` counts the edge pixels of the reference, `noise` the rest.
- Each path of a pixel goes through the pixel center (step 1 of stage 18). With more than one sample for each frame the picture has no smooth edges until stage 18. Without this, the reference had smooth edges and the picture at 1 sample did not: that difference was most of the `relmse`.

## 5. Decisions

- `indirect_clamp = 8`. Measured 2, 8, 32 against a reference with the same value, 64 frames: `relmse` 0.00068, 0.00083, 0.0019. 8 is about four times the direct light near the lamp.
- `denoise_reach = 4096`. Measured 0, 256, 1024, 4096 at rest, 1024 frames: sharpness 0.984, 1.002, 0.992, 0.987; `relmse` 0.000039, 0.00025, 0.000050, 0.000035. In `walk_720p`, 1024 is worse than 4096 (`relmse` 0.0054 against 0.0048): a pixel there has about 32 samples and needs all four passes.
- `SIGMA_PLANE = 0.005` (5 cm at 10 m), not tuned.
- The temporal pass keeps its test of the camera distance: the two frames have two cameras, and the test compares one pixel with its own history.

## 6. Uncertain

- The debug views were not looked at in the running game. The numbers and the dumps confirm section 2.
- `noise` at 4 frames is 22 (before 2.5): the variance over time knows nothing in the first frames, and the 3x3 measure alone is weak. Stage 17, step 6.
