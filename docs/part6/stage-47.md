# Stage 47: fields that describe themselves

State: done. Stage file: [stage-47-fields.md](stage-47-fields.md) (written by the session from 2.14 of the plan; the owner did not see it). Plan: `README.md`, 2.1, 2.14, item 1 of 3.2.

## 1. What was built

- **The derive `Fields`** (`crates/macros`, a proc-macro crate). On a struct or an enum it writes `maki_core::fields::Fields`: `kind()`, the table of the fields, and `tree()`, the value as a tree. The hints of a field are in `#[field(...)]`: `label`, `default`, `skip`, `range`, `step`, `color`, `file`, `pick`, `some`. `#[fields(default)]` on a struct takes the default of each field from `Default`. The doc comment of a field is its help text.
- **`maki_core::fields`**: the tree (`serde_json::Value`), the table types (`Kind`, `Field`, `Variant`, `Form`), the trait, its code for the types with no fields (numbers, `bool`, `String`, `PathBuf`, `[f32; 3]`, `Option<T>`), and the registry (`FieldTypes`, `add_field_type`): a module gives a type a name.
- **The item kinds of a room have the derive**: `PropDef`, `LightDef`, `DoorDef`, `MoverDef`, `PrefabUse`, `Doorway`, and the types in them (`PoseDef`, `LookDef`, `TextureDef`, `SoundDef`, `MeshDef`, `Material`, `Filter`, `Wall`). `RoomShell` is new: the fields of a room that are of the room itself (size, textures). The scene module gives each a name.
- **The inspector of the editor is drawn from the table** (`crates/editor/src/fields.rs`, 200 lines): one function for all kinds. The editor takes the tree of the selected item, draws it, and reads a changed tree back into the item with `serde`. The rows by hand for the seven kinds are gone (`item_properties` was 140 lines with each field by name; the widgets `texture`, `mesh`, `pose`, `look`, `name` of the editor: 130 lines).
- The hover text of a row is the doc comment of its field (`maki_ui::widgets::help`).

Picture: [a prop in the inspector: a variant with its fields, a group, optional values](pictures/s47_inspector.png).

## 2. Results

| Check | Result |
|---|---|
| The gate: each item kind from the table | Room, doorway, door, prop, mover, light, prefab: `item_properties` takes the tree and the table by the name of the type. One name of a field is left in the panel: `name`, to tell the events of a rename (see 3) |
| The gate: the same room file | `the_editor_writes_each_room_file_with_the_same_content` (script crate): each item of the three room files of the game goes to its tree and back; the room and the text of the file are the same |
| Test: the derive (`maki_core::fields::tests`, 5 tests) | Each kind and each hint; a struct, and the four forms of a variant; the default from `Default` and from the field; a skipped field; a value to its tree and back (also 0.15, which is not exact in binary); the zero tree of a type reads as the type; the registry; a hint on the wrong kind stops with its reason |
| Test: each registered type (`maki_scene`) | The tree of each of the seven kinds with nothing set reads as the kind (the table names the fields as `serde` does), and a thing with values goes to its tree and back |
| Test: the inspector (`maki_editor`) | A frame with no input draws the rows of each kind and changes no tree; the trees read back give the same document and the same text; a changed tree changes its item and no other; a tree of another kind is an error and changes nothing |
| Test: the derive in the script crate | `a_type_of_the_script_crate_describes_its_fields`: a struct of `game/scripts`, which names `maki-engine` only |
| By hand, in the editor | Prop: the mesh from "Cube" to "Sphere" (its three fields show with the values of the default), color and material turned on, "Metal" chosen (its roughness shows), a drag of the radius, a name typed (the outliner shows it); two steps of undo, each one field. Light: "back to default" of the volume of its sound (the button goes), the cone turned on, the hover text of a label. Door: its doorway shows as the name of the outliner. Mover (added): its two poses as groups. Saved: the file has these changes and no other value. Not by hand: a prefab (no room of the game has one), redo |
| `cargo test`, static; dynamic | 168 and 5 passed, 0 failed |
| `cargo clippy --all-targets`, static and dynamic | Clean |
| Validation | No error in the log of the two editor runs |
| Symbols of the engine DLL (watch 6) | 57,783 of 65,535 (stage 46: 56,599; +1,184). 11.8 % is left. See 5 |
| The game | `bench.py same trip_720p s46 s47`: ok (the same pictures). `gpu_total` against `s46`: -0.2 % to +1.0 % over the scenarios (the +1.0 % is `idle_1080p`, 0.102 to 0.103 ms); `trip_720p` 1.916 to 1.921 ms. The game has no code of the inspector |

## 3. Deviations from the stage file

- **One name of a field is in the panel**: `name`. An event finds a thing by its name, and the editor corrects the events when the name changes: the panel compares the `name` of the tree before and after. With objects (stage 48) the name is a part of the object, not of a component.
- **Three parts of an item have no table**: a spawn point (a name and a place in a map of the room), an event (its own panel, stage 51), and the switch "the gizmo changes the second pose" of a mover (a state of the editor).
- **What the panel of before did and a table does not say** (watch 5):
  - The look of a prop showed only for a generated mesh, and the radius of the lights only for a file. Now the two show always. With components (stage 48) a file and a generated shape are two components, each with its own fields.
  - The direction of a light showed only with a cone. Now the two are side by side; the label says "direction of the cone".
  - The doorway of a door: a field with `pick = "doorways"`. The editor gives the names of the list, as before.
