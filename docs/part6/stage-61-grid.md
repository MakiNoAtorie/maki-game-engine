# Stage 61: movement on the grid, the path search, the line of sight

Plan: [README.md](README.md), 2.15 ("Movement", "Grid", the slice item 6: an enemy that follows the player and loses the player out of its line of sight), 2.7 (the templates "Chaser" and "Patrol"), 3.2 item 8. Report: `docs/part6/stage-61.md`. Needs: stage 60 (physics).

Goal: a room has a grid (a cell size, 1 m if not set). The walk of the player is free or on the grid, as a setting of the walking module. Characters that are not the player walk by a path search on a grid that the room load makes from the collision of the room. The physics gives the line of sight. An enemy made from the template "Chaser" rests, sees the player, follows the player around walls, and gives up when it loses the player out of its line of sight. The snap of the editor uses the grid of the room.
Gate: a test in code: a room with a wall between a character and a place: `WalkTo` walks the character around the wall to the place and waits until it is there; on the grid, a press of a direction moves the player one cell and a cell with a wall does not take it; a ray of the line of sight is blocked by a wall and not by a sensor; a chaser follows the player that it sees, and stops and goes back to rest some seconds after a wall hides the player. The game plays as before with the free walk: the trip run, `bench.py gate`, the GPU time against `s60`.

State: written by the session from the plan and the code at commit `4fd5c2f`. Approved by the owner (2026-10-17); not started: the owner does it later.

## Now (the code at stage 60)

- The walk (`crates/gameplay/src/player.rs`): `Walker { radius, height, step_height, max_slope, push_force, direction, speed }` and `walk` on `Physics::move_character`. `steer_player` sets the direction of the player from the input actions (free, any angle). No other system steers a walker: "some other system tells it where to go (... an NPC module for its characters)". There is no component of a walker for an object of a room.
- A room (`RoomShell`, `crates/scene/src/room.rs`): width, depth, height, textures. No grid.
- The physics (stage 60): fixed colliders of the room, kinematic doors and movers, `Physics::ray`, `overlap`, `move_character`; `ScriptContext::ray`, `overlap`, `object_id`. The floor is at y = 0.
- `MoveObject(object, place, seconds)` (stage 56) moves an object in a line, through walls.
- The templates of "New script" (`crates/engine/templates/`): `patrol.rs` and `movement.rs` are there, no `chaser.rs`.
- The editor snaps a position to `grid_step` (0.25 m) of its settings, the same in each room.

## The way

- **The grid of a room.** `RoomShell.cell` (meters, `#[serde(default)]` 1.0, not written when 1.0): the size of a square cell on the floor, with a cell edge at the corner of the room. A cell is a place of the grid: `(column, row)`; its center is a point on the floor.
- **The walk grid** (`maki_gameplay` or a crate module of its own; not the renderer): made at each room load from the physics: a cell is blocked when a box of the cell (a little smaller) and the height of a walker overlaps a fixed collider. A kinematic collider (a door, a mover) is not in the made grid; the path search tests it when it reads a cell (an overlap), so a door that opens lets a path through. The grid is made again after a room load, not in each frame.
- **The path search**: A* on the walk grid, 4 neighbours (and 8 with no corner cut, as a setting of the module), from the cell of a walker to the cell of a place; the path is the list of cell centers, the last one the place itself. No path: `None`. A pure function with a test of each case.
- **Characters that walk.** A component `walker` for an object of a room (`#[derive(Fields)]`: radius, height, speed; the step and slope of the player prefab as defaults): the object gets a `Walker` and walks with the same `walk` as the player (the character controller). A component `PathFollow` (not in the file) holds the path that it walks; a system steers the `Walker` to the next center. The command `WalkTo(object, target)` (a place, or an object: its place at the start) walks the object by a path search and waits until it is there (or gives a warning and goes on when there is no path); `ScriptContext::walk_to(target)` and `stop()`; a script reads `arrived()`.
- **Movement on the grid.** `[walk] mode = "Free" | "Grid"` (default `Free`): on the grid, the player moves from cell center to cell center in the 4 directions of the input actions: a press starts a move of one cell if the next cell is free, a hold goes on cell by cell, and a press of another direction turns the player first (as RPG Maker games do); the move is the character controller toward the center, at the walk speed. A walker of `WalkTo` on the grid walks the same path.
- **The line of sight.** `Physics::line_of_sight(from, to, filter) -> bool`: no collider between the two points (sensors and the two entities skipped). `ScriptContext::sees(target, distance, angle)`: the target is closer than `distance`, inside the angle of the facing of the object, and in line of sight from the eyes (a height of the walker). A condition `Sees(object, target, distance)` for events.
- **The template "Chaser"** (`crates/engine/templates/chaser.rs`, in the list of "New script"): states rest, follow, search, give up: it sees the player (`sees`), follows by `walk_to` (a new path when the player is more than a cell from the last end), keeps the last place where it saw the player for some seconds, then goes back to its start. Its fields: distance, angle, give-up seconds, speed. The template "Patrol" walks its points with `walk_to`.
- **The editor.** The snap step is the cell of the room divided by `[editor] snap_divisions` (default 4: 0.25 m in a room of 1 m, as now). The grid lines of the view (if the view has them) are at the cells of the room.

