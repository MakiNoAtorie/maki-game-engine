# Stage 27: viewport editing

Plan: [README.md](README.md). Report: `docs/part4/stage-27.md`.

Goal: a room is laid out with the mouse: select, move, rotate, scale, add, delete, duplicate, with undo.
Gate: a room can be laid out with the mouse alone.

## Context in the code

### Picking

- Camera rays cull back faces, so the camera looks into the room through the near walls. A pick ray that does not cull selects the near wall.
- Behind a mirror or glass, the guides of the denoiser (normal, distance, albedo, motion) are those of the virtual point, not of the mirror.
- An id for each pixel is a new output of the trace pass. It must cost nothing in a build with no editor.
- `Gpu::read_image` reads an image back (the benchmark dumps use it).
- Mesh data is on the CPU in `Assets<Mesh>`. Room A has about 11 000 triangles; there is no BVH on the CPU.
- A light is a sphere with a radius of 0.04 to 0.15 m in the rooms of the game, and the picture shows it. A cone light has a direction and a cone angle.
- An event area is a rectangle on the floor: `center` and `size` in X and Z, with no rotation. An `Autorun` or `Parallel` event can have no area.

### Items and references

| Item | Data | Referenced by |
|---|---|---|
| Prop | `PropDef`: name, mesh (`Cube`, `Cuboid`, `Sphere`, `File`), pose, look, collider | - |
| Light | `LightDef`: name, position, radius, color, intensity, on, direction, cone | `LightOn`, `LightOff`, `LightIntensity` by name |
| Doorway | `Doorway`: wall, offset, width, height, depth | `DoorDef.doorway` by index |
| Door | `DoorDef`: name, doorway, seconds, color, open | `OpenDoor`, `CloseDoor` by name |
| Mover | `MoverDef`: name, mesh, look, poses `a` and `b`, seconds | `OpenDoor`, `CloseDoor` by name |
| Spawn | name and position in a map | `Transfer(room, spawn)` from other room files; `start_spawn` of the config |
| Event | `EventDef`: name, area, pages | `Parallel` events are found by name |

- Names are optional, and nothing checks that a name is unique (`OPEN_ITEMS.md`, "Events with the same name").
- A door or light name that the room does not have gives a warning at run time, not an error at load.
- A delete of a doorway moves the index of each doorway after it.
- No room of the game has a mover that is not a door, or a prop from a model file (`OPEN_ITEMS.md`).
- The room box has its center at the origin; the floor is at Y 0.

### Drawing

- The UI pass draws into the `output` image after the tone curve, before the blit to the swapchain. A line that the egui painter draws is not in the path tracer, the denoiser, or TAA. It has no depth test.
- `GraphicsPipelineDesc` has one fixed state, used by the UI pass (`OPEN_ITEMS.md`, "Graphics pipeline state"). A line pass in the frame graph is the second state.
- The egui renderer draws no paint callbacks and no user textures. A gizmo crate must draw with plain egui shapes, and must support egui 0.36.

### Undo

- The document of stage 26 is plain data: `RoomDef` and the events are `Clone` and `PartialEq`, and a room file has less than 100 lines. A copy of the document for each undo step is cheap.

## Prompt to paste

```text
Stage 27: viewport editing.

Goal: a room is laid out with the mouse: select, move, rotate, scale, add,
delete, duplicate, with undo. Read docs/part4/stage-27-viewport-editing.md
first.

Picking
- Click selects the item under the cursor; shift-click adds to the
  selection; click on empty space clears it. The outliner and the viewport
  share one selection.
- Pick against what the picture shows: a GPU id per pixel read back a frame
  later (an editor-only output of the trace pass, with no cost in a build
  without the editor), or a CPU ray against instance bounds then triangles,
  with the back-face cull of the camera rays. Choose and justify. Lights,
  spawns, and event areas have no mesh or a very small one: they get pick
  shapes (a sphere with a least size, a marker, the area rectangle).

Gizmo
- Translate, rotate, scale gizmos for the selection, local and world space.
  Use a maintained crate if one fits egui 0.36, our camera, and a renderer
  that draws plain egui shapes only (for example transform-gizmo-egui);
  else write a small one.
- Snapping, on by default and toggled with a modifier key: a grid step for
  position, an angle step for rotation, set in the editor settings (a
  config table of the editor, stage 24). A room-maker game lives on a grid.
- A mover has two poses: the gizmo edits the one that is selected, and the
  other shows as a ghost or a marker.
- Room shape edits (width, depth, height, doorway wall, offset, and size)
  stay in the property panel, with the room preview updating live.

Edit operations
- Add: prop (generated shape or model file), light, spawn point, doorway,
  door on a doorway, mover, event area. New items appear in front of the
  editor camera on the floor.
- Delete, duplicate (with an offset), rename. A delete of a doorway fixes
  the doorway index of each door, and removes or warns about its door.
- Names stay unique where events refer to them (doors, movers, lights,
  spawns, events). Renaming an item that events reference updates the
  events or warns, say which. A spawn can be named by a Transfer in
  another room file: say what a rename does there.

Undo and redo
- Every change to the document is an edit that can be undone and redone,
  including changes from the property panel and from the gizmo. A gizmo drag
  or a slider drag is one undo step, not one per frame.
- The history clears on open, survives save, and does not include play mode.
- The document is small plain data: a copy for each step is acceptable if
  it is the simplest design. Say what you chose.

Overlays
- Draw in the viewport, editor only: selection outline or bounds, light
  radius and cone, spawn markers with names, event area rectangles on the
  floor with the event name, door hinge and swing, the two poses of a
  mover, room grid. Choose how to draw lines (egui painter with projected
  points, or a line pass in the frame graph) and justify it. Overlays do
  not enter the path tracer, the denoiser, or TAA.

Done when a room can be built from the empty-room template with the mouse
alone (walls from settings, then props, lights, door, mover, spawns, event
areas), saved, and played; every operation undoes and redoes; picking
selects what the picture shows, including through a doorway, through the
near walls, and behind glass where it makes sense; benchmarks keep their
numbers; validation is clean. Update OPEN_ITEMS.md ("Room file props from
files", "Movers of a room file", "Events with the same name", "Graphics
pipeline state" if a line pass was added). Stop and report.
```

## What to watch for

- **Picking the wrong thing.** Click a prop seen in the mirror ball of room B, an object behind the glass ball of room A, a prop seen through a near wall, and an object at the edge of a pixel. The report says what each case selects.
- **Undo granularity.** Drag a gizmo for two seconds, then undo one time. Everything goes back in one step.
- **Overlays in the denoiser.** An overlay drawn before the tone curve smears and ghosts. Overlays belong after the picture is done.
- **Name references.** Rename the door that `locked door` opens, then play. Either the event follows the rename or the editor gave a warning. Do the same for the spawn `west` of room B, which `room_a.ron` names.
- **Doorway indices.** Add a second doorway before the first, or delete one, and check that each door is still in its doorway.
- **ID output in the game.** If picking uses a GPU id image, a build without `editor` must have the same `gpu_trace` as before.
