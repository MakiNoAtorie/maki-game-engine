# Performance audit and improvement plan

Audit of commit `47499c1` (stage 9). Measured on an NVIDIA RTX 5060 Ti, driver 591.86, Windows 11, release build.
This document is the work order for later sessions. Applied so far: T1 to T3, P1 to P4, R1 to R4, D1 (the part that D2 needs), D2, D5 steps 1, 2, and 4, and one change that was not in the plan (E1). T4 was done as a measurement. Section 0 has the results. Sections 2 to 10 are the plan as written before that work: their line numbers and "where" notes are of commit `47499c1`. Each experiment was a temporary edit of the engine source that was reverted after its run.

Sources: the code of this repository, the measurements in `tools/bench_history.csv`, and two research reports in the Downloads folder of the user, both of 2026-10-11: `Real-Time_RT_Survey.md` ("the survey" below) and `RT_Path_Tracing_2025.md` ("the second survey").

Goal, set by the user: get all the performance that the code can give. The test scene is small, so the plan is measured on a load of the size of a game level (section 3.2), and it has two tracks: more samples in the time of a frame, and less time for each ray.

Contents:

0. Status after three rounds
1. Rules for the session that applies this plan
2. Benchmark tools
3. Baseline
4. Main result
5. Plan, in order
6. Tool work that comes first (T1 to T4)
7. Samples for each frame (P1 to P5)
8. Cost of a ray (R1 to R5, S1, S2)
9. Features from the survey (D1 to D5)
10. Small or conditional changes (C1 to C5)
11. Measured and rejected
12. What the plan takes from the two surveys
13. Not a bottleneck
14. Appendix A: places that need rework (the content of this file before the audit)

## 0. Status after three rounds

Runs `loop1` (round 1) and `loop2` (round 2) in `tools/bench_history.csv`. Same GPU and driver as the audit. Work tree on top of `47499c1`, not committed.

### 0.1 Result

`gpu_total`, median in milliseconds, one sample for each pixel and frame.

| Scenario | Before | Round 1 | Round 2 | Change |
|---|---|---|---|---|
| `test_360p` | 0.181 | 0.147 | 0.147 | -18.8 % |
| `test_720p` | 0.463 | 0.429 | 0.435 | -6.0 % |
| `test_1080p` | 1.259 | 1.188 | 1.106 | -12.2 % |
| `test_2160p` | 4.266 | 4.092 | 4.152 | -2.7 % |
| `stress_720p` | 0.649 | 0.505 | 0.485 | -25.3 % |
| `stress_merged_720p` | 0.562 | 0.502 | 0.483 | -14.1 % |
| `heavy_1080p` | 2.057 | 1.552 | 1.429 | -30.5 % |
| `heavy_merged_1080p` | 1.599 | 1.542 | 1.288 | -19.4 % |
| `heavy_mixed_1080p` | 3.760 | 2.839 | 2.713 | -27.8 % |

"Before" is `baseline2`, `heavy_baseline`, and `after_T3`. New scenarios, round 2 only: `test_1080p_spp4` 3.579, `heavy_mixed_1080p_spp4` 9.338 (2.33 ms for each sample, against 2.71 with one sample in a frame), `heavy_mixed_1080p_rays6` 2.291 (-15.6 % against 12 rays).

The test scene gains little in `gpu_trace` (-2 % at 1080p, inside the noise at the other sizes): its gain is the TLAS build and the `accumulate` pass. The scenes with many triangles gain in `gpu_trace` (-13 % to -29 %).

### 0.2 What was applied

Each step was measured on `heavy_1080p` and `heavy_mixed_1080p` only (runs with the label of the step). The numbers are `gpu_total`, diffuse and mixed, against the step before.

| Step | Change | Gain | Picture |
|---|---|---|---|
| T2, a part | `Gpu::read_image`, `FrameGraph::image`, `--bench-dump FILE`, `bench.py same <scenario> <label_a> <label_b>`. `bench.py run` dumps the last frame of each scenario to `target/bench/<scenario>/<label>.dump`. No `quality` command, no reference pictures, no `--bench-static` | - | Two runs of one build are equal bit for bit |
| P2 | No TLAS build when the instance list is the same | -6.1 %, -4.9 % | Equal |
| R2 | `motion` image removed: no gain. Depth and normal stored at the first ray of the path, not after the loop: the full gain of "no store" | -7.0 %, -7.5 % | Equal |
| P4 | `radiance` as `Rgba16Float` | -2.7 %, -1.9 % | 3 % of the pixels differ, by the rounding to 16 bits |
| R1 + R3 | Packed normals, three for each triangle, in one buffer for each mesh group. One BLAS for all meshes of a model. `MeshEntry` is 32 bytes. Vertex and index data live only for the BLAS build | -11.3 %, -15.3 % | 0.4 % to 0.8 % of the pixels differ (16-bit normals change the result of some shadow and bounce rays). The means agree within 0.4 % |
| P1 step A | `samples_per_frame` (1 to 64) | 8 % less for each sample at 4 samples | Equal at 1 sample |
| R4 steps 1, 2 | `max_path_rays` (2 to 32, default 12). The last surface of a cut path counts as diffuse | 6 rays: -13 % (mixed) | 128 pixels differ at 12 rays |
| E1, new | One ray query in the path loop for path rays and shadow rays | -6.8 %, -4.4 % | Equal |

Notes:

- R2: the cost was not the stores. Two values (`depth`, `normal`) that stay alive from the first ray to the end of the path made the pass 8 % slower. This did not hold as a general rule: see the two rejected state changes in section 11.
- R1 and R3 were measured together. R1 alone needs one more buffer for each mesh, and 1000 meshes then fill the bindless buffer array (4096 slots) during the load, because a destroyed buffer keeps its slot until the next frame. `Gpu::create_blas` now takes the triangle data as slices and frees its work buffers at once.
- R3: a model is one BLAS. The mesh instances of one model with one transform are one TLAS instance. A mesh of a model that is drawn alone, or with another transform, draws the full model; `BuildTlas` logs a warning one time. C4 is no longer necessary for a model: the 1000 meshes of `heavy_1080p` are one build.
- E1: a GPU runs the 64 pixels of a work group in step. With two ray query sites, a pixel that waits at one site stands still while the others trace at the other site. Section 8, S1, names the same effect.
- Validation is clean (test scene, `MAKI_RENDER_SELFTEST=1`, `heavy_mixed_1080p` with 4 samples; debug build).

### 0.3 Round 3: picture quality for the same time

Run `loop3`. Rounds 1 and 2 made a sample cheaper. Round 3 makes the picture better for the samples that it has, so its numbers are errors (`relmse` against a reference of 65536 samples, section 0.5), not frame times. The frame is longer by the denoiser.

| Step | Change | Result |
|---|---|---|
| T1 | `--bench-static`; scenarios `static_1080p`, `idle_1080p` | An idle frame is 0.048 ms |
| T2, rest | `bench.py reference`, `bench.py quality` | 4 times the samples give a quarter of the error (0.341 at 64, 0.085 at 256, `test_720p`) |
| P3 | A light change keeps `light_history_samples` (32) samples. Pixels whose history shows the light itself start again | Moving light, 1 sample: `relmse` 21.1 to 0.135 (`test_720p`), 24.7 to 2.10 (`heavy_mixed_1080p`). `accumulate` +0.04 ms to +0.08 ms |
| P1 step B | `samples_per_frame = 0` (the new default) fills `trace_budget_ms` (6.0) | `gpu_trace` 5.3 to 5.9 ms on `test_720p`, 4.9 ms on `heavy_mixed_1080p` |
| R4 step 3 | Default `max_path_rays` 12 to 8 | See the table below |
| D1, D2 | `normal` holds the distance, new `albedo` image, both for the first diffuse surface of the path. Denoiser: one prepare pass and `denoise_passes` (4) filter passes | Static light: `relmse` 22.9 to 1.23 at 1 sample, 5.55 to 0.104 at 4, 1.42 to 0.027 at 16, 0.085 to 0.0037 at 256 (`test_720p`). `heavy_mixed_1080p`: 5.85 to 1.33 at 4, 0.408 to 0.098 at 64. Cost: 0.48 ms at 720p, 1.07 ms at 1080p, 4.5 ms at 2160p |
| D5 steps 1, 2, 4 | Light buffer (256 lights), resampled pick from 8 candidates, point lights from the glTF file | 9 lights against 1: `gpu_trace` +22 % (diffuse), +30 % (mixed). 8 candidates against a uniform pick: `relmse` 0.027 against 0.112 at 64 samples for 10 % more time |
| T4 | Ray counter, as a temporary edit | Time for each ray: 0.299 ns diffuse, 0.386 ns mixed, 0.178 ns test scene. Rays for each pixel: 2.23, 3.16, 2.81 |

Path limit (R4 step 3), `heavy_mixed_1080p`, no denoiser:

| `max_path_rays` | `gpu_total` | `relmse` at 256 samples | `relmse` at 8192 samples (the bias) |
|---|---|---|---|
| 4 | 2.019 | 0.054 | 0.0157 |
| 6 | 2.356 | 0.071 | 0.0053 |
| 8 | 2.569 | 0.087 | 0.0036 |
| 12 | 2.794 | 0.100 | 0.0027 |

A lower limit has less error at 256 samples: the long paths through glass bring most of the noise. The bias shows only at thousands of samples. 8 costs 8 % less than 12 with a bias of about 0.001; 6 costs 16 % less with a bias of 0.003. The user can set 12 again in the config.

Notes:

