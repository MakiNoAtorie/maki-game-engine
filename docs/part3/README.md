# Part 3: picture quality

Goal: a sharp, stable, good-looking picture at 60 fps, at one sample for each pixel and frame. The denoiser must not blur. Edges must be smooth (anti-aliasing).

Sources: the pasted plan "Path Tracer: Audit and Implementation Plan" and the two surveys of October 2026 (denoising, real-time path tracing). This file adapts them to the code as it is. Work one stage at a time (working agreement in `CLAUDE.md`).

## 1. Audit

State of the work tree on 2026-10-11 (branch `part2`, on top of `fb55fc5`). Items 1 to 8 are those of the pasted plan.

| # | Item | Found | Verdict |
|---|---|---|---|
| 1 | Accumulation | `denoise_temporal` keeps a mean of up to `max_samples` (1024) for each pixel, reprojected with `motion`. `samples_per_frame = 0` fills `trace_budget_ms` (6 ms): about 64 samples in a frame at 720p on the RTX 5060 Ti. | The quality in motion comes from brute force. All tuning must be done at 1 sample; more samples are a bonus on fast GPUs. |
| 2 | Fireflies | A bounce ray that finds a light by way of a mirror or through glass adds the full light (`sees_light` is true again after a mirror or glass). No clamp but the 16-bit limit (60000). The filter spreads each one to a soft dot. | Defect. Limit the bounce light before the denoiser. |
| 3 | Light sampling | Direct light is sampled at each diffuse surface: one shadow ray, light picked from 8 candidates by importance, a point in the cone of the sphere. A bounce ray cannot count the light a second time. | Good. MIS is not necessary while lights are spheres and surfaces are diffuse. |
| 4 | Output | Exposure (manual), ACES in the short form of Narkowicz, correct sRGB encode. 8-bit output with no dither. | Correct. Not pretty: the curve shifts hues, dark gradients show bands. |
| 5 | Denoiser | SVGF form: temporal mean, variance, 4 a-trous passes, albedo taken out and put back. Guides: normal, distance, albedo, motion. No roughness (no material has one). Defects: section 2. | The main work of part 3. |
| 6 | Path end | The path ends at the second diffuse surface (one bounce) or after `max_path_rays` (8). No Russian roulette. | Good for now. Roulette is not necessary with one bounce. |
| 7 | BSDF | Three kinds: diffuse, perfect mirror, clear glass. No GGX, no roughness, no emissive mesh, no texture. | Nothing to fix. A rough material is new work (stage 21). |
| 8 | Backends | One backend: ray queries in a compute shader. No software tracer. | Not in part 3. |

Not in the list of the pasted plan:

| Item | Found |
|---|---|
| Primary rays | Traced, not rasterized. The trace pass writes the guides. Keep: one code path, and the cost is small (0.18 ns for each ray). |
| Anti-aliasing | None at 1 sample: the first path of a pixel goes through the pixel center. With more samples, the others take random points, but the guides stay those of the center: an edge pixel holds the light of two surfaces divided by the color of one. |
| Upscale | `tonemap` scales with a bilinear sampler. No seam for an upscaler. |
| Random numbers | PCG hash, white noise. An R2 sequence was measured at 256 samples with no filter and rejected (`docs/perf-improve.md`, section 11). Not measured at 1 sample with the filter. |
| Quality metric | `relmse` against 65536 samples only. `relmse` does not punish blur: each denoiser decision of rounds 3 and stage 12 was taken with a metric that prefers a soft picture. |
| Lights | Sphere lights only. No cone light for a flashlight. |
| GPU time | `heavy_mixed_lights8_1080p`: `gpu_total` 4.1 ms at 1 sample. Denoiser 1.3 ms at 1080p. The budget has room. |

## 2. Why the picture is blurry

In order of effect, as read from the shaders. Stage 15 confirms each one with the debug views before it changes code.

