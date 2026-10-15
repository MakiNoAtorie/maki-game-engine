# Stage 49: prefabs with changes, and the player as data

Plan: [README.md](README.md), 2.1, 2.14, item 2 of 3.2. Report: `docs/part6/stage-49.md`. Needs: stage 48 (objects, the registry of component types).

Goal: a kind is a prefab file: objects with their components and default values. An object of a room that uses a prefab stores only the values that it changed; the others follow the file. The player is a prefab file, not code of its own.
Gate: a change of a prefab file shows in each instance that did not change that value. The inspector marks a changed value and resets it to the prefab. The game plays as before: the trip run passes, the benchmark against `s48` shows the same pictures.

State: written by the session from 2.1, 2.14, and issues #6 and #7. Not seen by the owner.

## Tickets

| Ticket | Content |
|---|---|
| #6 | Prefab files are lists of objects. An instance stores its changes as trees. Loader, make, unpack, and the inspector on instances |
| #7 | The player is a prefab file that the project file names. The room loader has no code for the player |

## The way

### The prefab file (#6)

```ron
(objects: [
    (name: "lamp", pose: (position: (0.0, 2.0, 0.0)), components: {"light": (radius: 0.1, color: (1.0, 0.9, 0.8), intensity: 8.0)}),
    (name: "shade", components: {"prop": (mesh: Cube(0.4))}),
])
```

- `PrefabDef` is `objects: Vec<ObjectDef>`, in the space of the prefab: the form of the room file. The lists `props` and `lights` go. No prefab file of the game is in the old form; the files of the tests are written again.
- Any object can be in a prefab, not only props and lights ("make prefab" takes each selected object).

### The instance (#6)

```ron
(name: "lamp 2", pose: (position: (3.0, 0.0, 1.0)), components: {"prefab": (file: "prefabs/lamp.prefab", changes: {"lamp/light": {"intensity": 3.0}})}),
```

- The component `prefab` has `file` and `changes`: a map from `object/component` (the name of the object in the prefab, or its index if it has no name) to a tree of the changed fields. A tree is the form of 2.14: the inspector, the undo, and the changes read the same tree.
- **Why trees and not text**: a change is a part of a value (one field). RON cannot read a part of a struct with no type, and a tree can be merged with no type. The tree of a field that is an enum is in its tagged form (`{"Sphere": {...}}`), as the tree of stage 47 has it.
- The objects that an instance shows: each object of the file, its pose in the space of the instance (`pose_in`), each changed component as its tree from the file with the changes merged in (through the registry), and back to text. A change of a type that no module registered, or one that does not read, is left out with a warning in the log; the value of the file shows.
- One function makes these objects (`maki_scene::instance_objects`); the loader spawns each of them as an object of the room, with the `RoomItem` of the instance. Unpack puts them into the room.

### The inspector (#6)

- An instance shows its own rows (name, pose, the file), then a group for each object of the file and in it a group for each component, from the merged tree.
- A changed field has a mark and a button "reset": the field goes out of `changes`. A change in a group writes the difference to the file's tree into `changes` (only the fields that differ; an empty map goes).
- Undo: the document is copied (stage 31); nothing more.

### The player (#7)

- `game/assets/rooms/prefabs/player.prefab`: one object with the component `player` (walk speed, run speed, radius) and `prop` (the mesh: a new `MeshDef::Capsule { radius, height }`, the mesh of now; or a file).
- The project file names it: `player = "prefabs/player.prefab"`, from the folder of the rooms. `PlayerModule` takes the path; the `[player]` table of the config goes.
- The player module spawns the player from the file. The room loader no longer moves the player: it writes the place of the spawn point into `CurrentRoom`, and the player module moves the player in the same frame.

## Done when

- The gate.
- Tests: a prefab file of objects reads and writes back; an instance with no change shows the objects of the file at its pose; a changed field of an instance stays when the file changes, and the other fields follow the file; reset removes a change; the inspector of an instance draws its groups with no change of the document; the player is spawned from its prefab file with the values of the file.
- Tests and clippy, static and dynamic; a validation run; the trip run; the benchmark against `s48`; the symbol count of the engine DLL.

## What to watch for

1. **The picture**: the player capsule must be the mesh of now (`Mesh::capsule(radius, height, 24, 8)`), and it is in each picture of the benchmark.
2. **The symbol count** (57,962 of 65,535).
3. **The timing of the player move** at a room change: the same frame as now, or the camera and the denoiser see one frame at the old place.
4. **A prefab inside a prefab**: not in this stage. An object with `prefab` in a prefab file is left out with a warning.
5. **The asset watch**: a changed prefab file loads the room again: the watch of stage 45 gets the prefab files of the room, as it has its model files. The player file too (#7).
