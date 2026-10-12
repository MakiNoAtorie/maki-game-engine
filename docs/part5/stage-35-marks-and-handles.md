# Stage 35: marks with depth, missing handles

Plan: [README.md](README.md). Report: `docs/part5/stage-35.md`. Item: M8 of [`../part4/MISSING.md`](../part4/MISSING.md).

Goal: a mark of the editor is hidden by what is in front of it, and the mouse does what only the numbers of the property panel did.
Gate: the grid does not show on a prop. A cone light is aimed and an event area is sized with the mouse.

Written after the report of stage 34, from the code at `88d8f22`.

## Context in the code

### The marks

- `crates/editor/src/overlay.rs`: the marks are lines of the egui painter. `Pen::line` turns two points of the room into pixels (`View::segment`). No depth: the comment of the file says so.
- The trace pass writes, for each pixel, the world normal and the distance from the camera along the ray: the image `normal`, `Rgba16Float`, distance in `w` (`crates/render/src/passes.rs:65`). It is a guide of the denoiser, at the internal resolution.
  - Glass: the first glass surface.
  - A mirror: the surface that the mirror shows, with the length of the full path (`trace.glsl:981`). The distance at a mirror pixel is not that of the mirror.
  - All channels of all images of the trace pass are in use: a new value is a new image.
- `Gpu::read_image` (`crates/gpu/src/lib.rs:1051`) copies an image to the CPU between frames. It waits for the GPU. `dump_picture` of the benchmark uses it, with `FrameGraph::image`.
- `Accumulation::samples` (`render_world.0`) starts again when the picture changes (camera, room, light).
- `view::pick` has the ray against each triangle of the room, and `Mesh::material` tells a mirror.
- `GraphicsPipelineDesc` has one state (`OPEN_ITEMS.md`, "Graphics pipeline state"): a line pass with a depth test needs a second one, a vertex format, and a depth image that the frame does not have.

### The gizmo

- `crates/editor/src/gizmo.rs`: `Handle` is an axis or the floor square. `Gizmo::of` is `None` for an item with no pose (a doorway). `Drag::apply` makes the document again from the start of the drag.
- A turn (`Tool::Rotate`) changes the rotation of each selected pose at its own place.
- `Document::place(item, second)`: for a mover, pose `a` or pose `b`. A move with the checkbox off moves `a` and leaves `b`.
- `Editor::viewport` (`lib.rs:365`) takes a handle with `Gizmo::handle_at` and starts a `Drag`.
- A light has `direction` and `cone` (`LightDef`); the overlay draws the cone of a selected light, 2 m long.
- An event area is a rectangle on the floor (`Area`: center, size). A doorway is on a wall: `offset`, `width`, `height` (`Doorway::span`, `Wall::frame`).

## Prompt

```text
Stage 35: marks with depth, missing handles.
Read docs/part5/stage-35-marks-and-handles.md and item 8 of
docs/part4/MISSING.md first.

Marks with depth
- The editor reads the distance image of the trace pass after the
  picture changed, not in each frame, and cuts the lines of its marks
  with it: a part of a line that is behind a surface is not drawn.
- The render crate gives the distances; the editor does not know the
  name or the format of the image.
- A mirror: its pixels have the distance of what it shows. The editor
  tests the mirror meshes on the CPU, as the pick does.
- Cut: the grid, and the marks of the items that are not selected
  (event areas, spawn points, doors, movers) with their names.
  On top, as before: the selection, the item under the pointer, the
  gizmo.
- With no distance image (the first frame, a test) the marks are as
  before.

Handles
- A cone light: a handle where the middle of the cone meets the floor.
  A drag aims the light at the point of the floor under the pointer.
- An event area: a handle at each corner. A drag moves the corner on
  the floor; the corner across stays.
- A doorway: a handle at each top corner. A drag on the wall changes
  the width and the height; the other side stays.
- These handles snap to the grid as a move does.
- A turn of more than one item turns the group around the gizmo.
- A move of a mover moves its two poses. With the checkbox, the
  second pose only.

Out of scope: a line pass of the frame graph, a new image of the trace
pass, a group scale, a turn of an event area, handles of the room size.

Done when: a test cuts a line with a distance image; a test for each
new handle and for the group turn; in the running editor the grid does
not show on the cube, the sphere, the glass ball, and the two mirrors
of room A; trip_720p is byte-equal and the GPU time of the game does
not move. Update OPEN_ITEMS.md ("Marks behind objects", "What the
gizmo does not change", "Graphics pipeline state"). Report.
```

## What to watch for

- **The game.** The read of the image is in the editor only: no cost in the game, `trip_720p` byte-equal.
- **The cost in the editor.** The time of one read, and how often it happens while the camera is at rest (never) and while it moves (each frame).
- **One frame late.** The image is of the frame before: while the camera moves, the cut is one frame behind the marks.
- **The floor.** The grid is on the floor: the cut must not eat the grid where nothing is in front of it (half floats, a slack).
- **Mirrors and glass.** No grid on the mirrors of room A, on the glass ball, on the mirror ball of room B.
- **The near walls.** The camera looks through the back side of the room box: marks in the room are not cut by a near wall.
- **A handle and the gizmo at one place.** The corner handle of an area and the floor square of the gizmo; the aim handle of a lamp that points straight down is under the lamp.
- **A doorway at the end of its wall.** A drag past the end of the wall, and to a width or a height of 0.
- **Undo.** A drag of a new handle is one step.