- P3 without the rule for the light itself made the error larger (5.6 to 23.4 at 4 samples, `test_720p`): the light sphere is some hundred times brighter than a surface, and its old places stayed in the history as a trail. The trace pass marks a pixel whose path found a light (alpha of `radiance`), and the mark stays in the mean (alpha of `accumulated`).
- D2 is one signal: direct and bounce light are not apart, and there is no reprojection (D3). The filter measures the noise as the variance of a 3x3 block. `OPEN_ITEMS.md`, "Denoiser, one signal".
- Cost of round 3 for the path tracer alone (`test_1080p_raw`, `heavy_mixed_1080p_raw`: no denoiser, no history, 12 rays): `gpu_trace` 0.895 and 2.775 ms. The same load was 0.867 ms (run `T1`) and 2.617 to 2.671 ms (runs `E1r`, `preD`) before round 3: +3 % to +6 %. The run `loop2` shows less (0.824 ms), but that run was 5 % faster than the runs before and after it with the same code. The light mark, the two surface stores, and the index of the light buffer were each removed as a test (runs `xmark`, `xstores`, `nonu1`): each is below 1 %. Compare a change only with a run of the same session.
- T4: the mixed load has 41 % more rays and each ray costs 29 % more. This is above the limit of 1.2 that section 6 sets for S1.
- The pass columns moved: `tonemap` is 0.027 ms after the denoiser (0.073 before), and `trace` took a part of the difference. Judge by `gpu_total` (section 2.6).

### 0.31 Stage 12: temporal reprojection (D3)

Done in part 2, stage 12 (`docs/part2/stage-12-denoiser.md`). The `accumulate` pass is gone: `denoise_temporal` keeps the mean of each pixel and finds it again with the `motion` image. A camera change keeps the full history; a change of the lighting (a light, a moving object) keeps `light_history_samples` for each pixel.

`relmse` at `samples_per_frame = 1`, 1280 x 720, against 65536 samples (`bench.py quality`):

| Scenario | Before (spatial filter, reset on each change) | Stage 12 |
|---|---|---|
| `walk_720p`, frame 180 (player walks, door opens, camera follows) | 0.515 | 0.00703 |
| `test_720p`, 64 frames at rest | 0.00545 | 0.00453 |
| `test_720p`, 1024 samples at rest | 0.000924 | 0.00103 |

The variance for the filter is the larger one of the variance over time and the variance of the 3x3 block. Measured, in the same order as the table: over time alone 0.00700, 0.00838, 0.00464; the 3x3 block alone 0.0104, 0.00474, 0.00104. The variance over time alone misses the rare bright samples that a neighbor pixel got.

Glass is now a surface of its own for the denoiser (normal of the glass, white color). With the surface behind the glass, which differs from frame to frame, the history was lost in half of the frames: 0.0111 at 1024 samples.

The 12 % rise at 1024 samples at rest is not explained. Probable cause: the filter blurs inside glass now.

GPU time of the denoiser passes, median (run `stage12`): 0.13 ms at 640 x 360, 0.76 ms at 1280 x 720, 1.34 ms at 1920 x 1080 (`denoise_temporal` 0.375, `denoise_prepare` 0.205, four filter passes 0.668, `denoise_resolve` 0.091). Before, `accumulate`, `denoise_prepare` and four filter passes: 0.95 ms at 1920 x 1080.

### 0.35 Pass over the interactions (run `loop4`)

A check of the parts against each other, after round 3. `gpu_total` against `loop3`: -2 % to -8 % at 1080p and 2160p, -15 % to -20 % at 720p.

| Change | Result |
|---|---|
| Noise measure of `denoise_prepare` from a 3x3 block (was 5x5) | The pass 0.285 to 0.184 ms at 1080p. `relmse` at 4 samples 0.033 to 0.024 (`test_720p`), 0.971 to 0.881 (`heavy_mixed_1080p`); equal at 64 samples and with a moving light |
| The two edge tests of `denoise_filter` as one exponential | The four filter passes 0.786 to 0.763 ms at 1080p. Same picture |

The denoiser is now 0.95 ms at 1080p. Nothing else was found: the CPU frame is equal to the GPU frame in all scenarios but `idle_1080p` (0.18 ms), and the other tests of this pass are in section 11.

### 0.4 Not done, and why

| Item | Why |
|---|---|
| S1 | T4 gives the case (1.29 times the time for each ray). The test needs a ray tracing pipeline, which changes the stack of `CLAUDE.md`: a decision of the user. The form with two dispatches does not sort the threads, so it cannot show the gain |
| D4 | Large, and it changes the picture (a cache is biased). The measured upper limit is -15 % of `gpu_trace`. With D2 in place its value is the light of many bounces, not the time. Decide after D3: ReSTIR GI needs D3 |
| D5 step 3 | Only for hundreds of lights |
| C1, C2, C3, C5, S2 | Their conditions (section 10) are not true yet |

### 0.5 How to measure quality

```
python tools/bench.py reference test_720p                      # one time, 65536 samples
python tools/bench.py quality test_720p --label a --frames 64  # static light
python tools/bench.py quality test_720p --label b --frames 720 --moving --set light_history_samples=0
```

`quality` prints `relmse`, the median `gpu_total`, and their product with the frame count. Compare two settings at equal GPU time. A run with `--moving` must have a multiple of 240 frames: the light is then at the place of the reference. References exist for `test_720p`, `heavy_mixed_1080p`, and `heavy_lights8_1080p`; a reference is valid for one scene, resolution, and light.

## 1. Rules for the session that applies this plan

- Obey `CLAUDE.md`: one stage at a time, stop and report after each stage, architecture rules 1 to 6.
- One item of this plan is one stage, unless the user groups items.
- For each item:
  1. `python tools/bench.py run --label before_<id>` (skip if the newest `baseline` run is of the parent commit).
  2. Make the change.
  3. Debug run with validation: `MAKI_LOG=debug cargo run -p maki-game -- --bench-frames 300 > log 2>&1`, then search `log` for `vulkan`, `ERROR`, `panicked`. No match is permitted.
  4. `cargo test --workspace`.
  5. `python tools/bench.py run --label after_<id>`, then `python tools/bench.py compare before_<id> after_<id>`.
  6. Report the compare lines of the columns that the item names. Do not report a gain that is below the noise limit (section 2.3).
- An item that changes only speed must not change the picture. After T2 exists, prove it: the frame dump before and after must be equal.
- An item that changes the picture (P1 with more than one sample, P3, R1, R4, R5, all D items) is judged by error at equal GPU time (T2), not by frame time.
- If the measured gain is below the noise limit, revert the change and write the result in section 11.
- The numbers in this document are for the GPU named above. Record a new baseline on other hardware first.

## 2. Benchmark tools

### 2.1 Files

| File | Role |
|---|---|
| `tools/bench.py` | Runs the scenario matrix, appends summary rows, compares runs |
| `tools/gen_scene.py` | Seeded generator of a stress scene (`.glb`): spheres, with optional mirrors, glass, and point lights |
| `tools/bench_history.csv` | Stored results. Committed. One row for each scenario and column |
| `tools/bench_env.csv` | One row for each run: commit, GPU, driver, OS, build, frames, warmup |
| `target/bench/<scenario>/` | Work files of the last run: `config.toml`, the generated scene, `bench.csv` (one row for each frame), `log.txt`. Ignored by git |

Each tool file starts with a `# tool:` line. List the tools with `rg -n "^# tool:" tools`.
The tools use the Python standard library only (Python 3.9 or later).

### 2.2 Commands

```
python tools/bench.py run --label <label>            # all scenarios, about 1 minute
python tools/bench.py run --label <label> --only test_1080p stress_720p
python tools/bench.py compare <label_a> <label_b>    # CSV: name, median a, median b, change in percent
python tools/bench.py selftest                       # checks the statistics against known answers
python tools/gen_scene.py --objects 256 --segments 48 --seed 1 --out target/bench/stress.glb
python tools/gen_scene.py --objects 1000 --segments 64 --mirror-share 0.25 --glass-share 0.25 --lights 8 --out target/bench/mixed.glb
```

- `run` builds the release binary, then runs `maki-game config.toml --bench-frames 700` for each scenario. The first 100 frames are not measured.
- A label is a part of the `run_id`. `compare` uses the newest run that contains the label.
- `commit` has the suffix `-dirty` when the work tree differs from `HEAD`.
- History row: `run_id,commit,name,params,n,unit,median,p95,p99,min,max`. `name` is `<scenario>/<column>`. `gpu_total` is the sum of the pass columns of each frame.
- The window must stay visible during a run. A minimized window stops the frames, and the run does not end.
- Close other programs that use the GPU.

### 2.3 Noise limit

Two runs of the same commit (`baseline`, `baseline2`):

- GPU columns above 0.1 ms: medians agree within 1.3 %. Use 2 % as the limit.
- GPU columns below 0.1 ms: the timer step is 0.001 ms. Use 0.003 ms as the limit.
- `cpu` column: medians agree within 3.2 %, but p95 is 2 to 3 times the median. Use it only to see if a scenario is limited by the CPU.

### 2.4 Scenarios

The window is 1280 x 720 in all scenarios, so `tonemap`, `ui_overlay`, and `present` write the same number of pixels.