1. **One signal.** Direct light, bounce light, reflections, and what shows through glass are one image. The brightness test of the filter has one variance for all of them: the noisy bounce light opens the filter, and the filter then blurs the sharp shadow edge of the direct light.
2. **Glass is one flat surface for the filter.** `normal` and `albedo` of a glass pixel are those of the glass (white). All that shows behind or in the glass is "light" on one surface with one normal: the a-trous passes blur it over up to 30 pixels. Stage 12 measured the same (the unexplained 12 % rise at rest).
3. **The 3x3 variance takes each light edge for noise.** `denoise_prepare` uses `max(over time, over space)`. The variance over space of a converged mean is not zero at a shadow edge, a contact shadow, or a light falloff. So the filter never turns off there, also at 1024 samples. SVGF uses the measure over space only while the history is shorter than 4 frames.
4. **A global history cut.** When a light or an object moves, each pixel of the picture keeps at most `light_history_samples` (32). The player moves most of the time. At 1 sample that is a 32-frame lag (shadow trail); at 64 samples it is no history at all, and the picture is a spatial blur of one frame. There is no test for each pixel.
5. **Fireflies** (audit item 2) raise the variance around them, and the filter widens.
6. **A fixed filter width.** Four 5x5 passes with steps 1, 2, 4, 8 at each pixel, also where the history is long. Only the variance narrows the filter, and items 1, 3, and 5 keep the variance high.
7. **Reflections lose their history in motion.** A mirror pixel has the guides of the reflected surface and the motion of the mirror. The temporal pass reads the history of another point (`OPEN_ITEMS.md`, "Reflections in motion").
8. **The distance test has no slope.** It permits a fixed relative difference for each pixel of offset. On the floor, seen at a flat angle from the top-down camera, it is too strict along the view and too loose across objects that stand on the floor.

## 3. What part 3 takes from the sources, and what it does not

| Technique of the pasted plan | Decision | Why |
|---|---|---|
| Output transform, clamp | Stage 15 (clamp), stage 19 (look) | Fast wins. |
| VNDF sampling, Russian roulette | With stage 21 only | No GGX material and one bounce: nothing to fix now. |
| 1 path for each pixel + reference mode | Exists | `samples_per_frame = 1`, and `max_samples` with `bench.py reference`. New: the bench gates are at 1 sample. |
| Rasterized G-buffer | No | Traced primary rays keep one code path for a later software tracer. Measure again if the trace pass becomes the limit. |
| NEE + MIS | Exists (NEE). MIS with stage 21 | See audit item 3. |
| ReSTIR DI | Not now | Rooms have fewer than 10 lights and the pick already looks at each one. What is left is penumbra noise, which the split denoiser of stage 16 handles. Trigger: some hundreds of lights or emissive props (`OPEN_ITEMS.md`, "Many lights"). |
| A-SVGF temporal gradients | Replaced by a fast-history clamp (stage 17) | Gradients need forward projection of samples with their seeds: large. A second, short mean and a clamp of the long mean to it (the method of NRD ReLAX and ReBLUR) is one more image and some lines, on any backend. |
| Explicit specular chains for glass | Stage 16 | Root cause 2. |
| Path regularisation | With stage 21 | Mirrors and glass are perfect: there is no roughness to raise. Until then the clamp of stage 15 does this job. |
| ReSTIR GI | Stage 22, with a gate | The largest noise source after stage 16 is the bounce light. About 1 ms. Gate: lower error than the same GPU time spent on more samples. |
| World-space radiance cache (hash grid) | Stage 23, with a gate | Light of many bounces in dark rooms, and shorter paths. Biased. Small bounded rooms make it cheap. |
| Upscaler interface | Stage 18 | The anti-aliasing pass is the seam. |
| Software tracer, Metal | Not in part 3 | The split signals and the guides are plain images: nothing here blocks them. |
| Neural denoisers, DLSS-RR, NRD, OIDN | No | Not portable to macOS, or a licence, or not real time. The pipeline must not depend on one. |
| Caustics (ReSTIR FG, BDPT, PG), ReSTIR PT | No | About 50 ms for each frame. |

