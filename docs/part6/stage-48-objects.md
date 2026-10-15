# Stage 48: the room file is a list of objects

Plan: [README.md](README.md), 2.1, 2.14. Report: `docs/part6/stage-48.md`. Needs: stage 47 (the table of fields, the registry `FieldTypes`).

Goal: a room file lists objects. An object is a name, a pose, an optional parent, and components; a component is a name and a tree of values. The lists `props`, `lights`, `doors`, `movers`, `prefabs` go.
Gate: the three rooms of the game are lists of objects, migrated by code. The outliner, the pick, the gizmo, and the inspector work on any object. A room file that the editor saves is the same text as the file it read. The benchmark against `s47` shows the same pictures.

State: written by the session from 2.1 and 2.14, which the owner approved. Not seen by the owner.

## Tickets

The stage is five tickets (GitHub issues #1 to #5). Each leaves the game as it runs. The old lists stay until #5: expand, move, then delete.

| Ticket | Content |
|---|---|
| #1 | The list `objects` beside the old lists. The registry of component types. Props are the first kind: the props of the three rooms move into the list, and "add" of the editor makes an object. Outliner, pick, gizmo, inspector on objects |
| #2 | Lights as a component |
| #3 | Doors, doorways, movers as components (a door is a component of the object of its doorway) |
| #4 | Prefab uses and the rest (spawn points, the ops of the editor: make and unpack a prefab) |
| #5 | The old lists, their item kinds in the editor, and `RoomItem` variants go. Report `stage-48.md` |

## The way

### The file

```ron
objects: [
    (name: "cabinet", pose: (position: (-3.2, 0.0, -3.7)), components: {"prop": (mesh: File("models/cabinet.glb"))}),
    (name: "lid", parent: "cabinet", components: {"prop": (mesh: Cube(0.5))}),
],
```

- `name` and `parent` are optional. `pose` has the default of `PoseDef`. `components` is a map from the name of a component type to its value, in the order of the names.
- A component value is RON in the form of its type, as the old lists had it (`Cube(1.0)`, not a map). A person reads and writes it as before.

### A component with no code

RON needs the type to read a value: `Cube(1.0)` has no meaning without `MeshDef`. So the file keeps the form of the type, and a component is kept as **its RON text** (`ron::value::RawValue` when the room is read). The decision:

- **The text is kept as is.** A room reads with no type for any component. The text of each component is made compact when it is read (comments and white space out of it, but in strings): a comment in a component belongs to its object, as a comment in an item does now (`Notes`).
- The game and the editor read a text into its type only where they use it. A component whose type no module registered is written back as its text, with no change. The inspector shows it as text, and does not change it.
- Not chosen: a form of the file that reads with no type (a map for each variant). It makes the file longer and unlike a file that a person writes, and gains nothing in this stage: the inspector needs the type for its table anyway.

The document of the editor holds the room with these texts. The tree of a component (2.14) is made from its text through the registry when a tool needs it (the inspector), and a changed tree goes back to text through the registry. The undo keeps copies of the document (stage 31), so it needs no tree.

### The registry of component types

`maki_scene::add_component_type::<T>(app, name)`: the name, the table of fields (`FieldTypes` of stage 47, under the same name), and the two functions of the text: text to tree (read the RON as `T`, give its tree), tree to text (read the tree as `T`, write compact RON). A type has `Fields`, `Serialize`, and `Deserialize`.

The game builds a component of the scene from its text, as the loader of the room spawns an item now. A component that is one `bevy_ecs` component by itself (a script, stage 50) gets a generic "insert" in the registry with its first use.

### Props as the first kind

- The component `prop` (`maki_scene::Prop`): mesh, look, collider, light radius: the fields of `PropDef` without name and pose, which are of the object.
- The loader spawns each object with a `prop` as it spawns a prop, with `RoomItem::Object(index)`. An object with no component of the scene shows nothing.
- The pose of an object is in the space of the room. A parent is a name, written and read; the pose in the space of the parent comes with its first use (prefab instances, stage 49).

### The editor

- `Item::Object(index)`. The outliner has a list "Objects": the name, else the kind of its first component and the index.
- Pick and highlight: the meshes with `RoomItem::Object`. The gizmo changes the pose of the object.
- The inspector: the rows of the object (name, pose, parent) from the table of `ObjectDef`, then a group for each component from its registered table. A component with no type: its name, "no module registered this type", and its text.
- The name of an object is the name that events use (a rename corrects the events, as for an item).

### The migration

A function moves the props of a room into its objects, with their comments. A test that is run by hand (`--ignored`) runs it on the room files of the game and saves them. Ticket #5 deletes it with the old lists.

## Done when

- The gate.
- Tests: the file form of an object, and a room read and written back with the same text; a component with no registered type keeps its text through a load and a save; the props of the three rooms are objects; the inspector reads each component tree and writes it back with no change.
- By hand: the editor on room A: an object picked in the view and in the outliner, moved by the gizmo, a field of its `prop` changed, undo, save.
- Tests and clippy, static and dynamic; a validation run; the benchmark against `s47`; the symbol count of the engine DLL.

## What to watch for

1. **The symbol count of the engine DLL** (57,783 of 65,535). Each component type adds the RON and `serde_json` code of its type. Count after #1. The way out that is not measured: `opt-level = 2` for the engine crates in the profile `dynamic`.
2. **Notes.** A comment of a prop must stay with its object through the migration and each save.
3. **The same file.** The editor writes a component text as it was read; a text that a person wrote with `1` in place of `1.0` stays so until the inspector changes it.
4. **The patch of the room** (`patch_room`): a move of an object with a generated mesh does not load the room again, as for a prop.
5. **Two lists for one thing** while the old lists stay (#1 to #4): a name of an object and of an item may be the same. The unique names of the editor look in both.