| Scenario | Internal resolution | Scene |
|---|---|---|
| `test_360p` | 640 x 360 | Test scene of the game (cube, sphere, glass ball, two mirrors) |
| `test_720p` | 1280 x 720 | Same |
| `test_1080p` | 1920 x 1080 | Same |
| `test_2160p` | 3840 x 2160 | Same. Long frames: a small change shows above the timer noise |
| `stress_720p` | 1280 x 720 | 256 spheres, 565 248 triangles, 257 BLAS, 257 TLAS instances |
| `stress_merged_720p` | 1280 x 720 | The same triangles in one mesh: 2 BLAS, 2 instances |
| `heavy_1080p` | 1920 x 1080 | The size of a game level: 1000 spheres, 3 968 000 triangles, 1001 BLAS, 1001 instances |
| `heavy_merged_1080p` | 1920 x 1080 | The same triangles in one mesh |
| `heavy_mixed_1080p` | 1920 x 1080 | `heavy_1080p` with a mirror share and a glass share of 0.25: 245 mirrors, 277 glass spheres |
| `heavy_lights8_1080p` | 1920 x 1080 | `heavy_1080p` with eight point lights. Not active, see below |
| `heavy_mixed_lights8_1080p` | 1920 x 1080 | The two together. Not active, see below |

The two light scenarios are comment lines in `SCENARIOS`. The generator writes the lights into the file with a standard glTF field (`KHR_lights_punctual`). The engine does not read it yet: the shader gets one light from the config. An active scenario would thus show another load under a wrong name. D5 makes the two true. Until then, section 3.2 has numbers for eight lights from temporary shader edits.

The same seed gives the same spheres with and without the options, so the scenarios differ only in the materials and the lights.

To add a scenario, add one line to `SCENARIOS` in `tools/bench.py`. The third item of the tuple holds more config keys, for example `{"samples_per_frame": 4}`.

### 2.5 What the benchmark does not measure

- The light moves in each frame, so each frame is the first sample of a new mean. The `accumulate` pass never reads the old mean. T1 closes this gap.
- Picture quality. T2 closes this gap.
- Loads that the engine cannot show yet: many lights (D5), textures, objects that move.
- The number of rays. A change can make each ray faster or make fewer rays; the pass time does not tell which. T4 closes this gap.
- CPU time of each schedule. Use Tracy (`cargo run -p maki-game --release --features profile`) if section 13 stops being true.

### 2.6 Limits of the pass columns

- The column of the first pass (`build_tlas`) grows with the length of the frame: 0.113 ms in `heavy_1080p`, 0.273 ms for the same scene when the trace pass is longer. It holds a part of the wait for the frame before. Judge a change of the first pass by `gpu_total`.
- `accumulate` and `tonemap` are not stable from scene to scene at equal resolution: `accumulate` is 0.156 ms in `test_1080p` and 0.043 ms in `heavy_merged_1080p`. The cause was not found. Do not judge a change by these two columns alone.

## 3. Baseline

### 3.1 All scenarios

Runs `baseline2` (first six rows) and `heavy_baseline` (last two rows), medians in milliseconds, n = 600 frames for each scenario, one sample for each pixel and frame.

| Scenario | gpu_total | build_tlas | trace | accumulate | tonemap | ui_overlay | present | cpu |
|---|---|---|---|---|---|---|---|---|
| `test_360p` | 0.181 | 0.035 | 0.100 | 0.006 | 0.018 | 0.010 | 0.010 | 0.302 |
| `test_720p` | 0.463 | 0.039 | 0.359 | 0.023 | 0.018 | 0.012 | 0.011 | 0.433 |
| `test_1080p` | 1.259 | 0.039 | 0.842 | 0.156 | 0.071 | 0.013 | 0.011 | 1.285 |
| `test_2160p` | 4.266 | 0.039 | 3.510 | 0.563 | 0.110 | 0.012 | 0.011 | 4.235 |
| `stress_720p` | 0.649 | 0.080 | 0.486 | 0.039 | 0.018 | 0.012 | 0.011 | 0.602 |
| `stress_merged_720p` | 0.562 | 0.029 | 0.452 | 0.039 | 0.018 | 0.012 | 0.011 | 0.528 |
| `heavy_1080p` | 2.057 | 0.113 | 1.703 | 0.142 | 0.072 | 0.013 | 0.011 | 2.069 |
| `heavy_merged_1080p` | 1.599 | 0.029 | 1.479 | 0.043 | 0.022 | 0.012 | 0.011 | 1.610 |

Read-out:

- `trace` is 55 % to 92 % of the GPU frame.
- Cost of `trace` for each million pixels and sample: 0.41 ms in the test scene (the same at all four resolutions), 0.53 ms with 565 000 triangles, 0.82 ms with 4 million triangles.
- `build_tlas` is paid in each frame, also when no instance moved: 0.035 ms (6 instances) to 0.113 ms (1001 instances).
- `test_360p` is limited by the CPU (0.30 ms for each frame, 3300 frames in one second). All other scenarios are limited by the GPU.
- Mesh upload at start: 0.30 to 0.45 ms for each mesh. 1000 meshes take 451 ms.

### 3.2 A load of the size of a game level

At 1920 x 1080. The mixed row is the scenario `heavy_mixed_1080p` (run `after_T3`). The two light rows are temporary shader edits on `heavy_1080p` (runs `heavyexp_lights8_all`, `heavyexp_lights8_pick1`), because the engine has one light.

| Load | trace | gpu_total | Against `heavy_1080p` |
|---|---|---|---|
| 1000 diffuse spheres, 4 million triangles, one light | 1.703 | 2.057 | - |
| The same, 245 spheres are mirrors and 277 are glass | 3.248 | 3.760 | trace +91 % |
| Eight lights, one shadow ray to each at each diffuse hit | 4.844 | 5.587 | trace +184 % |
| Eight lights, one shadow ray to a random one | 1.769 | 2.128 | trace +4 % |

What this means for a game at 60 Hz (16.7 ms):

- Mixed materials at 1080p: 3.8 ms for one sample. Four samples fill the frame. At 3840 x 2160 one sample takes about 15 ms (estimate from the pixel count).
- Each 10 % of `trace` is then 0.3 ms for each sample. The changes of section 8 are worth their work at this load, not at the load of the test scene.
- One more shadow ray at each pixel costs 0.22 to 0.26 ms at 1080p. Light count must not set the ray count (D5).
- Textures, objects that move, and a denoiser are not in these numbers.

### 3.3 Where the time of the trace pass goes

`heavy_1080p`, parts of the path removed one at a time. The picture is wrong in these runs; they only measure.

| Part of the path | trace | Share |
|---|---|---|
| Full path | 1.703 | 100 % |
| Camera ray, first hit, first shadow ray (`heavyexp_no_bounce`) | 0.651 | 38 % |
| Bounce ray and second hit | 0.792 | 47 % |
| Shadow ray at the second hit (`heavyexp_no_second_shadow`) | 0.260 | 15 % |

Other measured parts:

- Normal fetch at a hit: up to 16 % of `trace` with diffuse spheres, up to 22 % with mixed materials (`heavyexp_flat_normal`, `heavyexp_mixed_flat_normal`). An upper limit: the run has no fetch at all, and the wrong normals change the directions of the bounce rays.
- The three stores to `depth`, `normal`, `motion`: 6 % to 9 % of `trace` in the stress and heavy scenes, nothing in the test scene (`exp_no_gbuffer_writes`, `heavyexp_no_gbuffer`, `heavyexp_mixed_no_gbuffer`).
- Long mirror and glass paths: a limit of 6 rays in place of 12 takes 13.5 % off a mixed load, a limit of 4 takes 27.7 %. These runs are from before T3: a temporary edit made the materials, with a trace time of 3.115 ms (`heavyexp_mixed_segments_6`, `heavyexp_mixed_segments_4`).
- One BLAS in place of 1000: `trace` -13.2 % (`heavy_merged_1080p`).

## 4. Main result

1. With the scene of today the GPU is idle. Default config (640 x 360, vsync, 60 Hz): 0.18 ms of each 16.7 ms. At 1080p: 1.26 ms. The picture needs 17 s for 1024 samples, and each move of the light starts it again with one sample for each pixel.
2. With a game-sized load the reserve is small. Mixed materials and 4 million triangles: 3.8 ms for one sample at 1080p. Eight lights with a shadow ray to each: 5.6 ms. A denoiser and textures come on top.
3. So the plan has two tracks, and both are necessary:
   - Use the frame: trace as many samples as the frame permits (P1), and do not throw samples away on a light change (P3). Then the denoiser and temporal reuse (D2, D3).
   - Lower the cost of a ray (section 8). R1, R2, R3, and P2 together are worth between 20 % and 35 % of the GPU frame at the heavy load. This is an estimate: the parts were measured one at a time, R1 is an upper limit, and the gains do not add up fully.
4. The mixed load costs 91 % more. The benchmark cannot tell yet how much of this is more rays and how much is slower rays (threads of one work group that go different ways). T4 counts the rays. The answer decides the largest item of the survey, Shader Execution Reordering (S1).

## 5. Plan, in order

Gains are for `gpu_trace` in `heavy_1080p` unless a column is named. "Mixed" is the load with mirrors and glass.