## Tickets

Approved by the owner (2026-10-17). The issues are not published yet: at the start of the stage.

| Ticket | Blocked by |
|---|---|
| A. The grid of a room, the walk grid from the physics, the path search (prefactor) | none |
| B. Characters that walk: the component `walker`, the path, `WalkTo`, `walk_to` of a script | A |
| C. Movement on the grid for the player | A |
| D. The line of sight: the query, `sees` of a script, the condition `Sees` | none |
| E. The templates "Chaser" and "Patrol" on the path and the line of sight | B, D |
| F. The snap of the editor from the grid of the room | A |

A and D can start at once; B, C, F after A; E after B and D. B and C both touch the walk in `crates/gameplay/src/player.rs`: B adds a steer system and a component in a file of its own, C changes `steer_player`; they keep to their own functions.

## Done when

- The gate.
- Tests: the cell of a room reads and writes (and is not written at 1 m); the walk grid of a room with a wall, a prop, a door; a door that opens frees its cells for a path; A* finds the shortest path around a wall, finds none to a closed room, keeps no corner cut with 8 neighbours; `WalkTo` to a place and to an object, its wait, no path; `walk_to`, `stop`, `arrived` of a script; on the grid, a press moves one cell, a hold goes on, a press of another direction turns, a blocked cell does not take the player, the free walk is the walk of stage 60; `line_of_sight` blocked by a wall, not by a sensor or the two entities; `sees` with its distance and angle; the condition `Sees`; the chaser follows a player that it sees, searches at the last place, gives up and goes back; the patrol walks its points around a wall; "New script" with "Chaser" builds; the snap step of a room of 2 m with 4 divisions is 0.5 m.
- Tests and clippy, static and dynamic; a validation run; the trip run (the same as stage 60); `bench.py gate`; the benchmark against `s60`; the symbol count; the reload test; the templates of "New script" build.
- A run by hand, by the input script: a test room with a chaser and a wall: the chaser sees the player, follows around the wall, loses the player behind the wall, and goes back; the player on the grid in the same room.

## What to watch for

1. **The pictures**: the free walk is the default and does not change; the benchmark rooms have no walker that is not the player. `bench.py gate`.
2. **The time of a path search**: a room of 100 x 100 cells is 10,000 cells; a search is in the fixed step of the walker that asks, not in each step of each walker. Measure a search in the largest test room.
3. **The doors**: a closed door blocks a path, an open one does not; a door that closes on a walker in its way does not hold the walker in the door forever (the walker stops and asks a new path).
4. **Determinism**: the order of A* (ties) and of the walkers is fixed, so a test and a benchmark give the same paths in each run.
5. **Rule 4**: the grid walk reads the input actions, not keys.
6. **The script DLL**: `ScriptContext` gets new functions. The reload test passes; the templates of "New script" build.
7. **Not in this stage**: a navigation mesh (2.15: later, if a room needs it), a path in 3D (stairs, levels), crowds and avoidance between walkers (two walkers can push each other; a walker waits and asks again), the animation of a walk (stage 62), a debug view of the grid and the paths (editor lane, item 34).