From the denoising survey, section 5 (what decides the quality): history confidence (stage 17), sampling for the filter (stage 20), guides (stage 16), albedo out of the signal (exists), firefly limit (stage 15), one stage that does anti-aliasing and upscaling (stage 18).

## 4. Target pipeline

```
trace ─┬─ direct    (light / albedo of the first diffuse surface)
       ├─ indirect  (the same, bounce light)
       ├─ specular  (reflection part of primary glass; later: glossy)
       └─ guides    normal + distance, albedo, motion (of the virtual point behind a mirror)
denoise_temporal   one pass, the three signals; fast and slow history; clamp
denoise_prepare    variance for direct and indirect
denoise_filter_N   direct: narrow. indirect: wide. specular: none.
denoise_resolve    albedo * (direct + indirect) + specular
taa                jitter resolve at the output resolution  <- upscaler seam
post               bloom, exposure, tone curve, grade, vignette, dither, sRGB
```

Each stage is a module with an image contract. A debug view shows the input and output of each one. `denoise_passes = 0`, and a new switch for each new pass, compare with and without.

## 5. Stages

Each stage leaves the game in a state that runs, with clean validation. Gates use `walk_720p` and `test_720p` at `samples_per_frame = 1` unless the stage tells otherwise.

| Stage | Name | Needs |
|---|---|---|
| 15 | Metric, and denoiser fixes with one signal | - |
| 16 | Split signals; mirror and glass | 15 |
| 17 | History confidence for each pixel | 16 |
| 18 | Anti-aliasing (TAA) and the upscaler seam | 17 |
| 19 | The look: tone curve, bloom, dither | 18 |
| 20 | Samples for the filter (blue noise) | 17. Gate |
| 21 | Rough materials (GGX), emissive meshes, cone light | 16, 18 |
| 22 | ReSTIR GI | 17. Gate |
| 23 | World-space radiance cache | 22. Gate |

Stages 15 to 19 are the answer to "blurry" and "pretty". Stages 20 to 23 are each decided by a measurement; skip one that fails its gate and add a row to `docs/perf-improve.md`, section 11.

### Stage 15: metric, and denoiser fixes with one signal

1. Metric that sees blur. `bench.py quality` also prints a sharpness number: the gradient energy of the picture over that of the reference (1.0: as sharp; below: blurred; above: noise). Add a side-by-side dump (picture, reference, difference) as one PNG. Record the baseline at 1 sample, at rest and for `walk_720p`.
2. Confirm section 2 with the debug views. New views: the filter input, the variance that the filter gets (not the moments), the filter output before albedo.
3. Firefly limit: the light of a bounce ray is limited before it is added (config `indirect_clamp`, in units of light after albedo is taken out). Never the direct light of the primary surface, never a light that the camera sees.
4. Variance: over space only while the history is shorter than 4 frames; else over time. Stage 12 measured "over time alone" as worse by `relmse` at rest because of rare bright samples: step 3 removes those. Measure again with the two numbers.
5. Distance test with the slope of the surface: the difference that counts is the distance to the plane of the center pixel (normal and position), not the difference of the two camera distances.
6. Filter width from the history length: fewer passes, or a smaller step, where the mean has many frames.

Gate: sharpness at rest at 1024 samples is 0.98 or more. `relmse` of `walk_720p` does not rise. `relmse` at 1024 samples at rest is at or below 0.000924 (the value before stage 12).

### Stage 16: split signals; mirror and glass

1. The trace pass writes `direct` and `indirect` in place of `radiance`, each as light with the albedo taken out. No more rays: the path already knows which one it adds to.
2. The temporal pass keeps a mean and moments for each. The filter runs on each with its own variance: few passes for direct (shadows stay sharp), the full count for indirect. Start with two runs of the same shader; put them in one shader with two targets only if the time shows a need.
3. Mirror: the guides stay those of the surface that shows. The distance becomes the full path length, and `motion` is that of the virtual point (camera origin + first direction * path length) with the camera of the frame before. Correct for a flat mirror and a scene at rest; else the history test rejects it.
4. Glass as the primary surface: the path splits one time. The refracted path gives the guides and adds to `direct` and `indirect` as a mirror path does, with the tint in the albedo. The reflected path adds to `specular`, times the reflectance. `specular` gets the mean over time only, no filter. Glass deeper in a path stays a random choice.
5. Close "Denoiser, one signal" and "Reflections in motion" in `OPEN_ITEMS.md` if their checks pass.