| Order | Id | Change | Gain | Evidence | Effort |
|---|---|---|---|---|---|
| Done | T3 | The loader reads glTF materials | The mixed scenario is true | Run `after_T3` | - |
| 1 | T1 | Benchmark with a light that does not move | Measures `accumulate` with history, and the idle frame | - | Small |
| 2 | T2 | Frame dump and error metric | Makes quality changes measurable; proves that speed changes keep the picture | - | Medium |
| 3 | T4 | Ray counter | Time for each ray: separates "fewer rays" from "faster rays" | - | Small |
| 4 | P1 | More than one sample in each frame | Time to N samples divided by the samples in a frame | Sure: cost is linear in samples | Small |
| 5 | P2 | No TLAS build when the instance list did not change | `gpu_total` -3 % to -19 %; -5.4 % at the heavy load | Measured: run `exp_tlas_skip` | Small |
| 6 | P3 | Light change keeps a short history | Noise during light motion: variance divided by (history cap / samples in a frame) | Estimate | Small |
| 7 | P5 | Low-discrepancy samples | Lower error at the same sample count, no ray cost | Literature (second survey) | Small |
| 8 | R1 | Compact normals, one fetch for each hit | Up to -16 %, mixed up to -22 % | Measured upper limit | Medium |
| 9 | R2 | Fewer stores to `depth`, `normal`, `motion` | -6 % to -9 % | Measured | Small |
| 10 | R3 | One BLAS for the static meshes of a model | -13 %, and the TLAS build -74 % | Measured | Medium |
| 11 | R4 | Path length limit as a config value | Mixed: -13.5 % at 6 rays, -27.7 % at 4 | Measured; changes the picture | Small |
| 12 | C4 | All BLAS builds of a load in one command buffer | Load time: 451 ms for 1000 meshes today | Measured | Small |
| 13 | P4 | `radiance` image as `Rgba16Float` | `gpu_total` 0 % to -3 % | Measured: run `exp_radiance_16f` | Small |
| 14 | D1 | Per-pixel data and history in the frame graph | Necessary for D2 to D5 | - | Large |
| 15 | D2 | Spatial denoiser | A clean picture from 1 to 16 samples | Literature | Large |
| 16 | R5 | Bounce ray in each second pixel | -21 % to -23 %; needs D2 | Measured | Small |
| 17 | D3 | Temporal reprojection | No restart for camera and object motion | Literature | Large |
| 18 | D5 | Many lights: one shadow ray, resampled pick | Eight lights: +4 % in place of +184 % | Measured | Medium |
| 19 | D4 | Bounce light: hash radiance cache or ReSTIR GI | Cache: up to -15 % and light of many bounces | Measured upper limit; literature | Large |
| 20 | S1 | Shader Execution Reordering, as a test | Survey: 11 % to 24 % in one game, more in others | Survey; needs T4 and a decision of the user | Large |
| - | S2 | Opacity micromaps | Survey: one trace pass from 7.90 ms to 3.58 ms | Survey; needs alpha-tested geometry | Medium |
| - | C1 to C3, C5 | Section 10 | Memory, or only under a condition | Mixed | - |

## 6. Tool work that comes first

### T1. Benchmark with a light that does not move

Why: in benchmark mode `push.samples` is always 1, so `accumulate.comp:37` never loads the old mean. The normal case (read and write of a 32-bit float image) is not measured. The frame after the last sample (only `tonemap`, `ui_overlay`, `present`) is not measured.

Steps:

1. `game/src/main.rs`: add the option `--bench-static`. It is valid only with `--bench-frames`. Add it to `USAGE` and to the `parse_args` test.
2. `game/src/bench.rs`: `BenchModule` gets `static_light: bool`. When true, do not add `move_lights`.
3. `tools/bench.py`: add a fourth item to the scenario tuples, a list of more command line arguments. Add `static_1080p` (1920 x 1080, `["--bench-static"]`, default `max_samples`) and `idle_1080p` (the same with `{"max_samples": 8}`: frames after the warmup are idle frames).
4. `README.md`: add the option to the Benchmark section.

Check: `static_1080p/gpu_accumulate` is above the `test_1080p` value. `idle_1080p/gpu_trace`, `gpu_accumulate`, and `gpu_build_tlas` are below 0.003 ms.

### T2. Frame dump and error metric

Why: P1, P3, and all D items trade bias and noise against time. Frame time alone cannot rank them. A speed-only change needs a proof that the picture is the same. The random numbers depend only on the pixel and the frame number (`trace.comp:239`), so two runs of the same commit give the same picture.

Steps:

1. gpu crate: `Gpu::read_image(image: ImageHandle) -> Result<Vec<u8>>`. Copies the image into a CPU-readable buffer and waits for the GPU (the pattern of `run_now`, `accel.rs:360`). Call it outside of a frame. The image must be in `Access::TransferSrc` for the copy; the function records the barrier from and back to the state that the caller gives.
2. render crate: `FrameGraph::image(id: ResourceId) -> Option<(ImageHandle, (u32, u32), Access)>` gives the image, its size, and its state after the last frame.
3. `game/src/bench.rs`: option `--bench-dump <path>`. After the last frame, read `accumulated` and write the file: two little-endian `u32` (width, height), then the `f32` RGBA values.
4. `tools/bench.py`: new command `quality <scenario> --reference <dump>`. It runs the scenario with `--bench-static --bench-dump`, then appends one row with `unit` = `relmse` to the history. Relative mean squared error: the mean over all pixels and the channels R, G, B of `(x - r)² / (r² + 0.01)`.
5. `tools/bench.py`: new command `same <dump_a> <dump_b>`. Prints `ok`, or the number of pixels that differ and the largest difference. Exit code 1 if they differ.
6. Make the reference one time for each scenario: static light, `max_samples = 65536`, frames = 65536 (after P1: fewer frames). Keep it in `target/bench/reference/`. Do not commit it.

Rules for use:

- Speed-only change: `same` must print `ok` for `test_720p` with a static light and 64 frames.
- Quality change: report `relmse` and `gpu_total` together. Compare at equal GPU time: choose the frame counts so that `frames * gpu_total` agree within 5 %.

### T3. The loader reads glTF materials

Done. `material_kind` in `crates/assets/src/model.rs`:

- transmission factor above 0.5: glass, with the index of refraction of the file (1.5 if the file has none, or has a value below 1);
- else metallic factor 0.5 or more and roughness factor 0.1 or less: mirror;
- else diffuse. The glTF defaults (metallic 1, roughness 1) are diffuse by this rule.

Result (run `after_T3`): `heavy_mixed_1080p/gpu_trace` is 3.248 ms. The temporary edit of the audit had given 3.115 ms with 250 mirrors and 250 glass spheres. The scenarios without such materials did not change (+0.1 % to +0.2 %). Validation is clean with a generated file.

### T4. Ray counter

Why: section 4, point 4. Also R4, R5, D4, and D5 change the number of rays, and R1, R2, R3, S1 change the time of a ray. One number for each makes the results comparable.

Steps:

1. `trace.comp`: count the ray queries of the pixel in a local `uint` (one count in `closest_hit`, one in `occluded`). Store the count in the `motion` image, which no pass reads until D3. With D3, move the count to its own image behind a config key.
2. With T2: `--bench-dump` also writes the sum of the counts of the last frame. `tools/bench.py` appends a row `rays` (unit `count`) and a row `trace_ns_per_ray`.

Check: in `heavy_1080p` the count for each pixel is between 1 and 4. The sum agrees with section 3.3: a pixel whose camera ray and bounce ray hit diffuse surfaces has 4 rays.

Use: record `trace_ns_per_ray` for `heavy_1080p` and `heavy_mixed_1080p`. If the mixed value is more than 1.2 times the diffuse value, the threads lose time to divergence, and S1 has a case.

## 7. Samples for each frame

### P1. More than one sample in each frame

Where: `crates/render/shaders/trace.comp:232-347` (`main`), `crates/render/shaders/accumulate.comp:35-42`, `crates/render/src/accumulation.rs:48` (`start_frame`), `crates/render/src/passes.rs:200` and `:274`, `crates/render/src/lib.rs:89` (`ExtractedFrame`), `crates/core/src/config.rs`, `crates/core/src/app.rs` (config checks).

Why: section 4. One sample costs 0.84 ms at 1080p and 0.10 ms at 360p. A 60 Hz frame has 16.7 ms.

Step A, fixed count:

1. Config: `samples_per_frame: u32`, default 1. Check in `App::new`: 1 to 64. Add the key to the config table of `README.md`.
2. `ExtractedFrame`: add `samples_per_frame`.
3. `Accumulation::start_frame(max_samples, samples_per_frame)`: the frame adds `min(samples_per_frame, max_samples - samples)` samples. New getter `frame_samples()`. Extend the unit test: with 3 samples in a frame and a cap of 8, the counts are 3, 6, 8, then no sampling.
4. `Trace`: add `frame_samples` to the push constants (the block has 100 of 128 bytes; the new field makes 104). In `trace.comp`, put the path loop in a loop over the samples. Do not seed again in the loop: the random chain of the pixel continues, so one sample in a frame gives the same picture as before. Write the mean of the samples to `radiance`. Take `depth` and `normal` from the first sample.
5. `Accumulate`: add `frame_samples` to the push constants. `accumulate.comp`: `mean = old + (radiance - old) * float(frame_samples) / float(samples)`. The first frame of a mean has `samples == frame_samples` and replaces the old content.
6. `crates/ui/src/stats.rs`: show the samples of the frame.

Step B, automatic count (a separate stage):

1. Config: `trace_budget_ms: f32`, default 6.0. `samples_per_frame = 0` means automatic.
2. A system of the accumulation module reads `FrameGraph::timings()`: the time of the pass `trace` and the frame number. It keeps the sample count of the last 4 frames (index `frame % 4`) to get the time of one sample: `trace_ms / samples_of_that_frame`.
3. Next count: `clamp(floor(trace_budget_ms / ms_of_one_sample), 1, 64)`. Without timings (GPU without timestamps): 1.
4. Do not change the count by more than a factor of 2 from frame to frame.

Check:

- Step A with `samples_per_frame = 1`: all columns within the noise limit; `same` prints `ok` (after T2).
- Add scenarios `test_1080p_spp4` and `test_1080p_spp16`. Expect `gpu_trace` near 4 and 16 times the `test_1080p` value, and no change of the other columns.
- With T2: `relmse` of 64 frames at 4 samples agrees with 256 frames at 1 sample within 10 %.
- Step B: on a 60 Hz display with vsync, the stats panel shows 60 FPS and a GPU frame near the budget.

