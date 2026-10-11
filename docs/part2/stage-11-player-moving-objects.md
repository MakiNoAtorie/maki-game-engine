# Stage 11: player and moving objects

State: done. Changes against this plan:

- Lights have no `position` field: the position is the `Transform` of the entity, so a light can use the interpolation (step 3) and a mover.
- Interpolation is in `core` (`Interpolated`, schedule `FixedFirst`): simulation code writes `Interpolated::current`, not `Transform`.
- Collision (step 4) is not "X, then Z": the circle makes the full step and each box pushes it out by the shortest way. With "X, then Z" the circle stopped dead at a box corner.
- The camera clamp (step 5) keeps the target `camera_margin` meters from the walls. The view still shows the outside: the room is an open box seen from outside (`OPEN_ITEMS.md`).
- The room has a doorway with a passage (`Mesh::room_with_doorways`), so the door has a wall to be in.
- Cost of step 6 (`bench.py compare s11b s11c`): `trace` 0.314 to 0.369 ms (`test_720p`), 1.977 to 2.310 ms (`heavy_mixed_1080p`).

Do the steps in order. Each step ends with a check; do not start the next step before the check passes. Steps 1 to 5 are gameplay and must change little in the renderer. Steps 6 and 7 are renderer work.

## Steps

### 1. The `scene` crate, with the world components

- [x] New crate `crates/scene` (`maki-scene`), after `assets` in the order. `render` and `game` depend on it.
- [x] Move `MeshInstance`, `Light`, and `Camera` from `render` to `scene`. Only the components: extraction and `Camera::ray_basis` use stay in `render`.
- [x] Update `CLAUDE.md`: crate order, rules 7 and 8, the target.
- Check: no behavior change. `bench.py same` against run `stage10` is `ok`. `cargo tree -p maki-scene` shows no `gpu`, `render`, `ui`.

### 2. Actions

- [x] New actions in `maki_platform::Action`: move (4 directions), run, interact, toggle light control, toggle doors (debug).
- [x] Bindings: WASD and arrows move, Shift runs, Z and Enter interact, one key for each toggle.
- [x] A resource says who gets the move actions: the player (default) or the light. The light controller reads the same move actions when it has control; the `MoveLight*` actions go away.
- Check: the help panel lists the new actions with no edit to the panel. The toggle moves the light as before.

### 3. Player

- [x] `Player` component (in `scene`) and a `PlayerController` module (in `game`).
- [x] Mesh: glTF from config, else a generated capsule (`Mesh::capsule` in `assets`). The mesh is its own model: a model is one BLAS.
- [x] Movement in `FixedUpdate`, walk and run speed from config. The player faces the movement direction.
- [x] Interpolation: the entity keeps the pose of the fixed step before. A system in `Update` writes `Transform` as the blend of the two poses with `Time::alpha`. Use it for the light too (`OPEN_ITEMS.md`, "Light motion between fixed steps").
- Check: the player walks through walls, smoothly, at 60 Hz and with vsync off. The picture is noisy while it moves (expected).

### 4. Collision

- [x] `Collider` component (in `scene`): an axis-aligned box. The room walls and the static props get one.
- [x] The player is a circle on the floor plane against the boxes. Move on X, then on Z, so the player slides along a wall. No physics engine.
- [x] Unit test: a move into a box stops at its face; a move along it does not stop.
- Check: the player cannot leave the room or enter a prop.

### 5. Camera, door, mover

- [x] Camera follow: the target moves toward the player with a smoothing time from config. Clamp so the view stays in the room. In `Update`, after the interpolation.
- [x] `Mover` component (in `scene`): two poses, a duration, a state (at A, to B, at B, to A). A system in `FixedUpdate` moves it; it uses the interpolation of step 3.
- [x] `Door`: a mover that turns on a hinge, with a collider that blocks while the door is not open.
- [x] The debug action toggles all doors and movers.
- Check: the camera follows and never shows the outside. The door opens, closes, and blocks. Validation is clean.
- Check: `git diff --stat` of steps 2 to 5 shows no file of `crates/render` or `crates/gpu`. If it does, find out why before step 6.

### 6. Motion vectors

- [x] Frame data buffer: one for each frame slot, as `LightBuffers`. It holds the previous camera basis and position.
- [x] Instance data buffer: the previous transform of each TLAS instance, at the index that the ray query gives.
- [x] `motion` image (`Rg16Float`) from the trace pass: current screen position minus previous screen position of the surface point, in pixels.
- [x] `normal` and `albedo` use the center of the pixel for the surface data (not the random position), and they are written in each frame that traces.
- Check: unit test of the projection (a point, two cameras, the expected vector). Validation is clean. `bench.py compare`: the cost of the new stores is known and in the report.

### 7. Debug views

- [x] A debug panel (registered by the render or game module, not in the ui crate) with a view mode: final, motion vectors, depth, normals.
- [x] The tonemap pass, or one debug pass before it, shows the selected image.
- Check: static geometry shows motion only while the camera moves. The door shows motion while it turns and the camera is still. The player shows motion against a still floor.

Accumulation keeps its reset behavior. A noisy picture during movement is expected until stage 12.

## Repo state that this stage meets

- Stage 10 put `MeshInstance` in the render crate. `ExtractedInstance` has `id` and `previous_transform`, `ExtractedCamera` has `previous`. No pass reads them yet.
- `previous_transform` is the transform of the last `Extract`. With step 3, that is the interpolated pose, so the two transforms are one rendered frame apart.
- `Action` is one central enum in `platform`: a new action is an edit there. The guide accepts this (stage 2: "a central enum for now").
- All meshes of a model share one BLAS. The player, the door, and each mover must be their own model, or get their own BLAS (`OPEN_ITEMS.md`, "Moving instance").
- The `motion` image was removed in perf round 1 (R2). There is no depth image: `normal` holds the distance in its fourth channel.
- `Trace` gets its parameters in push constants (128 bytes maximum, 84 in use).
- No game code removes or moves an entity: the removal and move paths of extraction ran only in part.

## What to watch for

- Size of the renderer diff. The player and the door are mostly new modules. Large renderer edits outside steps 1, 6, and 7 mean a seam was wrong: find out why before stage 12.
- Jitter of the player at a high frame rate: the interpolation alpha is not used.
- Motion vector view: static geometry shows motion only when the camera moves.

## Done when

- [x] The player walks in the room with collision.
- [x] The camera follows.
- [x] A door opens, closes, and blocks correctly.
- [x] Motion vectors look right in the debug view.
- [x] Validation is clean.
- [x] `OPEN_ITEMS.md`: items that now pass are removed.
- [x] Stop and report.