- **Labels**: "light radius" (was "radius of its lights"), "cone (degrees to the edge)" (was "cone" with a row "degrees to the edge" in it; the number has no unit now).
- **"Back to default" is on more rows**: each field of a type with `Default` has it (a pose, a prop, a sound), not only the rows that had it by hand.
- **The rows are in the editor crate, not in `maki_ui`**: the row of a file needs the folder of the room and the converter table of the editor. A panel of a module that wants the rows gets them when the first one asks.
- **No list as a field** (`Vec<T>`): said in the stage file. The compiler names the hint to use (`skip`).

## 4. Decisions where the stage file was silent

- **The derive writes the tree itself; `serde` only reads it.** `serde` leaves a field out of a file when it has its default (`skip_serializing_if`), so a tree by `serde` would not have each field, and the inspector would have to know the default of each one. The tree of the derive has each field. The names and the forms are those of `serde`, and a test reads the tree of each registered type back.
- **`serde_json` is a dependency of the core crate.** It is in the engine by `gltf` (the same version, no second copy), and the plan (2.14) now names it. `syn`, `quote`, and `proc-macro2` are dependencies of the macro crate; the macros of `serde` and `bevy_ecs` have them.
- **Where the derive finds the core crate** (watch 4): it reads the manifest of the crate that is compiled. A crate that names `maki-core` gets `::maki_core`, another one `::maki_engine::core`. The macros of `bevy_ecs` do the same. The core crate names itself (`extern crate self`).
- **A hint on the wrong kind** (`range` on a text) stops the program at the start with its reason: the table is made when the type is registered.
- **An optional text** (`Option<String>`) is one text row: an empty field is no text.
- **A value that nothing gives** (`Kind::zero`): 0 (or the nearest end of the range), off, empty, white for a color, the first variant; a group has the default of each field.
- **A number with no sign** has a range from 0.
- **A name that is in use** in the registry gets the new type: a project can put its own type in the place of one of the engine.
- **The registry has a second function for each type** (`check`): read a tree and give the tree of what was read. The tests use it; stage 48 needs "build the component from the tree" at the same place.
- **The numbers of the inspector are `f32`** (watch 2): the widgets of stage 42 have `f32`, as the fields of the engine. A tree is read back only after a change, and an `f32` goes to `f64` and back with no change.
- **The room item** (watch 3): `RoomShell` has the five fields of the room itself, so the tree of "the room" does not copy the lists of the room.

## 5. Files

| File | Role |
|---|---|
| `crates/macros/` (new) | The derive `Fields` |
| `crates/core/src/fields.rs` (new) | The tree, the table types, the trait, the registry; the tests of the derive |
| `crates/core/src/lib.rs`, `Cargo.toml` | The module, the names `Fields` (trait and derive), `extern crate self`; `maki-macros`, `serde_json` |
| `crates/assets/src/mesh.rs` | The derive on `Material`, `Wall`, `Doorway` |
| `crates/scene/src/room.rs`, `lib.rs` | The derive on the item kinds, with the labels, ranges, and steps of the panel of before; `RoomShell`; the names of the kinds; a test |
| `crates/ui/src/widgets.rs` | `help`: the hover text of the next row |
| `crates/editor/src/fields.rs` (new) | The rows of a tree from its table |
| `crates/editor/src/document.rs` | `tree` and `set_tree` of an item |
| `crates/editor/src/panels.rs`, `widgets.rs` | `item_properties` from the table; the rows by hand are gone |
| `crates/editor/src/tests.rs`, `game/scripts/src/lib.rs` | Tests |
| `Cargo.toml` | `maki-macros`, `serde_json`, `syn`, `quote`, `proc-macro2` |
| `README.md`, `CLAUDE.md`, `OPEN_ITEMS.md`, `docs/part6/README.md`, `docs/part6/HANDOVER.md` | The inspector and the crates; the workspace; one row; the stage table; the state |

## 6. Uncertain

- **The symbol limit of the engine DLL.** This stage added 1,184 symbols; 7,752 are left. The engine crates are built at `opt-level` 0 in the dynamic build and export each generic function that they make; the code of `serde` for a type that is read from a tree is such code. Stages 48 to 50 add types. If the count comes near the limit: build the engine crates at `opt-level` 2 in the profile `dynamic` (slower builds of the engine, not of a script), or cut the exports. Not measured.
- **A component with no code, in the file** (2.14, for stage 48). A room file is RON, and RON needs the type to read a value: `Cube(1.0)` has no meaning with no `MeshDef`. A component of a script DLL that did not build cannot become a tree. Stage 48 must choose: keep the text of such a component as it is and write it back, or a form of the file that reads with no type. This stage does not decide it and does not block it.
- **The hover text** is the doc comment as it is written for the code: it has the marks of Markdown and names of Rust (`None`).
- **Whole numbers** go through an `f32` in the inspector: exact up to 16,777,216.
- **Two rows with the same label in one group** would share the state of their widgets (egui makes the id of a text field and of a list from the label). No type of now has that.
