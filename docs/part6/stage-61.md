# Stage 61: movement on the grid, the path search, the line of sight

State: done. Stage file: [stage-61-grid.md](stage-61-grid.md). Tickets #82 to #87 (pull request #81). Plan: `README.md` 2.15, 2.7, 3.2 item 8.

Lanes: #82 (A) and #85 (D) at the same time; then #83 (B), #84 (C), #87 (F) at the same time; then #86 (E). Then fixes by the orchestrator (a blocked grid move, a path to a blocked cell, `sees` while a room starts, the light of the test room), one code review (standards and spec), and one lane of fixes.

## 1. What was built

- **The grid of a room** (#82): `RoomShell.cell` (default 1 m, not written at 1 m). `maki_gameplay::grid`: `GridModule`, `[grid]` (`diagonal`, `height`), the resource `WalkGrid`, made after each room load from the fixed colliders; doors and movers are tested by an overlap when a search reads a cell. A* (`find_cells`, a pure function; 4 or 8 neighbours with no corner cut; fixed tie order) and `WalkGrid::find_path`. A path to a blocked cell (a prop) ends at its free neighbour nearest to the start.
- **Characters that walk** (#83): the room component `walker` (`WalkerDef`: radius, height, speed) walks with the same character controller as the player. `PathFollow` holds the path; `steer_walkers` follows it, and a walker with no progress for 0.5 s (a closed door) asks a new path. The command `WalkTo(object, Place | Object)` waits until arrival; no path: a warning and the next row. `ScriptContext::walk_to`, `stop`, `arrived`, `no_path`. The seam: `maki_events::WalkGoal` (`Idle`, `Asked`, `Walking`, `Arrived`, `NoPath`), required by `WalkerDef` and changed by value, never inserted in play.
- **The grid walk** (#84): `[walk] mode = "Free" | "Grid"`. On the grid: a press moves one cell, a hold goes on, a press of another direction turns first, a blocked cell does not take the player, an off-center player walks to the center of its cell first, a move that a body blocks turns back after 3 times its time. The moves are play state (`GridMoves`, cleared on `ResetPlay`).
- **The line of sight** (#85): `Physics::line_of_sight` (sensors and two entities skipped); `ScriptContext::sees(target, distance, angle)`; the condition `Sees(object, target, distance)` (object 0: the object of the event; target 0: the player). `sees` is false while a room starts.
- **The templates** (#86): "Chaser" (rest, follow, search at the last seen place, give up and go back; logs its states at debug) and "Patrol" on `walk_to`. The test room `room_chase.ron` and `game/chase.toml`; the script `chaser` of the game is a copy of the template.
- **The snap of the editor** (#87): the step is the cell of the room divided by `[editor] snap_divisions` (default 4). An old config with `grid_step` still loads.

## 2. Files

- `crates/scene/src/room.rs`: `cell` of a room.
- `crates/physics/src/lib.rs`: `line_of_sight`.
- `crates/events/src/script.rs`, `model.rs`, `state.rs`, `builtin.rs`, `lib.rs`: `EYE_HEIGHT`, `can_see`, `object_entity`, `WalkGoal`, `walk_to`, `stop`, `arrived`, `no_path`, `sees`; the condition `Sees`, its check and text.
- `crates/gameplay/src/grid.rs` (new): the walk grid and A*.
- `crates/gameplay/src/walkers.rs` (new): `walker`, `PathFollow`, `steer_walkers`, `WalkTo`.
- `crates/gameplay/src/player.rs`: `WalkSettings`, the grid walk, `GridMoves`.
- `crates/gameplay/src/lib.rs`, `crates/engine/src/lib.rs`: exports; `GridModule` and `WalkersModule` in the module list.
- `crates/editor/src/lib.rs`, `panels.rs`, `README.md`: `snap_divisions`, the old key `grid_step` read and dropped.
- `crates/engine/templates/chaser.rs` (new), `patrol.rs`, `crates/engine/src/new_script.rs`: the templates.
- `game/scripts/src/chaser.rs` (new), `game/scripts/src/lib.rs`: the script `chaser` of the game, the test of the chase room.
- `game/assets/rooms/room_chase.ron` (new, and its id file), `game/chase.toml` (new): the test room and its config.
- `tools/editor_scripts/stage61.txt` (new): the run by hand.
- Tests: `crates/gameplay/src/tests/l61a.rs` to `l61c.rs`, `crates/events/src/tests/l61d.rs`, `crates/engine/tests/l61e.rs`, `templates.rs`, `crates/editor/src/tests.rs`.
- `docs/CONVENTIONS.md`: the request component of a script, the physics while a room starts; the line on code intelligence goes (a fact of the machine).

## 3. Results

| Check | Result |
|---|---|
| Gate: `WalkTo` around a wall, its wait | `crates/gameplay/src/tests/l61b.rs` |
| Gate: a press moves one cell; a wall cell does not take it | `crates/gameplay/src/tests/l61c.rs` |
| Gate: a ray of the line of sight is blocked by a wall, not by a sensor | `crates/events/src/tests/l61d.rs` |
| Gate: a chaser follows the player it sees, stops and goes back to rest after a wall hides the player | `crates/engine/tests/l61e.rs` |
| Tests of "Done when" | Each line has a test in `l61a` to `l61e`, the room tests in `crates/scene`, the snap test in `crates/editor/src/tests.rs`, and the template build in `crates/engine/tests/templates.rs` |
| Tests and clippy | `tools/check.sh` on the merged tree: format, comments, clippy and tests static and dynamic, symbol count, reload test: all pass |
| Validation run | The editor runs below: no validation error |
| Trip run (debug) | The same legs, events, and room loads (A, B, A) as stage 60; no error |
| Picture gate (`bench.py gate`) | `trip_720p`, `walk_720p`, `materials_720p`, `textured_walk_720p` equal; `test_1080p` noise (1,652 pixels at 0.001), as in stage 60 |
| Benchmark against `s60` | Run `s61`. GPU total +0.7 % to +11.6 % against the run of 2026-10-17; `bench.py confirm 46be1c3` of the two largest steps (`trip_720p`, `heavy_mixed_lights8_1080p`), back to back: -0.3 % to +0.8 %. The machine, not the code |
| Time of a path search | 100 x 100 cells around a long wall, with a mover (each cell read is a physics overlap): 0.67 ms in the dynamic profile, 8 to 18 ms in a debug test build. The search runs once for each request, in the fixed step of the walker that asks |
| Run by hand in the editor | `tools/editor_scripts/stage61.txt` in `room_chase.ron`, free and grid walk: the chaser sees the player at the end of the wall, follows, loses the player behind the wall, searches, gives up after 3 s, and goes back to rest. The same in both modes |

## 4. Deviations from the stage file

- `Physics::line_of_sight` has a fourth argument `also`: `QueryFilter` skips one entity, the query skips two.
- The eye height is the constant `maki_events::EYE_HEIGHT` (1.5 m), not the height of the walker: `events` is below `gameplay` and cannot read `Walker`.
- The condition `Sees` has no angle: it looks all around. Target 0 is the player (the player has no `ObjectId`).
- Speed is a field of `walker`, not of the chaser or the patrol: the walker owns its speed.
- The script `chaser` of the game is a copy of the template: the templates are not compiled into the game crate.
- A path to a blocked cell (a prop, a character) ends beside it, not `None`: `WalkTo` to an object with a collider otherwise never had a path.

## 5. Decisions where the stage file was silent

- `[grid] height` (1.7 m): the height of the test box of a cell. It is a second value beside `WalkerDef.height`; one grid serves all walkers.
- A* costs 10 straight, 14 diagonal; ties by cost, then the distance left, then the cell index; neighbours +X, -X, +Z, -Z, then the diagonals.
- The grid walk runs with the run action (`run_speed`), as the free walk does; the stage file says "at the walk speed".
- Two directions held on the grid: forward or back wins over left or right.
- A grid move that a body blocks turns back to its cell after 3 times the time of a free move.
- `sees` is false while the room starts (`EventRunner::starting_room`, the 2 steps in which events wait too): before, a chaser saw the player at the origin before the walls were in the physics.
- The cone of `sees` is the full angle in degrees on the floor plane; the facing is +Z of the object; the chaser default is 120 degrees.
- The test room `room_chase.ron` is not in a benchmark scenario: the pictures of the benchmark rooms do not change.

## 6. Uncertain

- The chaser flips between Follow and Search when the player stands at the edge of its view (the walker turns as it walks, and the cone turns with it). It does no harm: each Search walks to the last seen place.
- A doorway door sits in the wall line, outside the grid: a path does not go through a doorway (a room change is not a path).
- Colliders of a model file that loads after the first fixed step of a room are not in the grid; dynamic bodies are not in the grid.
- Two walkers that block each other search again each 0.5 s; there is no avoidance (out of scope).
- `EYE_HEIGHT` and `[grid] height` are constants beside `WalkerDef.height`; a later stage can move the height of a walker into `scene`.
