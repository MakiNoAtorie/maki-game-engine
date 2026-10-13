# Stage 47: fields that describe themselves

Plan: [README.md](README.md), 2.1, 2.14, and item 1 of 3.2. Report: `docs/part6/stage-47.md`. Needs: stage 42 (the property grid).

Goal: one mark on a type says what its fields are, and the properties panel of the editor is drawn from that. No panel code by hand for an item kind.
Gate: the properties of each item kind of a room (room, doorway, door, prop, mover, light, prefab) come from the table of fields. The code of the panel has no name of a field of an item. A room file that the editor saves is the same text as before.

State: written by the session from 2.14, which the owner approved. Not seen by the owner.

## The way

The item kinds stay as they are (`PropDef`, `LightDef`, ...), and so does the room file. This stage makes the three parts that the object model stands on, and proves them on those kinds. Stage 48 changes the room file.

- **The derive `Fields`** (a proc-macro crate, `crates/macros`). On a struct: its fields. On an enum: its variants, each with its fields. It writes two functions: `kind()`, the table, and `tree()`, the value as a tree.
- **The tree** is `serde_json::Value` (the crate is in the engine already, by `gltf`). The derive writes the tree with each field in it, also one that the file leaves out because it has its default. `serde` reads a tree back into the type.
- **A field** has a name (the name in the file), a label, a help text (its doc comment), a kind, and a default.

| Kind | Rust type | Hints of `#[field(...)]` |
|---|---|---|
| Number | `f32`, `f64`, the integers | `range = 0.0..=1.0`, `step = 0.05` |
| Switch | `bool` | |
| Text | `String` | |
| Vector | `[f32; 3]` | `step` |
| Color | `[f32; 3]` with `color` | |
| File | `PathBuf` | `file = "model"`, `"picture"`, `"sound"`, `"prefab"` |
| Pick | a number or a text with `pick = "list"` | The tool gives the names of the list |
| Optional | `Option<T>` | `some = value`: the value when it is turned on |
| Group | a struct with the derive | |
| Choice | an enum with the derive | On a variant: `default = value` |

On each field: `label = "..."`, `default = value`, `skip`. On the type: `#[fields(default)]`: the default of each field is that of `Default` of the type.

- **The registry** (`FieldTypes`, a resource of the core crate): a module adds a type under a name. The scene module adds the item kinds. Stage 48 adds the function that builds a component.
- **The panel**: the editor takes the tree of the selected item, draws the rows from the table, and reads the changed tree back into the item. The widgets are those of stage 42.

Not in this stage: lists (`Vec<T>`) as a field (with the first component that has one), a field that shows by the value of another, the room file, the events panel.

## Prompt

```text
Stage 47: fields that describe themselves.
Read docs/part6/README.md (2.1, 2.14, 3.2 item 1) and
docs/part6/stage-47-fields.md first.

- `crates/macros`: the derive `Fields` for structs and enums, with the
  hints of the stage file. It works in an engine crate and in a script
  crate (which names `maki-engine` only).
- `maki_core::fields`: the tree, the table types, the trait, the
  registry.
- The derive on the item kinds of a room and the types in them. The
  labels, ranges, and steps are those of the panel of today.
- The properties panel of the editor from the table: one function for
  all item kinds. The rows by hand for these kinds go.
- The doc comment of a field is the hover text of its row.

Done when
- The gate.
- Tests: the derive (each kind, each hint, struct and enum forms); a
  value goes to its tree and back with no change, for each registered
  type; the derive in the script crate; the room files of the game are
  written with the same text as before.
- By hand: each item kind in the editor: a change of each kind of field,
  undo, and "back to default".
- Benchmark against `s46`, and the symbol count of the engine DLL.
```

## What to watch for

1. **Names in two places.** The table names a field as `serde` does. A rename of `serde` (`#[serde(rename)]`) that the derive does not see breaks the read. A test reads the tree of each registered type back.
2. **Numbers.** A tree has `f64`, an item has `f32`. A value that is shown and not changed must not change the file.
3. **A change in each frame.** The panel makes the tree of one item in each frame and reads it back only after a change. The room item must not copy the lists of the room.
4. **Where the derive finds the engine.** An engine crate names `maki-core`, a script crate names `maki-engine`. One derive for the two.
5. **What the panel of today does and a table cannot say**: the look of a prop shows only for a generated mesh; the direction of a light only with a cone; the doorway of a door is a list of the doorways of the room. Say for each what the stage does.
6. **The symbol count of the engine DLL** (56,599 of 65,535): the tree code is generic.
