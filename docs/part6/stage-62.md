# Stage 62: the core of animation: keys, tracks, the step mode

State: done; the picture gate is deferred by the owner (section 3). Stage file: [stage-62-animation.md](stage-62-animation.md). Tickets #89 to #93 (pull request #88). Plan: `README.md` 2.6, 2.15, 3.2 items 6, 9, 26.

Lanes: #89 (A), #90 (B), #91 (C) at the same time; then #92 (D); then #93 (E). Then one code review (standards and spec) and one lane of fixes.

## 1. What was built

- **The clip** (#89): `maki_assets::clip`: `Clip { mode, fps, length, tracks }`, the data asset `clip` (`<name>.clip.ron`, edited in the inspector from its field table); `Track { channel, mode, keys }` on the object of the animator; channels `Position`, `Rotation`, `Scale`, `Field { component, field }`; `KeyValue` (`Number`, `Vector`, `Switch`, `Text`). Pure sampling: `clip_time` (loop, hold at the end, the cut to the clip frame rate) and `Track::sample` (`Step` holds; `Linear` lerps, slerps the short way; a switch or a text steps).
- **The field write** (#90): `maki_scene::write_field` writes a top-level field through `Live`, only a different value, and names each error; `SetField` uses it.
- **A pose jump in the renderer** (#91): `maki_core::PoseJumps`; a counted jump of an `Interpolated` pose adds one when the pose changes; the extract asks `RenderWorld::jump_accumulation`: a soft reset, and in that frame the TAA also tests the surface of pixels at rest, so the old pose does not stay as a ghost.
- **The animator** (#92): the room component `animator` (`clip`, `speed`, `looping`, `playing`) and its run state `Playback` (required); `animate` in `FixedUpdate` with fixed-step time. Transform tracks are relative to the rest pose. A step write (a step key, a clip with `fps`, a cut, the loop of a step clip) is a counted jump; a linear one blends. Field tracks write through `write_field`, a warning once for an unknown component or field. A new clip by `SetField` is a cut; `ResetPlay` gives back the rest pose. A changed clip file plays at once.
- **Content and measure** (#93): the test room `room_anim.ron` (`game/anim.toml`): a cube that hops on twos, a lamp that pulses by a field track, a trigger area that cuts to a spin clip; the clips in `game/assets/rooms/data/clips/`. The scenarios `jump_720p` and `light_step_720p`, and `bench.py jump`. The input script `tools/editor_scripts/stage62.txt`.
- **Review fixes**: `snap` as before stage 62 and `jump(to, &mut PoseJumps)` for a counted jump (the uncounted callers are back on `snap`); `PoseJumps::add` only for a jump; the room check warns of an animator with a fixed collider; a changed clip file plays at once; the first step write of an object with no `Interpolated` counts its jump; one-line comments in `taa.comp`; `SetField` has no second check of `is_live`.

## 2. Files

- `crates/assets/src/clip.rs` (new), `lib.rs`: the clip and its sampling.
- `crates/scene/src/object.rs`, `lib.rs`: `write_field`, `FieldWrite`, `FieldError`.
- `crates/scene/src/animation.rs` (new): `AnimationModule`, `Animator`, `Playback`, `animate`.
- `crates/scene/src/room.rs`: the jumps of an editor patch; the room check of an animated prop with a fixed collider.
- `crates/core/src/interpolation.rs`, `app.rs`, `lib.rs`: `PoseJumps`, the counted jump; `App::build_schedules` (a test of a module list with no GPU).
- `crates/render/src/accumulation.rs`, `lib.rs`, `meshes.rs`, `taa.rs`, `shaders/taa.comp`: the jump frame.
- `crates/events/src/builtin.rs`, `lib.rs`: `SetField` on `write_field`; the order `run_events.after(animate)`.
- `crates/engine/src/lib.rs`: `AnimationModule` in the module list; the app test of the list.
- `tools/bench.py`: `jump_720p`, `light_step_720p`, `bench.py jump`.
- `game/assets/rooms/room_anim.ron`, `game/assets/rooms/data/clips/*.clip.ron` (new, with id files), `game/anim.toml`, `game/scripts/src/lib.rs` (the test of the test rooms), `tools/editor_scripts/stage62.txt`.
- `CODING_STANDARDS.md`: the two animated scenarios as an exception of the rule of the benchmark rooms.
- Tests: `crates/assets/src/clip.rs`, `crates/scene/src/object.rs`, `crates/core/src/interpolation.rs`, `crates/render/src/accumulation.rs`, `meshes.rs`, `crates/events/src/tests/l62d.rs`.

## 3. Results

| Check | Result |
|---|---|
| Gate in code: a step clip holds and jumps with no blend; a linear clip at 8 fps changes 8 times in a second; a field track changes the intensity of a light; a `SetField` of `animator.clip` cuts to the first key | `crates/events/src/tests/l62d.rs` |
| Gate of the picture (`bench.py reference jump_720p`, `reference light_step_720p`, `jump`) | **Deferred by the owner (2026-11-12)** to the quality session after the program is built. The jumping cube itself does not smear (the TAA drops it). Its old shadow and red bounce light fade over about 8 frames. Error (relmse) of the crop of the place it left, against a lamp that steps once after the same settled history: 4 frames after: 0.093 against 0.074; 8 frames: 0.037 against 0.044; 28 frames: 0.0024 against 0.0091. A crop away from both: 0.00017. The first form of the gate (a light that moves for 240 frames: 0.0097) was not like for like |
| Tests of "Done when" | Each line has a test in the files of section 2 |
| Tests and clippy | `tools/check.sh` on the merged tree: all pass |
| Validation run | The editor runs of ticket E: no validation error |
| Trip run (debug) | Rooms A, B, A as in stage 61; no error |
| Picture gate (`bench.py gate`) | `trip_720p`, `walk_720p`, `materials_720p`, `textured_walk_720p` equal; `test_1080p` noise (1,708 pixels at 0.001). One run of lane C gave `test_1080p` CHANGED; its run again: noise |
| Benchmark against `s61` | Run `s62`: GPU total -1.8 % to +2.1 % |
| Run by hand in the editor | `stage62.txt` in `room_anim.ron`: the cube hops in steps with sharp edges and no ghost (two shots one frame apart: the same pose; a new pose after 5 frames); the lamp pulses (floor brightness 53, 36, 62); the trigger area cuts to the spin clip |

## 4. Deviations from the stage file

- A track has no `target`: a clip animates the object of its animator only. A target by the name of another room object would break ADR 0001; stage 63 adds a `target` (a joint of a model) with a default.
- `animate` runs before `run_events` (`run_events.after(animate)`): after it, bevy refused the order against nine systems of other modules, and each needed an order line (rule 5). So a `SetField` of `animator.clip` cuts one fixed step later.
- The gate of the picture compares the jump with a lamp that steps once (`light_step_720p`), not with a light that moves; it fails at 4 frames and is deferred (section 3).
- The scene of `jump_720p` is written in `tools/bench.py`, not in `tools/gen_scene.py`.
- glTF animations and the `STEP` sampler are in stage 63: the loader reads no animation.

## 5. Decisions where the stage file was silent

- A pose change that a room load follows (spawn, transfer, a save load, `ResetPlay`, the light moves of the benchmark) is not a counted jump: a room load starts the picture again, and a light has no mesh.
- One step write in a clip snaps the whole pose: one `Interpolated` holds one pose, so a linear transform track beside a step one also snaps in that step.
- `KeyMode` default `Linear`; a step clip writes `mode: Step`. Keys in any order; before the first key, the first key holds; an empty track gives nothing.
- The clip time is a count of fixed steps times the step (an f32 sum drifts).
- Clip paths resolve from the folder of the room file.
- The jump frame of the TAA drops the anti-aliasing history of edges at rest for one frame.

## 6. Uncertain

- The lighting of a jump fades over about 8 frames (the gate, deferred).
- A fixed collider of an animated prop stays at its rest pose; the room check warns.
- A skinned pose has no motion vectors until stage 63; stage 63 measures its own `jump_720p` with a character.
- The noise of a lamp with a linear field track (each step a soft reset) was not measured.
- `animate` asks `DataAssets` for the clip in each step (the reload of a changed file), so `DataAssets` is marked changed in each step while an animator plays.