Risk: a frame longer than the display period halves the frame rate with vsync. The default budget of 6 ms keeps a margin.

### P2. No TLAS build when the instance list did not change

Where: `crates/render/src/passes.rs:144-176` (`BuildTlas::execute`), `crates/gpu/src/accel.rs:39` (`TlasInstance`).

Why: the TLAS is built in each sampling frame. The light is not in the TLAS, so a light change needs no build.

Measured (`exp_tlas_skip`, a temporary edit that stops the build after 5 frames): `gpu_total` -18.8 % (`test_360p`), -8.0 % (`test_720p`), -3.1 % (`test_1080p`), -12.5 % (`stress_720p`), -5.4 % (`heavy_1080p`). `gpu_trace` does not change.

Steps:

1. `TlasInstance`: derive `PartialEq`.
2. `BuildTlas`: add `built: Vec<TlasInstance>` and `valid: bool`. After the instance list is made, return if `valid && instances == self.built`. Else call `build_tlas`, then store the list and set `valid`.
3. Update the doc comment of `BuildTlas` (`passes.rs:121-128`) and the module comment of `accel.rs:9-10`.

No other module changes. A new BLAS has a new handle, so a mesh change makes the lists differ.

Check: `gpu_build_tlas` is below 0.003 ms in all scenarios, and `gpu_total` falls by the measured amounts. `gpu_trace` does not change. `same` prints `ok`. Validation is clean with `MAKI_RENDER_SELFTEST=1`.

Later step (with dynamic objects, Appendix A item 1): when only transforms change, record an update (`BuildAccelerationStructureModeKHR::UPDATE` with `ALLOW_UPDATE`) in place of a build. Measure build against update with a generated scene that moves 10 % of the instances.

### P3. Light change keeps a short history

Where: `crates/render/src/light.rs:58` (the reset on a light change), `crates/render/src/accumulation.rs`, `crates/render/src/lib.rs:82`.

Why: the camera is fixed and the geometry does not move when the light moves. The pixel of the old frame shows the same surface point. Only its lighting is old. A full restart drops samples that are almost correct. A mean with a short memory keeps them: the same idea as the history length limit of a temporal denoiser, without reprojection.

Steps (do P1 first):

1. `Accumulation::request_soft_reset()` sets a flag. In `start_frame`, when the flag is set and no full reset waits: `samples = min(samples, history_cap)`. The frame then adds its samples as usual. With the weight of P1 step 5 this is a moving mean.
2. `RenderWorld::soften_accumulation()` calls it. `extract_lights` uses it in place of `reset_accumulation`. All other callers keep the full reset (camera, instances, meshes, resolution, the R key).
3. Config: `light_history_samples: u32`, default 32. The value 0 gives the old behavior. Add the key to `README.md`.
4. Unit test: at 100 samples, a soft reset with a cap of 32 and 4 samples in a frame gives 36. A full reset in the same frame wins and gives 4.

Effect: while the light moves, a pixel holds about `history_cap` samples in place of `samples_per_frame`. The light answers with a delay of about `history_cap / samples_per_frame` frames (8 frames with 32 and 4). When the light stops, the old light has the weight `history_cap / samples` in the mean: 3 % at 1024 samples.

Check: with T2, light on the benchmark path: `relmse` against a reference of the same light position, with and without the history, at equal GPU time. Look at the picture: a shadow that moves has a soft trail of the stated length and no hard double edge. Tune the default with the user.

Note: after P3 the benchmark light causes soft resets, so `accumulate` reads the old mean in each frame. Record a new baseline after P3.

### P4. `radiance` image as `Rgba16Float`

Where: `crates/render/src/passes.rs:33-37`, `crates/render/shaders/trace.comp:234` and `:343`, `crates/render/shaders/accumulate.comp:14-15` and `:30`.

Why: `radiance` holds one frame. It needs no 32-bit precision. `accumulated` keeps `Rgba32Float` (see the comment at `crates/gpu/src/resources.rs:35`).

Measured (`exp_radiance_16f` against `baseline2`): `accumulate` -25 % at 1080p, -36 % in `stress_720p`, -9 % at 2160p. `gpu_total` -2.9 %, -3.1 %, +0.7 %. Image memory at 1080p: 33 MB to 17 MB.

Steps:

1. `passes.rs`: the format of `radiance` becomes `Format::Rgba16Float`.
2. `trace.comp`: store through `images_rgba16f`, and take the image size from that array. Clamp before the store: `min(radiance, vec3(60000.0))`. Without the clamp, a small light gives infinity: `Light::radiance()` with `MIN_RADIUS` is 3.8e6, and the largest 16-bit float is 65504.
3. `accumulate.comp`: add a second view of the binding, `rgba16f`, `readonly`, and load `radiance` from it.

Check: `gpu_accumulate` falls in `test_1080p` and `stress_720p`. The picture of the light sphere is the same after the tone curve (60000 is far above its white point). With T2: `relmse` against the reference does not rise.

Skip P4 if D2 is next: the denoiser changes these images anyway.

Not planned: accumulation inside the trace pass, with no `radiance` image. The upper limit is the `accumulate` column (12 % of `gpu_total` at `test_1080p`), but D2 needs the sample apart from the mean, and P1 makes the share small.

### P5. Low-discrepancy samples

Where: `crates/render/shaders/trace.comp:84-98` (`random`), `:239` (the seed), `:243` (position in the pixel), `:212-213` (direction to the light), `:339` (bounce direction).

Why: each `random()` value is an independent hash. The error of a mean of N independent samples falls with the square root of N. A low-discrepancy sequence covers the sample space evenly: the error at the same sample count is lower, and no ray is added. If the errors of neighbor pixels also differ in a regular way (as blue noise), a denoiser removes them more easily.

Source: the second survey names Z++ (Ahmed, Pharr, Huang, SIGGRAPH Asia 2026) and SZ sequences and NILE (Ahmed, Pharr, Ostromoukhov, Huang, SIGGRAPH Asia 2025). It gives one sentence for Z++ and titles for the other two. Step A below is older, well-known practice and does not need them; step B needs the papers.

Step A, a shifted sequence for each pixel:

1. Three pairs of numbers matter most: the position in the pixel, the direction to the light at the first diffuse hit, and the bounce direction at the first diffuse hit. For each pair: `fract(shift + n * vec2(0.7548776662, 0.5698402910))`. `n` is the number of the sample since the last full reset. `shift` is a hash of the pixel and the pair number. This is the R2 sequence with one random shift for each pixel and pair.
2. `n` comes from the accumulation, not from the frame number: add the number of the first sample of the frame to the push constants. With P1, sample `s` of the frame has `n + s`. With P3, use a count that the soft reset does not lower.
3. All other random numbers (the second hit, the choice at glass) keep the hash.

Step B, a separate stage: order the pixels along a Z curve and give each pixel its own part of one Sobol sequence, so that the errors of neighbor pixels differ as blue noise. Read Z++ first: it adds the time axis, which this renderer needs for the mean over frames.

Check, with T2: `relmse` against the reference at 16, 64, and 256 samples, hash against step A, in `test_720p` and `heavy_mixed_1080p` with a static light. Keep step A if `relmse` at 256 samples falls by 20 % or more. `gpu_trace` must stay within the noise limit. Look at the picture at 4 samples: a regular pattern means that the shifts of neighbor pixels are not independent.

## 8. Cost of a ray

All numbers are `gpu_trace` in `heavy_1080p` against the run `heavy_baseline`, unless another scenario is named. Do T4 first, so that each item is also measured for each ray. Measure each item in `heavy_1080p` and in `heavy_mixed_1080p`.

### R1. Compact normals, one fetch for each hit

Where: `crates/render/shaders/scene.glsl:22-77`, `crates/render/shaders/trace.comp:143-150`, `crates/render/src/meshes.rs:35` and `:145-248`.

Why: a hit loads 3 indices, then 3 vertices of 32 bytes at 3 addresses, and uses 12 bytes of each. No shader reads the positions: the BLAS has its own copy.

Measured upper limit (no fetch at all): -16.2 %, -13.9 % (`heavy_merged_1080p`), -22.3 % (mixed). Loading the `MeshEntry` one time in place of two times gives nothing (run `heavyexp_entry_once`): do not do only that.

Steps:

1. A normal buffer for each mesh: three `uint` for each triangle, one for each corner. A normal is in octahedral form, two signed 16-bit values in one `uint`. The shader reads `corners[triangle * 3 + 0..2]`: 12 bytes in sequence at one address.
2. `MeshEntry` gets the address of this buffer in place of the vertex and index addresses. Keep `entry_bytes`, `scene.glsl`, and the layout test in step.
3. The vertex and index buffers are then read only by the BLAS build. Free them after the build. Memory for each triangle falls from about 28 bytes (indexed, shared corners) to 12.
4. Unit test with seeded random unit vectors: decode(encode(n)) differs from n by less than 0.0001 in each part.

Check: `gpu_trace` falls by more than the noise limit in `heavy_1080p`, `heavy_merged_1080p`, and the mixed scenario. The picture is not bit-equal (16-bit normals): with T2, `relmse` against the reference does not rise.

### R2. Fewer stores to `depth`, `normal`, `motion`

Where: `trace.comp:343-346`, `crates/render/src/passes.rs:54-70`.

Measured (no store to the three images): -7.9 %, -6.2 % (`heavy_merged_1080p`), -8.9 % (mixed), -7.0 % (`stress_720p`), no change in `test_1080p` and `test_2160p`. No pass reads these images today, but D1 needs `depth` and `normal`.

Steps, measure after each:

