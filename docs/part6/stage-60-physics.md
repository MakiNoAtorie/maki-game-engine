# Stage 60: physics on Rapier

Plan: [README.md](README.md), 2.9 ("Physics crate"), 2.15 ("Physics (Rapier)", "Dependencies", "All before the slice"), 3.2 item 15. `OPEN_ITEMS.md`, "Open door leaf". Report: `docs/part6/stage-60.md`. Needs: stage 59 (merged, or its follow-up #74).

Goal: one system of collision, on the library Rapier, in a crate `physics` with no type of Rapier in its public API. The walls, the props, the doors, and the movers are colliders of it; the walking controller is its character controller; an `area` is a sensor of it (the same component, outputs, and hooks); an object can have a body that falls, hits, and is pushed; scripts get ray and shape queries. The open leaf of a door blocks.
Gate: a test in code: the player walks into a wall and slides along it, stops at a prop, and goes through a door only when it is open, and the open leaf blocks; an area tells "entered" and "left" and its interact as before; a box with a dynamic body falls to the floor and the player pushes it; a ray from a script hits the first collider and misses a hidden object. The game plays as before: the trip run passes with the same events and room loads; `bench.py gate`; the GPU time against `s59`; the symbol count of the engine DLL stays under the limit of `check.sh`.

State: written by the session from the plan and the code at commit `dcc46a2`. Approved by the owner (2026-10-17).

## Now (the code at stage 59)

- Collision is boxes on the floor plane, no physics engine (`crates/scene/src/collision.rs`): `Collider { min, max }` (axis-aligned, world space, each box blocks at each height) and `move_circle` (a circle pushed out of the boxes, two rounds). The room loader gives walls and props a `Collider` (`room_colliders`, `collider_around`, `crates/scene/src/room.rs`); a door has a `blocker` box at its closed place that goes away when it is fully open (`crates/scene/src/mover.rs`): the open leaf blocks nothing (`OPEN_ITEMS.md`).
- The walking controller: `Walker { radius, direction, speed }` and the move system in `crates/gameplay/src/player.rs` (`move_circle` with all `Collider` without `Hidden`). The player prefab has `radius`, `walk_speed`, `run_speed`.
- An area: `TriggerArea { width, depth }` (`crates/events/src/script.rs`) with the outputs `used`, `entered`, and the hooks `enter_area`, `leave_area`; the test is a rectangle on the floor around the object (`Area::area_at`, `crates/events/src/model.rs`).
- Show and hide (stage 56): a `Hidden` object has no collider (`crates/events/src/objects.rs` removes or restores its `Collider`), and its area gives no output.
- Nothing falls or is pushed; there is no ray query. The dependency `rapier3d` is named by the plan (2.15); the crate order of rule 1 has no `physics` yet.
- The engine DLL: 16,800 of 65,535 symbols; `check.sh` fails above 60,000.

## The way

- **The crate `physics`** (`maki-physics`), between `scene` and `events` in the order of rule 1 (`core -> platform -> assets -> scene -> physics -> events -> ...`): `events` (areas, queries of scripts) and `gameplay` (the walk) use it; it reads the components of `scene`. `CLAUDE.md` gets the new order and the crate line. On `rapier3d` (the current version, with the `glam` conversion feature if it has one; check its API before use). No type of Rapier in its public API: its own `Shape`, `Body`, `Hit`, `QueryFilter`.
- **The world of the physics** is a resource of the crate: the Rapier sets and pipeline. It steps in `FixedUpdate`, after the movers and before the events, with the fixed time step. The entities of `scene` and their Rapier handles are kept in a map of the crate (slotmap handles of Rapier inside, entity outside).
- **Colliders.** A component `collider` on any object: a shape (`Box` with half sizes, `Sphere`, `Capsule`, `FromMesh`: the box of the mesh as now, in a later stage a hull), a friction, and `sensor` (a flag). The room loader makes the walls and the props with `collider: true` into fixed colliders, as the boxes now; a door leaf and a mover are kinematic colliders that follow their pose, so an open leaf blocks. `Collider { min, max }` and `move_circle` go away when nothing uses them (expand, migrate, contract inside the stage).
- **Bodies.** A component `body` on any object: `Fixed` (default with a collider), `Dynamic` (mass, damping), `Kinematic` (moved by a command or a script, as a mover). The pose of a dynamic body goes back into the `Transform` of its entity after each step; the extract and the renderer read it as any pose (rule 3). Not saved; a room load starts them at their file place.
- **The character controller.** The walk is the character controller of Rapier: a capsule of `radius` and the height of the player prefab, slides along walls, steps up a small step, does not go up a slope above a limit (new fields of the player prefab, with defaults that keep the walk of now: `step_height`, `max_slope`), and pushes dynamic bodies (a force from a setting). Free movement only: the grid movement is stage 61. `Walker` keeps its fields.
- **Areas on sensors.** `TriggerArea` stays the same component in the file (`width`, `depth`; a height from the room) and is a sensor collider of the crate; "entered" and "left" come from the intersection events of the sensor with the player; "used" (interact while the player faces it) uses a shape query in front of the player. The hooks `enter_area` and `leave_area` as before. A hidden object: its collider and its sensor are off.
- **Queries.** `physics::ray(world, from, direction, length, filter) -> Option<Hit>` (entity, point, normal, distance) and `overlap(world, shape, pose, filter) -> Vec<Entity>`; `ScriptContext::ray` and `ScriptContext::overlap`. The line of sight of stage 61 uses `ray`.
- **The look of the room does not change**: the renderer reads poses as now. A dynamic body that moves changes the TLAS in its frame (2.9): a test room with many bodies is measured, not a room of the benchmark.

## Tickets

Approved by the owner (2026-10-17). GitHub issues #76 to #80.

| Ticket | Blocked by |
|---|---|
| #76 The crate `physics` on Rapier: the world, the step, colliders of the room as fixed and kinematic colliders, the symbol count (prefactor) | none |
| #77 The walking controller on the character controller; the open door leaf blocks | #76 |
| #78 Areas on sensors | #76 |
| #80 Bodies that fall and are pushed, colliders of any object, and a measure of many bodies | #77 |
| #79 Ray and shape queries for scripts | #76 |

#77, #78, and #79 can run at the same time after #76: #77 writes `crates/gameplay` (the walk) and `crates/scene` (the door leaf), #78 writes the area of `crates/events`, #79 writes the queries of the crate and `ScriptContext`. #80 after #77: the player pushes a body through the character controller.

## Done when

- The gate.
- Tests: a room gives the same blocking places as before (a wall, a prop, a closed door), and an open leaf blocks; the walk slides along a wall at an angle, stops at a prop, steps up a step below `step_height` and not above it, and does not go up a slope above `max_slope`; a hidden object does not block and its area gives no output, and it blocks again when shown; "entered", "left", "used", and the hooks of an area; a dynamic box falls, rests on the floor, and moves when the player walks into it; its pose is in its `Transform`; a kinematic mover carries no wrong push; a ray hits the nearest collider with its point and normal, misses a hidden object and a sensor with the filter; an overlap finds the bodies in a shape; a room load starts the bodies again.
- Tests and clippy, static and dynamic; a validation run; the trip run (the same events and room loads as stage 59, the positions of the script logged); `bench.py gate`; the benchmark against `s59`; the symbol count of the engine DLL; the reload test; the clean build time against stage 59.
- A run by hand, by the input script: walk along the walls of room A, through the door of room A when it opens, into an area; a test room with boxes that fall and that the player pushes.

## What to watch for

1. **The symbol limit**: Rapier, parry, and nalgebra add many symbols. Ticket A counts them first; above 40,000 in all, stop and ask (the way out of 2.3 is a second DLL).
2. **The pictures of the benchmark**: a walk of the trip and of `walk_720p` that goes along a wall can end at another place with the new controller, and then the picture differs. Compare the positions that the script logs with stage 59; a difference that is the new slide is a new base of the pictures, for the owner to accept (stop and ask), not a defect to hide.
3. **Determinism**: Rapier with the fixed step and no threads gives the same steps in each run; the benchmark and the tests need it. No `parallel` feature.
4. **Rule 2 and the API**: no type of Rapier or nalgebra outside the crate; the conversion to `glam` is inside.
5. **The order of the systems**: the movers move, then the physics steps, then the events see the areas; `ResetPlay` and a room load rebuild the world of the physics from the entities.
6. **Hidden objects** (stage 56): off in the physics, not removed, so show gives them back with the same handles.
7. **Build time**: a clean build and a rebuild after a line of a script, against stage 59 (`symbol-limit.md` has the method).
8. **Not in this stage**: joints, the grid movement and the path search (stage 61), the line of sight as a condition (stage 61), a hull or a mesh collider from a model file, ragdolls, vehicles, physics materials beyond friction, a debug view of the colliders (editor lane, item 34).
