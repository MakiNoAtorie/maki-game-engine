# Stage 60: physics on Rapier

State: done. Stage file: [stage-60-physics.md](stage-60-physics.md). Tickets #76 to #80 (pull request #75). Plan: `README.md` 2.9, 2.15, 3.2 item 15.

#76 as one lane (the measure first); then three lanes at the same time: #77, #78, #79; then #80. Two merges by hand (the physics dependency of `events` and the physics module of the test game, from #78 and #79). Then one code review (standards and spec) and one lane of fixes.

## 1. What was built

- **The crate `physics`** (#76): `maki-physics` on `rapier3d` 0.36.1 (`dim3`, `f32`, `std`; no threads), between `scene` and `events` in the order of rule 1 (`CLAUDE.md` changed). No type of Rapier or nalgebra in its public API: `Shape`, `Kinematic`, `Sensor`, `QueryFilter`, `Hit`, `CharacterSettings`, `CharacterMove`. `PhysicsModule` steps in `FixedUpdate` (`PhysicsSet::Step`, after the movers); the world is synced each step from the entities (room loads, `ResetPlay`, doors, `Hidden`). The boxes of the room (`Collider` of `scene`) are its fixed colliders; a door leaf and a mover are kinematic colliders that follow their pose.
- **The walk** (#77): `Physics::move_character` with an upright capsule of the player prefab: it slides, steps up (`step_height`, 0.25 m), stops at a slope (`max_slope`, 45 degrees). The fixed door blocker is gone: the leaf blocks at each pose, so the open leaf blocks (`OPEN_ITEMS.md`, "Open door leaf", removed). `move_circle` is gone; `Collider` stays as the source of the fixed colliders.
- **Areas** (#78 and the review): an `area` is a sensor collider (`Sensor`, axis-aligned, 2 m high) of its own, also on a door or a mover; "inside" is a shape test after the step (`Physics::in_sensor`), so the events know it in the step after the move; "used" is a reach query (0.9 m) and the facing rule as before.
- **Queries** (#79): `Physics::ray`, `overlap`; `ScriptContext::ray`, `overlap`, `object_id`; the filter skips a hidden object, sensors, and the entity of the script.
- **Bodies** (#80): the components `collider` (`FromMesh`, `Box`, `Sphere`, `Capsule`; friction; sensor) and `body` (`Fixed`, `Dynamic { mass, damping }`, `Kinematic`) on any object, from their field tables in the inspector. A dynamic body falls onto the floor of the physics (a fixed box with its top at y = 0, which no query sees), rests, hits, and its pose goes into its `Interpolated` pose; the player pushes it (`push_force`, 200 N); a kinematic body pushes it; a room load and `ResetPlay` start it again; bodies are not saved. The benchmark scenario `bodies_720p` (50 boxes that fall).
- **Review fixes**: an area on a mover or a door gets its own sensor; the sensor does not turn with its object; the sensor lag down to one step; a teleport of a kinematic body (spawn, transfer, `ResetPlay`, a jump of more than 0.5 m) gives it no speed, so it throws no box; `ResetPlay` starts a dynamic body with only a `Transform`; poses are written only when they differ; named constants.
- **Outside the tickets**: the converter thread of the editor ends with its app (the end of the app waits for it): an editor test failed when the thread wrote an id file into the temp folder at its delete, and after a reload of the code two threads could look at one folder.

## 2. Results

| Check | Result |
|---|---|
| Gate: the walk along a wall, at a prop, through a door only when it is open, the open leaf blocks | `crates/gameplay/src/player/walk_tests.rs` |
| Gate: an area "entered", "left", and interact | `crates/events/src/tests.rs`, `tests/objects.rs` |
| Gate: a dynamic box falls, rests, the player pushes it | `crates/physics/src/tests.rs`, `walk_tests.rs` |
| Gate: a ray of a script hits the first collider and misses a hidden object | `tests/objects.rs`, `crates/physics/src/tests.rs` |
| Tests of "Done when" | Each line has a test (the files above), but the climb of a slope below `max_slope` (section 5) |
| Tests and clippy | `tools/check.sh` on the merged tree with the fixes: format, comments, clippy and tests static and dynamic, symbol count, reload test: all pass |
| Trip run (debug) | The events, messages, and room loads the same as stage 59, no validation error; the positions of some legs differ by up to 6 cm (the gap of the controller at a wall, the open leaf that brushes the player, a walker at a wall that stands still) |
| Picture gate (`bench.py gate`) | `trip_720p` CHANGED (530,782 pixels: the last frame is at another place, from the new walk); `walk_720p`, `materials_720p`, `textured_walk_720p` equal; `test_1080p` noise (1,671 pixels at 0.001). The owner accepted the new trip picture as the base (2026-10-17) |
| Symbols of the engine DLL | 19,004 of 65,535 (stage 59: 16,800; Rapier alone: +2,144) |
| Build times (dynamic profile, one run each) | Clean 182 s to 241 s (+32 %); a rebuild after a line of a script 2-3 s to 5-6 s |
| Benchmark against `s59` | Run `s60`. GPU total -4.8 % to +1.1 %: no step to confirm |
| Many bodies | `bodies_720p` (50 boxes that fall and rest, 300 frames from the start): GPU total median 2.17 ms; the TLAS build median 0.06 ms, p95 0.24 ms while they fall |
| Run by hand in the editor | By the input script, on a check room (room D with three dynamic boxes, removed after): the boxes fall and rest on each other; interact at the button opens the door through its wire; the player walks into the boxes and pushes them to the wall |

## 3. Deviations from the stage file

- **`Collider { min, max }` stays**: it is the source of the fixed colliders of the room (the stage file had it go away).
- **The floor**: a fixed box of the physics with its top at y = 0, not of the room; no query sees it, so the walk keeps the floor at y = 0 in code.
- **The area height** is a fixed 2 m (the stage file: a height from the room).
- **The new picture of `trip_720p`** is a new base by the decision of the owner.
- **Queries are methods of `Physics`**, not free functions.

## 4. Decisions where the stage file was silent

- A walker falls at 5 m/s (no acceleration), only when its walk move ends with no ground; a move under 0.1 mm is no move (else a walker at a wall never stood still).
- A standing walker makes no controller call (with no move, Rapier pushes the capsule out of what it touches, maybe into a wall).
- The player height: the height of the capsule mesh, or 1.7 m for a model file.
- `Box`, `Sphere`, `Capsule` sizes are in meters around the origin of the object; only `FromMesh` takes the scale. A mass under 0.01 kg is 0.01.
- A jump of a kinematic body: a snap of `Interpolated`, or more than 0.5 m in one step.
- `step_height` 0.25 m keeps the walk of the rooms of `game/`: no blocking prop there is lower (the lowest, the button of room D, is 0.3 m).
- The converter thread sleeps in slices of 50 ms, so the end of an app waits at most that, or the end of a conversion that runs.

## 5. Uncertain

- A walker on a kinematic slope stalls (Rapier 0.36); a fixed slope climbs. No test of a climb below `max_slope` (a test can make a slope only as a kinematic collider).
- A walker on a moving platform is not carried, and a rising mover does not lift it.
- The walk sees a new, shown, or moved collider one step late (it runs before the step that syncs the world).
- A dynamic body outside the floor (±500 m) falls for ever.
- A `Sensor` stays when its `area` component is removed in the editor.
- The push force is an impulse each step while the player touches a body; a heavy body can feel stiff at 50 Hz.
