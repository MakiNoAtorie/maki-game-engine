# Stage 19: the look

State: built. The gate is the owner's judgment of the screenshots. Plan: `README.md`, section 5.

## 1. What was built

- Dither in `tonemap`: triangle noise of one 8-bit step, new in each pixel and frame.
- Tone curves: AgX (the short form of Wrensch) and Khronos PBR Neutral. Config `tone_curve` (`"agx"`, the default, or `"neutral"`), radio buttons in the Debug panel ("Look"). The short ACES form is removed. `exposure` stays manual.
- Bloom (`crates/render/src/bloom.rs`, `shaders/bloom.comp`): 5 images, each half the size of the one before, down and up again (dual filtering), before the tone curve. The first step weights very bright pixels down, so one bright pixel does not make the glow flicker. Config `bloom` (0.04; 0: off, the passes do nothing).
- Vignette (`vignette`, 0.25) and grade (`grade_lift`, `grade_gamma`, `grade_gain`, neutral defaults). No film grain.
- `--bench-shot` and `tools/shot.py`: a PNG of the picture as the window shows it, with no panel.
- `SizeMode::OutputDiv(n)` in the frame graph, for the small bloom images.

## 2. Screenshots

`python tools/shot.py OUT.png --room room_a.ron --set tone_curve=agx` and the same for `room_b.ron --spawn west` and `tone_curve=neutral`: 400 frames at rest, 1 sample for each frame. The four pictures of this stage, at half size: `pictures/stage-19-look.png` (left AgX, right Neutral; top room A, bottom room B). The full pictures are in `target/shot/`, which is not in the repository.

What shows: AgX has less contrast and paler colors, and the lamp goes to white softly. Neutral keeps the wall and the object colors as in the scene and has more contrast; the glow of the lamp is stronger against it.

Cost, run `s19` against `s18`, 1280 x 720 output: the nine bloom passes 0.054 ms, `tonemap` +0.015 ms. About 0.16 ms at 1920 x 1080 (by the pixel count). Below 0.5 ms: met.

The picture before the tone curve did not change (`s19_f64` equals `s18_f64`).

## 3. Files

- `crates/render/shaders/tonemap.comp`: the steps of the look, `agx`, `neutral`, `stored` (dither).
- `crates/render/shaders/bloom.comp`, `crates/render/src/bloom.rs` (new).
- `crates/render/src/graph.rs`: `SizeMode::OutputDiv`.
- `crates/render/src/passes.rs`, `lib.rs`: the bloom passes in the graph, `Look` in `ExtractedFrame`.
- `crates/core/src/config.rs`, `app.rs`, `lib.rs`: `ToneCurve`, the six config keys.
- `game/src/debug.rs`: the "Look" section.
- `game/src/bench.rs`, `main.rs`, `crates/ui/src/lib.rs` (`Panels::close_all`), `tools/shot.py` (new).

## 4. Deviations

- None from the plan. `--bench-shot` and `tools/shot.py` are an addition: the gate needs screenshots, and the picture dump of the benchmark is the picture before the tone curve.

## 5. Decisions

- Default curve: AgX (the softer one; a horror picture is mostly dark, and AgX keeps more steps there). The owner chooses.
- Bloom with no brightness threshold: a part of the blur is blended into the picture (`mix`), so the total light stays the same. The room gets 4 % softer; the lamp, 150 times brighter than the room, gets the glow.
- The grade is after the tone curve, in linear display color; the vignette is before it, on the light.
- Dither with a hash, not a blue-noise texture: at one 8-bit step the pattern does not show.
- The debug views that show light (raw frame, direct, indirect) use the tone curve and the dither, and not bloom, vignette, or grade.

## 6. Uncertain

- The defaults for `bloom` and `vignette` are a guess for the owner to change (Debug panel, "Look").
- Dither on a dark gradient was not compared on a display by eye.