1. Remove the `motion` image and its store. It holds zeros. D3 adds it again with real values. (If T4 uses it for the ray count, do this step with D3.)
2. If step 1 gives less than half of the measured gain: put `normal` (octahedral, two channels) and `depth` in one `Rgba16Float` image. Check with D1 that 16 bits are sufficient for the depth test of the denoiser; if not, use `Rgba32Float`.

Note: with P1, the three images are written one time for each frame, not for each sample. With 4 samples in a frame the cost is a quarter. Do R2 after P1 and measure with the sample count of the target config.

### R3. One BLAS for the static meshes of a model

Where: `crates/assets/src/model.rs:87`, `crates/render/src/meshes.rs:145`, `crates/gpu/src/accel.rs:26` and `:92`.

Measured: `heavy_merged_1080p` against `heavy_1080p`: `gpu_trace` -13.2 %, `gpu_build_tlas` -74 %. `stress_merged_720p` against `stress_720p`: -7.0 % and -64 %. The spheres overlap much in the TLAS, which is a bad case. Expect less in a room with furniture. After P2 only the trace part is left.

Steps:

1. `BlasDesc` takes a list of geometries (vertex buffer, index buffer, counts) in place of one.
2. `GpuMeshes` builds one BLAS for all meshes of a `Model` that do not move, with one geometry for each mesh. The meshes of one BLAS get mesh table indices in sequence.
3. Shader: mesh index = custom index of the instance + `rayQueryGetIntersectionGeometryIndexEXT`.
4. A mesh that will move keeps its own BLAS. The model asset must tell which meshes are static; the simplest rule for now: all meshes of a loaded model are static.

Check: `heavy_1080p` reaches the trace time of `heavy_merged_1080p` within 3 %. `same` prints `ok` against the run before.

### R4. Path length limit as a config value

Where: `trace.comp:71-73` (`MAX_SEGMENTS = 12`), `OPEN_ITEMS.md` "Path length limit".

Measured on the mixed load: 6 rays -13.5 %, 4 rays -27.7 %. A path that reaches the limit brings no light, so a lower limit makes deep reflections dark. Russian roulette does not help here: mirror and glass keep 90 % to 100 % of the light, so almost no path would end.

Steps:

1. Config `max_path_rays`, default 12 (no change of the picture). Pass it in the push constants. Check 2 to 32 in `App::new`.
2. At the limit, do not end in black: treat the last surface as diffuse and add its direct light. The dark holes become plausible light.
3. With T2 on the mixed scenario: `relmse` and `gpu_trace` for 4, 6, 8, 12. Propose the default to the user with the four numbers and four screenshots.

### R5. Bounce ray in each second pixel

Where: `trace.comp:329-340`.

Measured (bounce in a checkerboard that changes with the frame): -23.1 %, -20.7 % (`test_1080p`). The pixel without a bounce gets no bounce light in that frame, so the bounce light of the other frames counts double.

Do this only after D2: without a filter, half of the bounce samples for 22 % of the time is a bad trade. With a filter the bounce light is smooth, and half of the samples is sufficient. Judge with T2 at equal GPU time.

### S1. Shader Execution Reordering, as a test

Survey rank 1: 11 % to 24 % in Indiana Jones and the Great Circle, 20 % to 50 % in the path tracers of NVIDIA, about one third in Alan Wake 2 together with opacity micromaps. The RTX 50 series does the reorder in hardware.

Why it is not a simple change here: it sorts the threads by the hit they found, before the hit is shaded. It exists only in a ray tracing pipeline (hit objects). This tracer is a compute shader with ray queries, which `CLAUDE.md` sets as the stack. The test needs a second form of the trace pass, and the user must accept a change of the stack if the test wins.

Evidence in this code that the order of threads matters: the mixed load costs +91 %. A work group of 16 x 16 in place of 8 x 8 gives -11 % with diffuse spheres but +9 % with mixed materials and +11 % in the test scene (runs `heavyexp_groups_16`, `heavyexp_mixed_groups_16`). T4 gives the proof or the disproof: time for each ray, mixed against diffuse.

Steps, only if T4 shows more than 1.2 times the time for each ray:

1. gpu crate: a ray tracing pipeline (`VK_KHR_ray_tracing_pipeline`: one ray generation shader, one miss shader, one hit group, a shader binding table) and `VK_EXT_ray_tracing_invocation_reorder`. Check first that the `ash` version in use has the extension; the older `VK_NV_ray_tracing_invocation_reorder` is the fallback on NVIDIA.
2. `trace.rgen`: the code of `trace.comp`, with each ray query replaced by a hit object: trace, then reorder with a hint (material kind in the low bits, as the SER whitepaper of the survey describes), then read the hit and shade in the same shader. Keep `scene.glsl`.
3. A config key selects the pass. Both stay until the numbers are in.
4. Compare on `heavy_1080p`, `heavy_mixed_1080p`, and the test scene: `gpu_trace`, `trace_ns_per_ray`, and `same` for the picture.

Keep it if the mixed scenario gains 10 % or more and no scenario loses more than 3 %. To keep the port small, do not mix traversal and material code in `trace.comp` in the stages before.

A form without a new pipeline, to test first if step 1 is too large: trace the camera ray and the first shading in one dispatch, write the state of the bounce ray to an image, and trace the bounce in a second dispatch. The threads of the second dispatch all do the same step. This costs two image stores and loads for each pixel, so measure it.

### S2. Opacity micromaps

Survey rank 1: the trace pass of Indiana Jones went from 7.90 ms to 3.58 ms with them. They store which parts of a triangle are see-through, so a ray needs no shader code to test a cut-out texture.

Not possible today: all geometry is opaque (`accel.rs:130`), and the loader reads no textures. Condition: glTF textures with `alphaMode` `MASK` (leaves, fences, hair). Then:

1. Do not test alpha in the loop of the ray query for each candidate. That is the slow path that the micromaps replace.
2. Use `VK_EXT_opacity_micromap`: bake a micromap for each alpha-tested mesh at load (the OMM SDK of the survey, or a small baker of our own), attach it to the BLAS build. The extension works with ray queries.
3. Benchmark: `gen_scene.py` gets an option for alpha-tested quads with a generated noise texture.

## 9. Features from the survey

Each item is a stage or more. The text gives the design frame and the test, not each step.

### D1. Per-pixel data and history in the frame graph

This is Appendix A item 2. It is the base of D2 to D5. The minimum:

- `trace.comp` writes direct light and bounce light apart, both without the albedo of the first diffuse surface, and writes that albedo to its own image. A filter must not blur the surface color.
- `depth` and `normal` come from a ray through the pixel center, or the first sample of P1, and are written in each frame.
- The graph gets "the image of the frame before" as a resource: two images that swap, with the barrier state kept for each.
- Pass parameters that do not fit in 128 bytes of push constants go into a buffer with one copy for each frame slot (the pattern of the TLAS instance buffers, `accel.rs:60`).
- `Accumulation` gets a per-pixel sample count in an image (`R16Float` or the alpha channel of the history) in place of one number for the picture.

### D2. Spatial denoiser

Survey family 2. The survey ranks DLSS Ray Reconstruction first for quality. Do not use it here: it is a closed SDK for one GPU maker, and this project plans a software tracer and a Metal backend (`CLAUDE.md`). Write the filter as compute passes.

Design: the edge-stopping wavelet filter of SVGF (Schied et al., "Spatiotemporal Variance-Guided Filtering", HPG 2017). Three to five passes with a 5 x 5 kernel and a step of 1, 2, 4, 8, 16 pixels. The weights fall with the difference of depth, normal, and brightness. Filter direct and bounce light apart, multiply by the albedo after the filter, then tonemap. Reference for the weights and the history rules: NVIDIA NRD (`github.com/NVIDIA-RTX/NRD`). Do not add it as a dependency, and read its license before you copy code. The same applies to the SHaRC and RTXDI sources named below.

Cost estimate, not measured: one pass loads about 75 texels for each pixel. `accumulate` loads 1 and costs 0.16 ms at 1080p. Plan for 2 to 5 ms at 1080p for all passes and measure. The budget exists (section 4).

Notes from the second survey: a network can denoise and upscale in one step from the noisy color only (Kazmierczyk et al., "Joint Denoising and Upscaling via Multi-branch and Multi-scale Feature Network", I3D 2025). That needs a trained network and an inference runtime, so it is not a path for this stage. Two things follow for the design: keep the filter at the internal resolution and let the planned upscaler come after it as its own pass, and keep the filter passes behind one seam so that a network can replace them later.

Test: `relmse` at equal GPU time, filter on and off, at 1, 4, and 16 samples. Each filter pass is a frame graph pass, so `bench.csv` shows its time with no tool change.

### D3. Temporal reprojection

Survey family 2. Replaces the full restart for camera and object motion, and replaces P3 for the light.

- `motion` gets real values: the screen position of the hit point in the frame before, from the camera of that frame (Appendix A item 2: `ExtractedCamera` needs the old matrices) and, for a moving instance, its old transform (Appendix A item 1: `ExtractedInstance` needs an id and the old transform).
- An accumulate pass with reprojection: read the history at `pixel - motion`, reject it when depth or normal differ, limit the history length.
- Test scene: `tools/gen_scene.py` plus a benchmark flag that moves the camera on a fixed path.

### D4. Bounce light

After a diffuse hit the path makes one bounce and ends at the second diffuse surface (`trace.comp:326-341`). Two techniques of the survey fit this path shape. Choose one. Decide with T2 numbers after D2.

Option 1, hash radiance cache (survey family 4, SHaRC, `github.com/NVIDIA-RTX/SHARC`):

