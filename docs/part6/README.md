# Part 6: plan (draft)

State: in discussion with the owner. Stage 41 (the spike) is done. This file holds what is decided.

## 1. Direction

The engine is a general engine in which a game in the style of RPG Maker can be made. It is not a copy of the RPG Maker workflow. Parts 4 and 5 made the event (pages, triggers, command rows) the only way to write logic, and the item kinds of a room a closed set. Part 6 corrects the two.

## 2. Decided

### 2.1 Object model (approved 2026-10-12)

The model of engines made for one game (typed objects, placed and tuned in the editor), not free composition as in Unity, and no node tree as in Godot.

- An object is a name, a pose, and a list of components.
- A component is a Rust type that a module registers, as a module registers its event commands today.
- A room file lists objects. The lists `props`, `lights`, `doors`, and the others go.
- A kind is a prefab: "player", "door", "lamp" are prefab files with their components and default values. The player is data, not a special case.
- A script is one more component, with hooks: start, step, interact, enter area.
- One set of commands: each registered event command is also a function that a script calls. Events and the command rows of the editor stay, as a shortcut over the same functions.
- Objects are flat, with an optional parent.
- The properties panel shows the components of the selected object, each from its registered type.

Cost: a new room file format with a migration of the rooms, and the item kinds of the editor (outliner, properties, pick, gizmo) made generic.

### 2.2 Editor and game are two things (owner, 2026-10-12)

As in Unreal and Godot: the editor is a program of its own that opens a project. It is not a mode of the game. Scripts are Rust; a script change must not need the editor to close. Rule 8 of `CLAUDE.md` gets new words.

### 2.3 How the editor gets the Rust code of a project (proven in stage 41)

- `maki-editor.exe`: engine and editor, no code of a game.
- A project: a folder with `assets/` and one Rust crate of scripts (components, behaviours, commands of that game).
- Development: the engine crates build as one DLL (`maki-engine`), the script crate as a DLL against it, with the profile `dynamic`. The editor loads a copy of the script DLL. After a change it builds the DLL (1 s) and loads the new copy when no game runs, into a new app. An old copy is not unloaded.
- Export: one program with the engine and the script crate linked statically, no editor.
- A change of an engine crate needs a new start of the editor: the engine DLL is in use.

Report and limits: `stage-41.md`.

## 3. Open

### 3.1 Next

- The editor as a program of its own, and the game as a project (the `game/` package becomes data and a script crate).
- The reload in the editor: a new app, with the window and the GPU device of the old one.
- The object model of 2.1.

### 3.2 Build times

Measured 2026-10-12, debug build, `cargo build -p maki-game` after `touch` of one file, 3 runs each:

| File touched | No editor | `--features editor` |
|---|---|---|
| Nothing | 0.3 s | 0.3 s |
| `game/src/main.rs` | 2.5 to 2.7 s | 2.7 to 3.3 s |
| `crates/gameplay/src/lib.rs` | 3.4 s | 3.9 to 4.0 s |
| `crates/events/src/lib.rs` | 4.1 to 4.3 s | 4.8 to 5.3 s |

Not in these numbers: the start of the game after the build, and a real edit (a `touch` changes no text, so the compiler reuses more).
