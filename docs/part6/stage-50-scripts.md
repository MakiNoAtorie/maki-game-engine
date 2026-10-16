# Stage 50: script components with hooks, commands as functions, "New script", the log panel

Plan: [README.md](README.md), 2.1, 2.4, 2.7, 2.12 (item 29), 2.14. Report: `docs/part6/stage-50.md`. Needs: stage 48 (objects, the registry of component types), stage 44 (the reload of the script DLL, "Open in editor").

Goal: the Rust code of a project gives behavior to objects. A script is a component with hooks; it calls the registered event commands as functions. The editor makes a new script from a template, and shows the log lines of the game and of the scripts.
Gate: an example script of `game/scripts` uses each hook, in the editor and in the game program. A change of it loads in the editor with no restart, and the room keeps its values; a script DLL that does not build keeps them too. The game plays as before: the trip run passes, the benchmark against `s49` shows the same pictures.

State: written by the session from 2.1, 2.4, 2.7, 2.12, 2.14, and issues #8 to #11. Not seen by the owner.

## Tickets

| Ticket | Content |
|---|---|
| #8 | The trait `Script` (hooks start, step, interact, enter area, leave area), `add_script`, one system for each registered type, the component `area`, the object entity. An example script |
| #9 | A script calls a registered command (`ScriptContext::call`): the handler of the registry, the same as for a row |
| #10 | "New script": a name and a template, the file in the script crate, its registration, the file in the code editor. The templates of 2.7 that need only the object model, and a test that builds each |
| #11 | The log panel: each log line, filters by level and source, search, equal lines as one with a count, empty at Play, a click on a line of a script opens the file at its line |

## The way

### The object entity (#8)

- `ComponentType` gets `insert`: `None` for the types of now (their spawn code stays in the room loader), `Some` for a type that is a component of `bevy_ecs` (`add_object_component`): it reads the text and inserts the value.
- An object with such a component has one entity for them, "the object entity": the entity of its prop with a generated mesh, or of its light, else a new one. It has `RoomEntity`, `RoomItem`, the name, and `Interpolated` at the pose of the object. So a script that moves its object moves its mesh, and a script on a light changes that light.

### The hooks (#8)

```rust
#[derive(Component, Default, Serialize, Deserialize, Fields)]
#[serde(default)]
struct Wobble { speed: f32 }

impl Script for Wobble {
    fn step(&mut self, context: &mut ScriptContext) { ... }
}
add_script::<Wobble>(app, "wobble");
```

- The trait is in `maki-events`: the hooks interact and enter area are the triggers of the events, with the same area test and the same interact reach; #12 makes the events components on these hooks.
- One exclusive system for each type, in `FixedUpdate` after the events. The script value is out of its entity while a hook runs (`std::mem::take`; the trait needs `Default`), so a hook has `&mut self` and the whole world (`ScriptContext`: the world, the entity, the helpers for the pose, the input actions, the flags).
- `start`: the first step of the entity. `step`: each fixed step. `interact`: the player faces the area of the object (the component `area`: a size along X and Z, the center at the pose of the object) and presses interact, and no event took that press. `enter_area` and `leave_area`: the player walks in and out. As with the events: no enter in the first step, no enter and no interact while an event blocks the player.
- The play state: the room loads again at Play and at Stop in the editor, so each script starts again. The flags and variables are the store of rows and scripts.

### Commands as functions (#9)

- `context.call(value)`: the registry makes the command (`CommandRegistry::command`), its first step runs at once with the world; a command that waits (a message) goes on as a running event with the name of the object, which blocks the player as an event does.

### "New script" (#10)

- A command of the editor (File menu): a small panel with a name and a template list; "Create" writes `src/<name>.rs` into the script crate and opens it in the code editor (the setting of stage 44).
- A template is a `.rs` file in `crates/editor/templates/`, with `ScriptName` and `script_name` in the places of the name. A folder `templates/` of the project adds its own (the same name of a file takes the place of the engine's).
- The registration: the template has `pub fn add(app: &mut App)`. The editor adds `mod <name>;` and `<name>::add(app);` to `src/lib.rs` of the script crate. If it does not find the place, the file stays and the status row says what to add.
- A test of `maki-engine` builds each template as a module.

### The log panel (#11)

- A `tracing` layer of the log setup of `maki-engine` keeps the lines in a store (`maki_ui::LogLines`, a ring of lines). The panel is a tool panel of `maki-devtools` (`add_tool_panel`).
- A line: level, target, text, file and line. The panel: a box for each level, a source filter (the target starts with the text), search in the text, equal lines in a row as one with a count, a button "Clear". At Play the store is cleared.
- A click on a line with a file of the script crate opens it at its line in the code editor.

## Done when

- The gate.
- Tests: each hook runs at its time (start one time, step, interact only when the player faces the area, enter and leave); the script moves its object; an object with an unknown script keeps its text; a script calls a command of the registry; each template builds and "New script" writes and registers a file; the log store keeps lines, the filters and the count.
- Tests and clippy, static and dynamic; a validation run; the trip run; the reload test (`tools/reload_test.py`); the benchmark against `s49`; the symbol count of the engine DLL.

## What to watch for

1. **The symbol count** (58,506 of 65,535): the generic system of each script type is in the script DLL, not the engine DLL; the log panel and the layer are in the engine.
2. **The picture**: no scripted object in the rooms of the benchmark, but the example script; it goes into room C (not in a benchmark scenario), or the pictures change.
3. **Interact twice**: one press starts an event or a script, not both.
4. **A hook that despawns its entity**: the value does not go back, with no panic.
5. **The log store in the editor**: lines of each frame (a warning in a loop) must not grow the memory: a ring with a limit, and the count of equal lines.