Gate: a shadow edge at rest is as sharp as the reference after 64 frames (sharpness of a crop). What shows behind the glass ball is sharp at rest. `walk_720p`: `relmse` and sharpness better than stage 15. Cost: report it; expected below +1 ms at 1080p.

### Stage 17: history confidence for each pixel

1. A second mean for each signal with a short limit (about 4 frames): the fast history.
2. The long mean is clamped to the fast mean plus or minus some standard deviations of the fast history. Where the lighting changed, the long mean follows in a few frames; where it did not, nothing changes.
3. The history length of a pixel falls with the size of the clamp, so the filter widens there for some frames.
4. Remove `soften_accumulation`, `light_history_samples`, and the `Extract` hint if the clamp does their work. Keep `reset_accumulation` (manual, resolution).
5. A pixel with no valid history tap looks in the 3x3 block around the place before it starts again.
6. Disoccluded pixels: more filter passes while the history is shorter than 4 frames (the 3x3 variance of stage 15, step 4).

Gate: moving light at 1 sample: no trail longer than about 6 frames in a frame sequence dump, and `relmse` better than with `light_history_samples = 32`. At rest: no rise. Close "Shadow trail" and "Flicker behind a moving object".

### Stage 18: anti-aliasing and the upscaler seam

1. Jitter: each frame moves all camera rays by one sub-pixel offset (Halton 2,3, 8 or 16 points). All samples of a pixel in a frame use the same offset, so the guides agree with the light. `motion` has no jitter.
2. New pass `taa`, after `denoise_resolve`, at the output resolution: reproject the history of the picture with `motion`, clamp it to the color range of the 3x3 block of the new frame (variance clip in YCoCg), blend. Sharper history read (Catmull-Rom) if the bilinear read softens in motion.
3. `taa` does the scaling: with `render_scale < 1` it puts each jittered input pixel at its true place in the output grid. `tonemap` no longer scales.
4. The seam: `taa` is the one pass between `denoised` (internal size) and `resolved` (output size). Its inputs are `denoised`, `motion`, the distance, and the jitter of the frame: the inputs of FSR, XeSS, MetalFX, and DLSS. No trait and no vendor code now; a second pass with the same images is the later change.
5. The light sphere and the background have no surface and no history in the denoiser: `taa` gives them smooth edges.

Gate: edges at rest agree with the reference (sharpness near 1.0, no stairs in a crop). No ghost behind the player in `walk_720p`. `render_scale = 0.67` looks better than the bilinear scale before. Cost below 0.5 ms at 1080p. Close "Edges and the light at rest".

### Stage 19: the look

1. Dither before the 8-bit store (blue noise or a hash, one bit of amplitude). Dark gradients are most of a horror picture, and bands show there first.
2. Tone curve: two curves that keep hues in bright colored light, AgX (film look, soft colors) and Khronos PBR Neutral (colors as authored). Config value `tone_curve`, and a control in the debug panel that changes it live. The short ACES form is removed. Keep `exposure` manual: automatic exposure removes the darkness that the game wants. Later it can be a value of the room file.
3. Bloom: bright parts of the picture (lamps, the light sphere, highlights) spread a soft glow. A chain of half-size images down and up, before the tone curve. Config: strength, 0 is off.
4. Vignette and a small color grade (lift, gamma, gain) as config values. Film grain only if the user wants it.

Gate: the user judges screenshots of both rooms. Cost below 0.5 ms at 1080p.

### Stage 20: samples for the filter (gate)