- A hash grid in world space stores the mean light that leaves each cell. The bounce ray ends in the cache: the second diffuse hit reads the cell in place of one shadow ray.
- Gain: light of many bounces for the cost of one, and bounce light that is a mean over frames. Measured upper limit of the time gain: the shadow ray at the second hit is 15.3 % of `gpu_trace` (`heavyexp_no_second_shadow`); the cache lookup and the update pass take a part of it back.
- Fit: shader code only, no need for one GPU maker, works with ray queries. The source is HLSL: port the hash grid to GLSL, or use a GLSL header if the repository has one. The scene is a room with a fixed camera above it: few cells are in use.
- Cost: an update pass that traces a part of the pixels (about 4 % in the SHaRC guide; check the current guide) and a buffer of some tens of megabytes.
- Problem to solve: the light moves. The cache must forget old light fast (a short history for each cell) or the room keeps the old light for many frames.
- Do not use the neural cache (NRC): the survey marks it experimental and bound to one GPU maker.

Option 2, ReSTIR GI (survey family 1; Ouyang et al., "ReSTIR GI: Path Resampling for Real-Time Path Tracing", HPG 2021; course notes: Wyman et al., "A Gentle Introduction to ReSTIR", SIGGRAPH 2023):

- Each pixel keeps a reservoir: the second hit point, its light, and a weight. The pass shares reservoirs over frames and between neighbor pixels, then traces one shadow ray to the chosen point.
- Gain: the paper reports 9 to 166 times lower error at one sample for each pixel.
- Cost: reservoir images (about 48 bytes for each pixel, two copies), one more ray for each pixel, and D1 and D3 complete.
- ReSTIR PT is not a target: the survey reports 13 FPS on an RTX 5090 in the Zorah demo.
- Design input from the second survey, if this option is chosen: "ReSTIR PT Enhanced" (Lin, Kettunen, Wyman, I3D 2026) reports 2 to 3 times the speed of ReSTIR PT. Its parts that apply to a small tracer: one set of reservoirs for direct and bounce light (then D5 step 3 and D4 share their images and passes), a neighbor relation that is used in the two directions (half of the cost of the reuse between pixels), and a map against repeated samples. For the choice of neighbors also "Compatibility-Guided Neighbor Selection for ReSTIR" (HPG 2026) and "Stochastic Pairwise MIS for Unbiased Large-Kernel Reuse in Real Time" (Eurographics 2026). The second survey had the abstract of the first paper and only the titles of the other two: read the papers before the design.
- A small network as the cache (neural radiance cache) is possible without special hardware: the second survey names an implementation on a mobile GPU (Allen et al., SIGGRAPH Asia 2025). It is still more work than the hash grid. Look at it only if the hash grid of option 1 is too coarse.

### D5. Many lights

Survey family 1, ReSTIR DI and RTXDI. With one light it gives nothing: `direct_light` (`trace.comp:196`) already samples the cone of the sphere, which is the best case.

Measured with eight lights in `heavy_1080p`: a shadow ray to each light costs +184 % (`heavyexp_lights8_all`). One shadow ray to a random light costs +3.9 % (`heavyexp_lights8_pick1`). So the ray count stays at one for each hit, and the work goes into the choice of the light. `OPEN_ITEMS.md` ("More than one light") plans a light buffer and a uniform random pick. Do this in its place:

1. Light buffer, one copy for each frame slot (see the `ponytail` comment at `passes.rs:217`).
2. Resampled pick: take M lights at random (M = 8), give each the weight `brightness / distance²`, pick one in proportion to the weight, trace one shadow ray. This is step 1 of ReSTIR DI and needs no history.
3. Only if a scene has some hundreds of lights: reservoirs with reuse over frames and neighbors (needs D1 and D3). Reference code: RTXDI (`github.com/NVIDIA-RTX/RTXDI`).

4. Lights from the scene file: the loader reads `KHR_lights_punctual` point lights (a feature of the `gltf` crate) and the scene module spawns a `Light` for each. `tools/gen_scene.py --lights N` writes them. Then make `heavy_lights8_1080p` and `heavy_mixed_lights8_1080p` active in `tools/bench.py`.

Test: T2 compares the uniform pick with the resampled pick at equal time in `heavy_lights8_1080p`. `gpu_trace` stays within 5 % of `heavy_1080p`.

## 10. Small or conditional changes

| Id | Change | Where | Evidence | Do it when |
|---|---|---|---|---|
| C1 | BLAS compaction | `crates/gpu/src/accel.rs:134` | Survey: 1027 MB to 606 MB in one game | GPU memory of the meshes shows. No frame time gain is expected |
| C2 | Mesh buffers in GPU-only memory | `crates/render/src/meshes.rs:194` | Not measured | Before a scene with more than 200 MB of mesh data. R1 step 3 removes most of these buffers |
| C3 | TLAS update in place of a build | `crates/gpu/src/accel.rs:79` | Not measured | Objects move (Appendix A item 1). See the last paragraph of P2 |
| C4 | All BLAS builds of a load in one command buffer | `crates/gpu/src/accel.rs:142` and `:360` | 451 ms for 1000 meshes, 0.45 ms for each | In the plan (order 12). Also `OPEN_ITEMS.md`, "BLAS build stops the game" |
| C5 | One random texel for each texture lookup | The stage that adds glTF textures | Second survey: Wronski, Pharr, Akenine-Möller, "Improved Stochastic Texture Filtering Through Sample Reuse", I3D 2025 | With the first texture (`OPEN_ITEMS.md`, "Textures from glTF") |

Notes:

- C1: add `ALLOW_COMPACTION`, read the compact size with a query, copy with the mode `COMPACT`, free the first structure. Log the size before and after.
- C2: `gpu-allocator` puts `CpuToGpu` buffers in memory that is CPU-visible. Where that memory is was not checked on this GPU. If it is system memory, or if the CPU-visible range on the card is full, each hit reads the normals over the bus. Check with the allocator report. The fix is the staging upload of `OPEN_ITEMS.md` ("Mesh buffers in GPU-only memory").
- C5: a hit in a compute shader has no automatic mip level, and a filtered lookup reads 8 texels. Take one texel, chosen at random with the filter weights; the mean over samples (and the denoiser) does the filtering. Choose the mip level from the ray: its length and the angle at the hit (ray cones, Akenine-Möller et al., 2021). The I3D 2025 paper is about textures that are larger on the screen than their texels; read it if such textures show noise.
- C4: `create_blas` makes a command pool, submits, and waits for an idle queue for each mesh. Collect the builds of one `GpuMeshes::sync` call, record them in one command buffer with one scratch buffer for each, wait one time. Check: the time from the first to the last `mesh uploaded` log line in `heavy_1080p` falls below 150 ms.

## 11. Measured and rejected

Runs in `tools/bench_history.csv`. Do not try these again without a new reason. Add a row for each item of this plan that fails its check.

| Change | Runs | Result |
|---|---|---|
| `glslc -O` in `crates/render/build.rs` | `exp_glslc_O` | `gpu_trace` +0.6 %, -0.2 %, 0.0 % (1080p, 2160p, stress). The driver compiles the SPIR-V again |
| TLAS with `PREFER_FAST_TRACE` (`accel.rs:85`) | `exp_tlas_fast_trace`, `heavyexp_tlas_fast_trace` | `gpu_trace` +0.3 % to +0.9 % at 6, 257, and 1001 instances |
| One load of the `MeshEntry` for each hit | `heavyexp_entry_once` | `gpu_trace` +0.2 %. The compiler does it already |
| Work group of 16 x 16 for the trace pass | `heavyexp_groups_16`, `heavyexp_mixed_groups_16` | -11.2 % with diffuse spheres, +9.0 % with mixed materials, +11.3 % in the test scene. No fixed size wins. Evidence for S1 |
| Work group sizes, again after E1 | `G16x16`, `G8x4`, `G4x4`, `G16x8` | `gpu_trace`, diffuse and mixed: +6.5 % and +10.4 % (16 x 16), +2.1 % and +0.5 % (8 x 4), +18.8 % and +29.2 % (4 x 4), -0.2 % and +3.1 % (16 x 8). 8 x 8 stays |
| `motion` image removed | `R2a` | No change. The image stays removed: it held zeros |
| Path regeneration: the samples of a pixel in the same loop as the rays, so a pixel whose path has ended starts its next path at once | `E5b`, `E5c`, `E5cx` against `E1`, `E1x` | `gpu_trace` +6.2 % (mixed) and +13.1 % (diffuse) at 1 sample. At 4 samples -2.5 % (mixed) and +0.6 % (test scene). Try again only if the game runs at 8 samples or more |
| Less state in the path loop: the light of a shadow ray as one number, the path adds to the sum of the pixel | `X22ab` | `gpu_trace` +1.6 % (mixed), +7.0 % (diffuse). With `t_min` and the ray flags computed from the kind of ray: +2.0 %, +7.3 % (`X22`). Small changes of the loop move the time by some percent in the two directions: measure each one |
| Normal to world space without the world-to-object matrix (upper limit) | `X18` | No change |
| P5 step A: R2 sequence with a shift for each pixel, for the pixel position and the two directions at the first diffuse surface | `hash16` to `hash256`, `r2_16` to `r2_256` | `relmse` at 256 samples: -5.9 % (`test_720p`), -0.1 % (`heavy_mixed_1080p`). The limit was -20 %. The error comes from paths that these three pairs do not control (glass, the second surface) |
| R5: bounce ray in each second pixel, with the denoiser | `r5_16`, `r5_64` against `r5base_16`, `r5base_64` | `gpu_total` -8 % to -12 %. `relmse` +61 % to +66 % in `heavy_mixed_1080p`; `test_720p` -20 % at 16 samples and +28 % at 64. The error falls faster with more samples than with this |
| No light sphere test for a ray that cannot show a light | `raw2` against `raw1` | `gpu_trace` +1.4 % (test scene), +6.0 % (mixed) |
| No light pick loop when the scene has one light | `raw3` against `raw1` | `gpu_trace` -0.4 %, -0.8 %: below the noise limit |
| `denoise_filter`: no read of the light of a tap whose normal weight is zero | `dnB` against `dnA` | The four filter passes +20 % (0.786 to 0.944 ms). A branch in the tap loop costs more than the read |
| `denoise_filter` with a 3x3 pattern and 4, 5, or 6 passes | `k3p4_*`, `k3p5_*`, `k3p6_*` | 6 passes: `gpu_total` -6 % to -12 %, `relmse` +66 % at 4 samples and +14 % at 64 (`test_720p`), +17 % and +12 % (`heavy_mixed_1080p`) |
| Normal buffers in GPU-only memory (C2) | `C2` | `gpu_trace` +0.4 %, +0.5 %. On this GPU, CPU-writable memory is as fast for the shaders. C2 stays conditional |

