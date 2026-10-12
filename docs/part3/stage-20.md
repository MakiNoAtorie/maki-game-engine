# Stage 20: samples for the filter (blue noise)

State: built, gate not met, off by default. Also: a defect of stage 18 found and corrected. Plan: `README.md`, section 5.

## 1. What was built

- `crates/render/build.rs` makes one blue-noise pattern, 64 x 64 values (void and cluster, Ulichney 1993), at build time. `crates/render/src/noise.rs` puts it in a buffer.
- The trace pass uses it for four random numbers of the first path of a pixel in a frame, at the surface that the pixel shows: the point on the light and the bounce direction (`blue()` in `trace.comp`). Each frame and each number reads the pattern at another random offset. All other numbers stay white noise.
- Config `blue_noise` (false), with a checkbox in the Debug panel.

## 2. Numbers

`bench.py quality`, 1 sample for each pixel and frame, denoiser and anti-aliasing on. `relmse`:

| Run | White noise | Blue noise | Change |
|---|---|---|---|
| Rest, 1 frame | 0.00386 | 0.00347 | -10 % |
| Rest, 4 frames | 0.00119 | 0.00107 | -10 % |
| Rest, 16 frames | 0.000339 | 0.000318 | -6 % |
| Rest, 64 frames | 0.000203 | 0.000191 | -6 % |
| Rest, 1024 frames | 0.000183 | 0.000181 | -1.5 % |
| `walk_720p`, frame 180 | 0.00224 | 0.00223 | -0.7 % |
| Moving light, frame 240 | 0.0188 | 0.0188 | 0 % |

Sharpness and noise: no change. `gpu_total`: +0.9 %.

Gate: -15 % at equal time. Not met. The second part of the gate ("or the user sees calmer noise in motion") is for the owner: the switch is in the Debug panel, "Path tracer". Row added to `docs/perf-improve.md`, section 11.

A first form used a fixed step for each frame in place of the random offset (a golden-ratio sequence for each pixel and number): -14 % and -20 % at 1 and 4 frames, but +20 % at 16 frames, +51 % at 64, +58 % at 1024. The numbers of one pixel over frames then are on a few lines, and the mean over frames converges to a wrong value for a long time.

### Correction of stage 18

The `Push` block of `trace.comp` had `vec2 jitter` after three 4-byte fields at offset 108. A `vec2` starts at a multiple of 8 bytes, so the shader read it at 112: its X was the Y that the Rust side sent, and its Y was not set. The trace pass and the pass `taa` did not use the same jitter. Now `noise` (4 bytes) is before `jitter`, and `Trace::execute` has an assert for the offset.

The numbers of stage 18 again, with the correct jitter and new references (white noise):

| Run | Stage 18 as measured then | Correct |
|---|---|---|
| Rest, 64 frames | 0.000171 / 0.966 / 0.84 | 0.000201 / 0.931 / 0.88 |
| Rest, 64 frames, `taa = false` | 0.000469 / 1.318 / 0.95 | 0.000707 / 1.624 / 1.04 |
| Rest, 1024 frames | 0.000151 / 0.964 / 0.79 | 0.000182 / 0.928 / 0.83 |
| `walk_720p`, frame 180 | 0.00176 / 0.649 / 3.83 | 0.00224 / 0.876 / 3.41 |
| `walk_720p`, frame 180, `taa = false` | 0.00103 / 1.296 / 0.87 | 0.00141 / 1.620 / 0.95 |
| `walk_720p`, frame 220 | 0.000282 / 0.918 / 0.78 | 0.000312 / 0.909 / 0.78 |
| Moving light, frame 240 | 0.0189 / 0.41 / 4.35 | 0.0188 / 0.50 / 3.48 |
| `render_scale = 0.67`, rest, 256 frames | 0.000768 / 0.46 / 2.29 | 0.000347 / 0.717 / 1.00 |
| `render_scale = 0.67`, `taa = false` | 0.00165 / 1.05 / 5.41 | 0.00173 / 1.41 / 3.56 |

The reference now has smooth edges in X and Y, so a picture with stairs (`taa = false`) is farther from it than before. The picture in motion and the picture at a low render scale are much better than stage 18 reported.

## 3. Files

- `crates/render/build.rs`: `blue_noise()`.
- `crates/render/src/noise.rs` (new): the buffer, a test of the pattern.
- `crates/render/shaders/trace.comp`: `blue()`, the buffer, the order of the `Push` fields.
- `crates/render/src/passes.rs`, `lib.rs`: push fields, `ExtractedFrame::blue_noise`.
- `crates/core/src/config.rs`, `game/src/debug.rs`: `blue_noise`.
- `docs/perf-improve.md` (section 11), `docs/part3/stage-18.md` (a note), `OPEN_ITEMS.md` (numbers).

## 4. Deviations

- One pattern with random offsets, not a set of patterns over space and time (Wolfe et al., 2022). A set that is also blue over time needs an optimizer that runs for minutes; the offsets give white noise over time, which the mean over frames handles as before.
- The code stays in the repository though the gate failed, off by default: the second part of the gate needs the owner's eyes. Remove `noise.rs`, `blue()`, and `blue_noise()` if the owner sees no gain.

## 5. Decisions

- The pattern is made in `build.rs` (0.1 s), not at each start of the game and not as a file in the repository.
- Step 3 of void and cluster fills the largest void to the end (the paper changes the rule at half).

## 6. Uncertain

- Other push constant blocks were checked by hand for the same defect (`taa.comp`, `denoise_filter.comp`, `tonemap.comp`): correct. There is no automatic check of a block against its Rust side.
