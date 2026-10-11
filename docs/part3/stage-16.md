# Stage 16: split signals; mirror and glass

State: done. Plan: `README.md`, section 5.

## 1. What was built

- The trace pass writes `direct`, `indirect`, `specular` (and the emission in their fourth channels) in place of `radiance`. The picture is `albedo * (direct + indirect) + specular + emission`.
- The temporal pass keeps a mean for each of the three, and moments for direct and indirect. The filter runs on direct (1 pass) and on indirect (4 passes), each with its own variance. `specular` gets the mean over time only.
- Mirror: the guides are those of the picture behind the mirror: mirrored normal, full path length, motion of the point at that length along the camera ray.
- Glass that the camera sees: the path splits one time. The path that goes through gives the guides; the reflected path adds to `specular`.
- Debug views: surface color, direct light, specular light. The tonemap pass has one mode value and four image slots for all views.

## 2. Numbers

1 sample for each pixel and frame, against the references of this stage.

| Run | Stage 15: `relmse` / sharpness / noise | Stage 16 |
|---|---|---|
| Rest, 64 frames | 0.000333 / 0.921 / 1.75 | 0.000252 / 1.005 / 1.49 |
| Rest, 1024 frames | 0.0000353 / 0.987 / 1.08 | 0.0000207 / 1.003 / 1.02 |
| `walk_720p`, frame 180 | 0.00481 / 0.837 / 1.44 | 0.00127 / 0.987 / 1.10 |
| `walk_720p`, crop of the large mirror | 0.0230 / 0.889 / 1.48 | 0.00375 / 0.989 / 0.96 |

Gate:
- Shadow edge at rest after 64 frames, crop `700,440,220,150`: sharpness 1.003. Met.
- Behind the glass ball at rest, crop `760,380,160,160`, 64 frames: sharpness 1.001. Met.
- `walk_720p`: `relmse` and sharpness better than stage 15. Met.
- Cost at 1080p, run `s16` against `s15`: `test_1080p` 2.48 -> 3.32 ms (+0.84 ms). `heavy_mixed_lights8_1080p` 4.42 -> 6.02 ms (+1.6 ms; a quarter of its spheres are glass, and each glass pixel has two paths). Trace +20 % and +34 %, temporal +0.16 ms, direct filter 0.4 ms, resolve +0.12 ms.

`OPEN_ITEMS.md`: "Denoiser, one signal" and "Reflections in motion" are closed. Four new items (see section 6).

## 3. Files

- `crates/render/shaders/trace.comp`: `trace_path` with the three sums, the emission, the way to the surface through mirror and glass, the second path.
- `crates/render/shaders/denoise_temporal.comp`: three means, two pairs of moments.
- `crates/render/shaders/denoise_prepare.comp`: one shader for the two kinds of light (`signal`).
- `crates/render/shaders/denoise_resolve.comp`: the sum.
- `crates/render/shaders/tonemap.comp`: modes for the debug views.
- `crates/render/src/denoise.rs`: `Signal`, `SignalImages`, passes `denoise_direct_*`.
- `crates/render/src/passes.rs`, `lib.rs`: trace images, view table, `denoise_direct_passes`.
- `crates/core/src/config.rs`, `app.rs`, `game/src/debug.rs`: `denoise_direct_passes`.

## 4. Deviations

- Emission is a fourth part. The plan puts the light of a lamp that shows as a reflection on glass in `specular`, with a mean over time. That mean has the motion of the surface behind the glass, not of the reflection: the bright point of the lamp on the glass ball left a trail of some pixels, 1000 times brighter than the floor (4 pixels gave `relmse` 0.042 for the picture). Emission has no noise, so it needs no mean.
- Glass on the way to the surface: only the first glass surface splits. Each later one (the far side of the ball) lets the path through with all its light. The plan says "glass deeper in a path stays a random choice": a random choice before the surface would change the guides from frame to frame. After the surface, glass is random as before.
- The mirrored normal. The plan keeps "the guides of the surface that shows". With the true normal and the path length, the plane test of the filter (stage 15) compares points of the picture behind the mirror with a normal of the room: it would reject each neighbor.
- The reflection path on glass ends at its first diffuse surface (no bounce light in a reflection). -11 % trace time in the scene with many glass spheres.
- `albedo` is a 16-bit float image (was 8-bit): the picture is now `albedo * light` with no division by the same value before.

## 5. Decisions

- `denoise_direct_passes = 1`. Measured 0, 1, 2, 3 at 16 and 64 frames and in the walk: 1, 2, and 3 agree within 2 %; 0 has more noise (3.2 -> 4.2 at 16 frames). The lamp of the test room is small; a large light can need 2.
- The mirrored normal uses the first mirror only (a matrix for all mirrors cost 6 to 10 % trace time).
- `specular` history is a 16-bit float image.
- A pixel with glass and no diffuse surface behind it (glass in front of the lamp or of the background) uses the glass as its surface, so its specular light has a history.
- Two runs of the filter shader, as the plan says. Not one shader with two targets.

## 6. Uncertain

- Heavy scene: 6.0 ms of the 8 ms budget at 1080p. Stages 17 and 18 add passes.
- The debug views were not looked at in the running game.
- New open items: reflections of things that move, mirror behind a mirror, light inside glass, emission in a reflection.