## 12. What the plan takes from the two surveys

### 12.1 The first survey: techniques in games, 2022 to 2026

| Survey family, its rank | Taken | Where in the plan | Not taken, and why |
|---|---|---|---|
| 3. Hardware and traversal, rank 1 | BLAS compaction. Shader Execution Reordering as a measured test. Opacity micromaps as the rule for alpha-tested geometry | C1, S1, S2 | Micro-meshes and swept spheres: no such geometry. Reordering and micromaps cannot go in today: they need a ray tracing pipeline and cut-out textures |
| 2. Denoising and temporal reprojection, rank 2 | The pipeline shape: split the light from the surface color, filter, reuse the frames before | D1, D2, D3, P3 | DLSS Ray Reconstruction itself: closed SDK of one GPU maker, against the planned software and Metal backends. Its measured gain over a hand-made denoiser is 3 % to 7 % of frame rate; the gain here comes from having a denoiser at all |
| 1. Reservoir sampling, rank 3 | Resampled light pick (the first step of ReSTIR DI). ReSTIR GI as one of two options for the bounce light | D5, D4 | ReSTIR PT: 13 FPS on an RTX 5090 in the demo that the survey names. Full ReSTIR DI: only for hundreds of lights |
| 4. Neural and caches, rank 4 | The spatial hash radiance cache (SHaRC) | D4 | Neural radiance cache: experimental, one GPU maker |

Where the survey does not fit this project:

- Its ranking is for games with a ray tracing pipeline, many materials, and foliage. The numbers of rank 1 (SER, micromaps) come from that case. This tracer has neither yet, so the order of this plan differs from the order of the survey.
- It has no numbers for ray queries in compute shaders, and none for the cost of a sample, a shadow ray, or a hit fetch. Sections 3.2 and 3.3 measure these.
- It names no open denoiser design. D2 uses SVGF (Schied et al., HPG 2017), and D4 the ReSTIR GI paper (Ouyang et al., HPG 2021); these two are not from the survey.
- It gives no path to use an idle GPU. P1 and P3 are the largest gains for the scene of today and are not survey items.

### 12.2 The second survey: research papers, 2025 and 2026

The second survey is a list of papers, most of them about reservoir reuse (ReSTIR). It says itself that it found no paper on hardware traversal or reordering, and that it had no abstract for many entries. It adds one item to the plan (P5), one rule for a later stage (C5), and design input for D2 and D4. It changes no priority: its papers improve techniques that are at the end of this plan.

| Paper of the second survey | Use here |
|---|---|
| Z++ (2026), SZ sequences and NILE (2025): sample sequences | New item P5 |
| Improved Stochastic Texture Filtering Through Sample Reuse (I3D 2025) | New rule C5, for the texture stage |
| ReSTIR PT Enhanced (I3D 2026), Compatibility-Guided Neighbor Selection (HPG 2026), Stochastic Pairwise MIS (Eurographics 2026) | Design input for D4 option 2 and D5 step 3 |
| Joint Denoising and Upscaling network (I3D 2025) | Note in D2: where the upscaler goes, and one seam for the filter |
| Neural Radiance Cache Implementation on Mobile GPU (SIGGRAPH Asia 2025) | Note in D4: a network cache needs no special hardware |
| Reservoir Splatting (2025), Multi-Layer Reservoir Splatting (2026) | For D3, only if D4 uses reservoirs |
| ReSTIR BDPT with caustics (Eurographics 2025) | The caustic speckle of `OPEN_ITEMS.md` ("Light by way of a mirror or through glass"). Too large for now; R4 step 2 and a clamp of the bounce light are the cheap answers |
| Gradient-Domain ReSTIR PT, ReSTIR PG (path guiding) | Not planned: they build on a full ReSTIR PT |
| Real-Time Level-of-Detail Rendering with ReSTIR, Ghost ReSTIR (volumes), ReSTIR-Sampled Shadow Maps | No level of detail, no fog, no shadow maps in this renderer. Look at Ghost ReSTIR if the game gets fog |
| GSCache, Radiance Caching for Differentiable Path Tracing, Taming Optimization Variance in Compact Neural Shading Networks | Other problems: volume data, inverse rendering, training of small networks |


## 13. Not a bottleneck

Looked at, no action:

- CPU frame: 0.30 ms with the UI on, which is 3300 frames in one second. The per-frame `Vec` allocations (`push_constants` at `passes.rs:109`, `plan_barriers` at `graph.rs:383`, the instance lists) are below the measurement limit.
- Three places do work for each instance in each frame: `extract_instances` (`game/src/scene.rs:127`), `BuildTlas::execute` (`passes.rs:150`), and `log_scene` (`meshes.rs:307`, two hash lookups for each instance). With 257 instances no cost shows. Measure again with Tracy near `MAX_INSTANCES` (4096). Appendix A item 1 changes this code anyway.
- `tonemap`, `ui_overlay`, `present`: 0.04 ms together at a 1280 x 720 window. The blit after the tonemap pass costs 0.011 ms.
- The PCG hash, the analytic light sphere test, the ray flags: no known gain.
- Russian roulette: a diffuse path already ends after one bounce. Only mirror and glass chains are long, and their albedo is near 1, so few paths would end early. R4 is the answer for long paths.

## 14. Appendix A: places that need rework

The content of this file before the audit, not changed: the foundation review of stage 9. Each place works for the MVP. Each one blocks a planned feature. Nothing here is fixed yet. D1 and D3 refer to these items.

Stage 10 (`docs/part2/stage-10-review-fixes.md`) fixed these lines: item 1, the first two (the component and the extract system are in the render crate; `ExtractedInstance` has `id` and `previous_transform`); item 2, the third and the fifth (`FrameGraph::add_history`; `ExtractedCamera::previous`).

### 1. Instance extraction (blocks dynamic objects)

Where: `game/src/scene.rs` (`MeshInstance`, `extract_instances`), `crates/render/src/meshes.rs` (`ExtractedInstance`, `ExtractedInstances`).

- The component and the extract system are in the game crate. The system clears `ExtractedInstances` and writes the full list. A second module cannot add its own instances.
- `ExtractedInstance` has a mesh and a transform only. It has no stable id and no transform of the frame before, so the renderer cannot compute motion.
- Each moved instance resets the accumulation. An object that moves in each frame keeps the picture at one sample for each pixel.
- The TLAS is built in full in each frame, and a mesh upload builds its BLAS with a wait for the GPU (`Gpu::create_blas`). The cost grows with the number of objects and uploads.

### 2. Per-frame data and history (blocks a denoiser)

Where: `crates/render/src/passes.rs` (`Trace::execute`, the `depth`, `normal`, and `motion` images), `crates/render/src/graph.rs`, `crates/render/src/accumulation.rs`, `crates/render/src/camera.rs` (`ExtractedCamera`).

- A pass gets its parameters only in push constants. `Trace` uses 100 of the 128 bytes. The camera matrices of the frame before do not fit.
- The frame graph knows images only. A buffer that the CPU writes in each frame has no place in it, and no barrier plan.
- The graph has no resource for "this image in the frame before". A denoiser needs the history of color, depth, and normal.
- `motion` is all zeros. `depth` and `normal` change with the random position of the camera ray in the pixel, and they are not written when the accumulation is complete.
- `ExtractedCamera` holds the camera of the current frame only.
- `Accumulation` is one flag for the full picture: reset all, or keep all. A denoiser needs a decision for each pixel.
- The trace shader writes one radiance value. Direct light, bounce light, and surface color are not apart.

### 3. The concrete `Gpu` type (blocks a software tracer and a Metal backend)

Where: `crates/render/src/graph.rs` (`Pass`, `PassContext::gpu`), `crates/render/src/lib.rs` (`RenderGpu`), `crates/render/src/meshes.rs` (`GpuMeshes`), `crates/gpu/src/lib.rs` (`create_compute_pipeline`), `crates/ui/src/overlay.rs`.

- `Pass`, `PassContext`, `GpuMeshes`, and `RenderGpu` name the struct `maki_gpu::Gpu`. No trait is between the renderer and the backend.
- A pass is a SPIR-V compute dispatch with bindless indices in push constants. A CPU tracer cannot run SPIR-V, so it cannot be a second implementation of the same calls.
- The seam is too low. A tracer backend needs the level "scene in, radiance image out". The graph, the tonemap pass, and the presentation can then stay as they are.
- The shaders are GLSL only. A Metal backend needs a second shader source or a translation step in `build.rs`.
- The UI pass uses the raster calls and the bindless set of the gpu crate directly. Each new backend must supply the same calls, or the UI needs its own seam.
