# Stage 23: light of many bounces (radiance cache)

State: built in a smaller form than the plan (the picture of the frame before as the cache, not a hash grid in world space). On by default. Plan: `README.md`, section 5.

## 1. What was built

- `max_bounces` (config, 1 to 16, default 2): a path ends after this many diffuse bounces. With 8, the benchmark makes a reference with the light of many bounces (`bench.py reference SCENARIO --set max_bounces=8 max_path_rays=32 bounce_light=false --name SCENARIO_bounces8`).
- `bounce_light` (config, default true): at the last surface of a path that has left a diffuse surface, the trace pass adds the bounce light that the same point had in the frame before: the mean indirect light of the denoiser at the pixel that showed the point (`bounce_light()` in `trace.comp`). The pixel counts only if its surface has the normal and the distance of the point. That mean was made the same way one frame before, so it holds the light of all later bounces.
- Debug panel, "Path tracer": the two controls.

## 2. Numbers

`relmse` against the reference with 8 bounces, 1 sample for each pixel and frame, 1280 x 720:

| Run | 1 bounce (before) | `bounce_light` | 2 bounces | `bounce_light` and 2 bounces |
|---|---|---|---|---|
| `test_720p`, rest, 256 frames | 0.0289 | 0.0134 | 0.0079 | 0.0014 |
| `materials_720p`, rest, 256 frames | 0.0265 | 0.0092 | 0.0067 | 0.0018 |
| `test_720p`, rest, 16 frames | 0.0314 | 0.0175 | - | - |
| `walk_720p`, frame 180 | 0.0337 | 0.0167 | - | - |
| Moving light, 30 frames after the stop | 0.0320 | 0.0171 | - | - |
| `gpu_total`, 720p, ms | 2.05 | 2.12 | 2.30 | 2.38 |

For comparison: the same picture against its own reference (the same code with more samples) has 0.0002. The light that one bounce does not have was the largest error of the picture by a factor of 100, and no earlier stage could see it: each reference had one bounce too.

Gate:
- Dark corners agree with a reference that has many bounces: the error is half (room A) to a third (room C) of what it was. Not all of it: a point that the picture does not show gets no bounce light (the near walls, which the camera looks through; the back of objects). Partly met.
- No light leak through walls: the cache is read only at a pixel whose surface has the normal of the point (cosine 0.9) and its distance from the camera (2 %). No test picture was made for a leak.
- No lag longer than about half a second after a change: 30 frames after the light stops, the error is 0.0171, against 0.0134 at rest. Met, about.
- Cost equal or lower `gpu_trace`: not met. 1080p, run `s23on` against `s21c`: `gpu_trace` 1.16 -> 1.36 ms (`test_1080p`), 5.19 -> 5.56 ms (`heavy_mixed_lights8_1080p`); `gpu_total` 3.89 -> 4.09 ms and 7.31 -> 7.69 ms.

Two bounces with `bounce_light` are 20 times nearer to the reference than one bounce alone, for 16 % more GPU time in the rooms of the game (`test_1080p_bounces2`: 4.66 ms). In the heavy benchmark scene that is 9.3 ms, over the budget of 8 ms.

The numbers of the denoiser against the own reference, with `bounce_light`: rest 64 frames 0.000274 / 0.937 / 0.72, `walk_720p` 0.00247 / 0.907 / 3.04, `materials_720p` 0.000449 / 1.027 / 0.97.

## 3. Files

- `crates/render/shaders/trace.comp`: `bounce_light()`, the bounce count, `flags` and `history_images` in the `Push` block (at its limit of 128 bytes: two image indices share one field).
- `crates/render/src/denoise.rs`, `passes.rs`, `lib.rs`: the history of the mean indirect light is declared before the trace pass, which reads it.
- `crates/core/src/config.rs`, `app.rs`, `game/src/debug.rs`: `max_bounces`, `bounce_light`.
- `tools/bench.py`: scenarios `test_1080p_bounces2`, `heavy_mixed_lights8_1080p_bounces2`.

## 4. Deviations

- Not a hash grid in world space. The cache is the mean indirect light of the frame before, in screen space: no new buffer, no new pass, no cells to clear at a room change (a reset of the picture clears it), and no light leak by construction of a cell. The cost: a point that the picture does not show has no entry. A hash grid adds those points; it needs buffers that two passes share, which the frame graph does not plan yet (`OPEN_ITEMS.md`, "Buffers and the TLAS in the frame graph").
- No longer paths for 4 % of the pixels: the cache updates itself, each frame from the one before.
- Stage 22 (its gate was not met) is not a base of this stage, as the plan had it.

## 5. Decisions

- `max_bounces = 2` (owner, 2026-10-11; the stage first had 1). Runs `part3end` and `part3end_b2`: `gpu_total` 4.12 -> 4.69 ms in `test_1080p`, 7.71 -> 9.34 ms in `heavy_mixed_lights8_1080p`, which is over the budget of 8 ms.
- The cache value has the limit `indirect_clamp`, as all bounce light.
- A pixel behind a mirror or glass has the guides of the picture behind the mirror: its distance does not agree with that of a true point, so it is never read as a cache entry.
- The reference of the benchmark (`bench.py reference`) is the default config, so it has `bounce_light` too. The references with 8 bounces have their own names.

## 6. Uncertain

- The picture gets brighter in corners and shadows: by eye, the rooms may now need a lower `exposure` or weaker lamps for the same mood.
- The cache reads a mean that has noise where its history is young, and the result feeds the next frame. No flicker shows in the numbers (noise 0.72 at rest); it was not looked at in the running game.
- Against its own reference, the picture after 1024 frames has more error (0.00039) than after 64 (0.00027): the mean of the picture and that of the reference go to the fixed point of the feedback by slightly different ways. Small against the 0.013 that is left to the 8-bounce reference.
