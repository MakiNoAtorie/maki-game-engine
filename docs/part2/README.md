# Part 2: playable vignette

Goal: a player walks through two rooms, a door opens, the denoiser works with motion, and events are data (RPG Maker style).

Work one stage at a time, and the steps of a stage in their order. Implement only the given stage, then stop and report (working agreement in `CLAUDE.md`).

## Order

| Stage | File | State | Needs |
|---|---|---|---|
| 10 | [stage-10-review-fixes.md](stage-10-review-fixes.md) | Done. Fixes 4 and 5 not made; fix 5 needs a decision | - |
| 11 | [stage-11-player-moving-objects.md](stage-11-player-moving-objects.md) | Done | 10 (fixes 1, 2) |
| 12 | [stage-12-denoiser.md](stage-12-denoiser.md) | Done | 11 (motion vectors, something that moves), 10 (fix 3) |
| 13 | [stage-13-events-second-room.md](stage-13-events-second-room.md) | Done | 11 (scene crate, door, mover, player) |
| 14 | [stage-14-benchmark-review.md](stage-14-benchmark-review.md) | Done | 12, 13 |

Stages 12 and 13 do not need each other. Keep the order of the guide: the denoiser first, so the room work of stage 13 is judged on a clean picture.

## Changes against the guide

The guide was written before the perf rounds, and its crate order has one conflict. The stage files have these changes:

1. **The `scene` crate starts in stage 11, not in stage 13.** The guide puts `scene` and `events` left of `render`, and asks `scene` to spawn rooms and `events` to drive doors and lights. `MeshInstance`, `Light`, and `Camera` are in `render` now, so `scene` and `events` cannot name them. Fix: the components that describe the world live in `scene`, and `render` depends on `scene` and extracts them. Stage 11 makes the crate and puts the new components (`Door`, `Mover`, `Collider`, `Player`) there from the start. Stage 13 adds the room file format to it.
2. **Stage 11 has two parts, gameplay first.** Motion vectors cannot be checked while nothing moves.
3. **Stage 12 is the temporal half only.** The albedo image and the spatial filter exist.
4. **Stage 13 has a fixed order:** room files, then events, then content.

## Context update

Dependency order, with the two new crates:

`core -> platform -> assets -> scene -> events -> gpu -> render -> ui -> game`

- `scene`: the components that describe the world, the room data format, load and unload of rooms.
- `events`: flags and variables, event definitions, the event interpreter.
- `scene` and `events` must not depend on `gpu`, `render`, or `ui`.

Rules 1 to 6 of `CLAUDE.md` stay. New rules:

7. Gameplay state that must survive a room change (flags, variables, player state) lives in resources outside room entities.
8. Game content (rooms, events) is data, not code. A new room or event must not need a recompile.

## Notes

- Part 2 is on the branch `part2`. The perf rounds are commit `fb55fc5`.
