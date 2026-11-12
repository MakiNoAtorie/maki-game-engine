# Stage 64: the timeline, and light animation

State: done. Stage file: [stage-64-timeline.md](stage-64-timeline.md). Tickets #102 to #108 (pull request #101). Plan: `README.md` 2.6, 2.7, 2.13, 2.15, 3.2 items 6 and 26.

Lanes: #102 (A) and #106 (E) at the same time; then #103 (B) and #105 (D); then #107 (F) and #104 (C); then #108 (G). Then one code review (standards and spec) and one lane of fixes. Outside the tickets: the loader of the script DLL after the update of CI to Rust 1.100.

## 1. What was built

- **The track write** (#102): `write_tracks` from the play of the animator; the timeline and the light animation as modules of their own.
- **The timeline** (#103): the object component `timeline` (`mode`, `fps`, `length`, `looping`, `playing`, `tracks` of `(object: ObjectId, track: Track)`; object 0 is its own object). It plays at the end of `animate`; pose tracks are relative to the pose of each target at the start; a step key is a counted jump; the end sets `playing` false; `ResetPlay`; the room check names an unknown object. A text key on `animator.clip` cuts the clip of a character.
- **The commands** (#104): `PlayTimeline(timeline, wait)` and `StopTimeline(timeline, end)` (end: the values of the last keys, a skip), with rows, help, and the room check.
- **Light animation** (#105): the light fields `temperature` (Kelvin, `kelvin_rgb`), `flicker` and `flicker_rate` (a seeded pattern, no random state), `pulse` and `pulse_seconds`, `fade`; the component `LightAnimation` on an animated light only; the live form writes the base values, the animation is on top.
- **The camera object** (#106): the component `camera` (`fov`, `active`): an active camera object takes the place of the follow camera; a timeline moves it by a pose track and switches it by a switch track.
- **The timeline panel** (#107): a tab of the bottom dock, first in the layout "Animate": a row for each track, its keys on a time ruler, a play head that a drag moves (a preview by `patch_room`), "Key", "Delete key", a key drag, "Add track"; each edit is one undo step.
- **Content and measure** (#108): the template "Cutscene trigger"; three light presets as prefabs (candle, fluorescent tube, warning lamp); the test room `room_cutscene.ron` (`game/cutscene.toml`); the scenarios `light_anim_720p` and `light_still_720p`, and `bench.py lightanim`; the input script `tools/editor_scripts/stage64.txt`.
- **Review fixes**: the test rooms name files by path and id (the editor marked them changed at open; the test of the test rooms now runs `correct_paths`); one function for the pose of a key (`set_channel`, `pose_on_rest`) in the game and the preview of the panel; `Timeline::NAME`; one seed of the flicker; a skip of the cutscene template that holds while the timeline plays; tests of a text key, a looping timeline, `playing` at the room load, and `PlayTimeline` with no timeline.
- **Outside the tickets**: `load_project` finds the engine DLL beside the script DLL, and the loader test finds the DLLs of its build above its own folder: Cargo 1.100 puts a test program below `target/<profile>` (CI failed on each branch). The local toolchain is 1.100 too.

## 2. Files

- `crates/scene/src/animation.rs`: `write_tracks`, the hook of the timeline, `set_channel` and `pose_on_rest` public.
- `crates/scene/src/timeline.rs` (new): `Timeline`, `TimelineTrack`, `TimelinePlayback`, `play_timeline`, `stop_timeline`.
- `crates/scene/src/light_animation.rs` (new), `room.rs`, `object.rs`, `lib.rs`: the light fields, `kelvin_rgb`, `LightAnimation`, `flicker_seed`.
- `crates/events/src/timeline.rs` (new), `lib.rs`: `PlayTimeline`, `StopTimeline`.
- `crates/gameplay/src/camera.rs`, `lib.rs`, `Cargo.toml`: the camera object.
- `crates/editor/src/timeline_panel.rs` (new), `lib.rs`, `panels.rs`, `document.rs`, `ops.rs`, `tests.rs`: the panel; the layout "Animate".
- `crates/engine/templates/cutscene_trigger.rs` (new), `crates/engine/src/new_script.rs`: the template.
- `crates/engine/src/loader.rs`, `programs/editor/tests/loader.rs`: the DLL paths under Cargo 1.100.
- `game/assets/rooms/room_cutscene.ron` (new), `room_skin.ron`, `prefabs/candle.prefab`, `fluorescent.prefab`, `warning_lamp.prefab` (new, with id files), `game/cutscene.toml` (new), `game/scripts/src/lib.rs`.
- `tools/bench.py`, `tools/editor_scripts/stage64.txt` (new), `CODING_STANDARDS.md` (the exceptions of the rule of the benchmark rooms).
- Tests: `crates/scene/src/tests/l64b.rs`, `l64d.rs`, `crates/events/src/tests/l64c.rs`, `crates/gameplay/src/tests/l64e.rs`, `crates/editor/src/timeline_panel.rs`, `crates/engine/tests/l64x.rs`, `templates.rs`.

## 3. Results

| Check | Result |
|---|---|
| Tests of "Done when" | Each line has a test in the files of section 2 |
| Tests and clippy | `tools/check.sh` on the merged tree with Rust 1.100: all pass (after one new clippy lint of 1.100 in `crates/editor/src/overlay.rs`, fixed on the stage 63 branch) |
| Validation run | The editor runs of tickets F and G: no validation error |
| Trip run (debug) | Rooms A, B, A; no error |
| Picture gate (`bench.py gate`) | `trip_720p`, `walk_720p`, `materials_720p`, `textured_walk_720p` equal; `test_1080p` once CHANGED, its run again noise (the known flake, `stage-63.md`) |
| Benchmark against `s63` | Run `s64`. GPU total -1.7 % to +5.9 %; only `heavy_mixed_lights8_1080p` over 3 % (+5.9 %), a scenario with no change of this stage in its code path and a drift of up to 11 % between runs before (`stage-61.md`). Not confirmed by `bench.py confirm` (the owner's rule: no speed work in a stage) |
| An animated light (`bench.py lightanim`) | A red lamp that pulses, against the same room with the lamp still: relmse 0.00356 against 0.00042 at frame 120 (8.5 times); GPU time equal. A pulse writes the light in each fixed step, so the history of the denoiser starts again in each step. For the quality session |
| Run by hand in the editor | `stage64.txt` in `room_cutscene.ron`: the walk into the area starts the cutscene: the camera object takes the view, the figure walks, hops, then waves, the candle flickers; after 6 s the follow camera is back and a move works. The panel: a scrub to 4 s and "Key" on the light track: a diamond at 4 s |

## 4. Deviations from the stage file

- The panel deletes a key with a button "Delete key": the Delete key of the editor runs before the panel (`command_keys`) and deletes the selected object.
- The config of the test room is `game/cutscene.toml`, not `game/timeline.toml`.
- The light fields stay flat in `LightSource`: serde does not allow `flatten` with `deny_unknown_fields`, which the room format needs.
- The camera logic is in `follow_player`: a system of its own had no order to `shake_camera` of the game.

## 5. Decisions where the stage file was silent

- A timeline plays at the end of `animate`, so a timeline track wins over an animator track on the same channel.
- The start poses of a timeline are kept across a restart and cleared by `ResetPlay` (no drift on a restart).
- `kelvin_rgb`: the Planckian locus of Kim et al. (2002), 1667 to 25000 K, XYZ to linear sRGB, divided by the color of 6500 K (6500 K is white).
- `LightAnimation` stays on a light when its fields go back to 0 (no second change of archetype); a live edit that gives a still light an animation field adds it in play.
- A camera object is `active` by default; two active cameras: the lowest id, a warning once.
- `PlayTimeline(wait: true)` on a looping timeline: a warning of the room check, and no wait at run time.
- A text key on `animator.clip` is the way a cutscene changes the clip of a character.

## 6. Uncertain

- The noise of an animated light (section 3): each write of a light is a soft reset.
- The race of the cutscene template (a first `step` before the play row) did not show in a headless run; the template is robust to it now.
- The editor does not preview the pose of a file prop (the figure) in a scrub.
- The camera object in the editor while not in play is not taken over; its view in play was checked by hand only.
