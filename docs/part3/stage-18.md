# Stage 18: anti-aliasing and the upscaler seam

State: done. Plan: `README.md`, section 5.

## 1. What was built

- Jitter: 16 points of the Halton sequence (2, 3). All paths of a pixel in a frame use the same point. `motion` has no jitter.
- Pass `taa` (`crates/render/src/taa.rs`, `shaders/taa.comp`), after `denoise_resolve`, at the output resolution: `denoised` in, `resolved` out. History read with a Catmull-Rom curve (5 taps), limited to the color range of the 3x3 samples (mean +/- 1.25 deviations, and never out of their lowest and highest value). The mean is taken of colors in the range 0 to 1 (Karis), so the edge of the lamp is smooth.
- Scaling: an output pixel takes the nearest sample at its true place; a sample that is not in the square of the output pixel counts less. `tonemap` no longer scales.
- In motion: a point that moves on the screen takes 30 % of the new frame (10 % at rest), and its history counts only if it showed the same surface. A pixel whose direct light is new in the denoiser (stage 17) also takes 30 %.
- `taa = false` and `taa_blend` (0: the plain mean, for the reference).
- `bench.py`: the dump is `resolved`. `relmse` leaves out the 0.01 % of the pixels with the largest error. `--set` can replace the fixed keys (`render_width = 0`).

## 2. Numbers

Correction (stage 20): the trace pass read its jitter from the wrong bytes of the push constants (a `vec2` at an offset that is not a multiple of 8), so its jitter did not agree with that of the pass `taa`. The numbers below are those with this defect. The correct ones are in `stage-20.md`, section 2; the largest change is the sharpness in `walk_720p`: 0.88, not 0.65.

1 sample for each pixel and frame. The reference now has smooth edges (the mean of all jitter points), so the numbers do not compare with stage 17.

| Run | `relmse` / sharpness / noise |
|---|---|
| Rest, 64 frames | 0.000171 / 0.966 / 0.84 |
| Rest, 64 frames, `taa = false` | 0.000469 / 1.318 / 0.95 (stairs) |
| Rest, 1024 frames | 0.000151 / 0.964 / 0.79 |
| `walk_720p`, frame 180 | 0.00176 / 0.649 / 3.83 |
| `walk_720p`, frame 180, `taa = false` | 0.00103 / 1.296 / 0.87 |
| `walk_720p`, frame 220 | 0.000282 / 0.918 / 0.78 |
| Moving light, frame 240 | 0.0189 / 0.41 / 4.35 |
| `render_scale = 0.67`, rest, 256 frames | 0.000768 / 0.46 / 2.29 |
| `render_scale = 0.67`, `taa = false` (nearest sample) | 0.00165 / 1.05 / 5.41 (blocks) |

Gate:
- Edges at rest agree with the reference: sharpness 0.966, no stairs in the crop of the sphere (`s18_f1024.png`). Met.
- No ghost behind the player in `walk_720p`: the surface test replaces the history where another surface was. The picture at frame 180 shows none (`s18_walk.png`). The picture is softer in motion (0.65): see section 6.
- `render_scale = 0.67` looks better than before: half the `relmse` of the nearest-sample picture, no blocks (`v_s67` crop: smooth sphere edge). The bilinear scale of before was not measured again.
- Cost: `taa` 0.10 to 0.14 ms at a 1280 x 720 output; about 0.3 ms at 1920 x 1080 (by the pixel count; the benchmark window is 720p). `tonemap` -0.004 ms. Below 0.5 ms: met.

`OPEN_ITEMS.md`: "Edges and the light at rest" is closed. New: "Anti-aliasing in motion".

## 3. Files

- `crates/render/src/taa.rs` (new): `jitter`, pass `Taa`, images `resolved` and `resolved_previous`.
- `crates/render/shaders/taa.comp` (new).
- `crates/render/shaders/trace.comp`: `push.jitter`.
- `crates/render/src/passes.rs`, `lib.rs`: the pass in the graph, `ExtractedFrame::{jitter, taa, taa_blend}`.
- `crates/render/src/accumulation.rs`: `reset_frame()`.
- `crates/core/src/config.rs`, `app.rs`, `game/src/debug.rs`: `taa`, `taa_blend`.
- `game/src/bench.rs`, `tools/bench.py`: dump of `resolved`, trimmed `relmse`, key replacement.

## 4. Deviations

- The surface test in motion and the faster blend are not in the plan. Without them the edges of all objects had a trail of 1 to 2 pixels while the camera moved (sharpness 0.33): the 3x3 range is wide at an edge, so the color of the object stays in it.
- The link to the denoiser (a pixel with new direct light blends faster) is not in the plan. Without it the anti-aliasing history added about 10 frames to the lag of a shadow that moves.
- `relmse` is trimmed. Four pixels of the lamp reflection on the glass ball, one pixel from their place in the reference, gave 0.27 for a picture that had 0.0018 without them.

## 5. Decisions

- 16 jitter points; blend 0.1 at rest and 0.3 in motion (measured 0.25 and 0.4 in the walk: sharpness 0.64 and 0.73, `relmse` 0.0017 and 0.0014; 0.4 shows more of the stairs).
- The denoiser does not know the jitter: at rest a pixel reads its own history, with no blend between pixels. In motion its history is up to one pixel off (the difference of the jitter of two frames).
- No motion from the nearest of the 3x3 pixels (the usual step for thin objects): the surface test does that work.
- `resolved` is a 16-bit float image. In the reference mode the mean stops to change after about 100 frames (a 16-bit step); the reference has the mean of the denoiser, which is exact, and edges from about 100 jitter frames.
- `taa = false` shows the nearest sample, not a bilinear blend.

## 6. Uncertain

- In motion the picture is softer than at rest, and by the metric softer than with no anti-aliasing (which has stairs). This is the known cost of this method; an upscaler with a sharpen step (FSR 2, XeSS) does better. The owner must judge the running game.
- With `render_scale = 0.67` the picture is smooth and soft.
- Flicker of thin, bright things (the lamp reflection on glass) while the camera moves was not looked at in the running game.