Blue-noise random numbers over space and time (a small generated texture set, made by code) for the first two decisions of a path: the point on the light and the bounce direction. White noise for the rest. Section 11 of `docs/perf-improve.md` rejected an R2 sequence at 256 samples with no filter; the new reason is the picture at 1 sample after the filter.

Gate: `relmse` at 1 sample after the denoiser falls by 15 % or more at equal time, or the user sees calmer noise in motion. Else reject.

### Stage 21: rough materials, emissive meshes, cone light

1. `Material` gets a roughness and an emissive color; the glTF loader reads them. GGX reflection with sampling of the visible normals (Dupuy and Benyoub, 2023), MIS between the light sample and the BSDF sample.
2. The `specular` signal now also holds glossy reflection, with a roughness guide. Low roughness: mean over time only (as stage 16). High roughness: a narrow filter.
3. After the first diffuse or rough bounce, later surfaces count as rougher (path regularisation). Russian roulette if paths get longer than now.
4. A cone light (the flashlight): a sphere light with a direction and an angle.
5. Emissive meshes show their color and light the room by bounce rays only. Direct sampling of mesh lights is the trigger for ReSTIR DI.

Gate: a rough metal and a plastic surface look as in a reference of 65536 samples. No new fireflies at 1 sample.

### Stage 22: ReSTIR GI (gate)

One reservoir for each pixel holds the best bounce sample (the second surface point and its light). Reuse over time (with the motion of stage 16) and over 3 to 5 neighbors, with a visibility ray for the chosen one. Diffuse bounce light only. Follow Ouyang et al., 2021, and the course notes of Wyman et al., 2023.

Gate: at equal GPU time, lower `relmse` and no lower sharpness than more samples for each frame, in both rooms, at rest and in `walk_720p`. Report the cost.

### Stage 23: world-space radiance cache (gate)

A hash grid over the room (GI-1.0 or SHaRC form): each cell holds the mean light that leaves the surfaces in it. The bounce ray reads the cache at the second surface in place of direct light only: light of many bounces at the cost of one. A small part of the pixels (about 4 %) follows longer paths to update the cells. Cells are cleared at a room change.

Gate: dark corners agree with a reference that has many bounces (new reference with a long path limit). No light leak through walls. No lag longer than about half a second after a door opens. Cost: equal or lower `gpu_trace`.

## 6. Rules for all stages

- Tune at `samples_per_frame = 1`. Then check that the automatic sample count still gains.
- Each gate has two numbers, `relmse` and sharpness, and a look at the side-by-side picture. `relmse` alone prefers blur.
- Each new pass has a GPU time column in `bench.csv` (automatic) and a debug view.
- Frame budget on the RTX 5060 Ti at 1080p: 8 ms `gpu_total` at 1 sample, to leave room for a mid-range GPU. Now: 4.1 ms in the heavy scene.
- New config values have defaults that give the best picture; 0 or `false` turns the feature off for a comparison.
- A change that fails its gate goes to `docs/perf-improve.md`, section 11.

## 7. Decisions

1. Tone curve of stage 19: AgX and Khronos PBR Neutral, both, with a toggle (user, 2026-10-11).
2. Stage 21 comes before stages 22 and 23. It changes the `specular` signal and adds MIS to the path; reuse and the cache are then built one time, on the final form of the path. Stages 22 and 23 can also fail their gates; stage 21 cannot.

## References

- Schied et al., 2017, SVGF. Schied et al., 2018, A-SVGF.
- NVIDIA NRD, README (ReLAX and ReBLUR: fast history, history clamp, anti-firefly).
- Karis, 2014, "High Quality Temporal Supersampling" (TAA). Salvi, 2016 (variance clip).
- Ouyang et al., 2021, ReSTIR GI. Wyman et al., 2023, "A Gentle Introduction to ReSTIR".
- Boissé et al., 2023, GI-1.0. NVIDIA SHaRC.
- Dupuy and Benyoub, 2023, "Sampling Visible GGX Normals with Spherical Caps".
- Weier et al., 2021, "Optimised Path Space Regularisation".
- Wolfe et al., 2022, "Spatiotemporal Blue Noise Masks".
