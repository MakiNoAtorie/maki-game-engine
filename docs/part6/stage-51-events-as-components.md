# Stage 51: the events as components, and the general commands

Plan: [README.md](README.md), 2.1, 2.8, 2.14. Report: `docs/part6/stage-51.md`. Needs: stage 50 (script components with hooks, commands as functions).

Goal: one system, two ways to use it. An event is a component of an object, as a script is: its area is the area of the object, its triggers are the hooks. A row reaches any component of an object and the Rust code of the project.
Gate: each event of the three rooms works as before (the trip run passes); the benchmark against `s50` shows the same pictures. A row "Set a field" and a row "Call a script function" work in the game and in the editor, with undo and the problems on the row.

State: written by the session from 2.1, 2.8, 2.14, and issues #12 and #13. Not seen by the owner. "Send a signal", the third general command of 2.8, needs the wires: stage 52 (#14).

## Tickets

| Ticket | Content |
|---|---|
| #12 | The component `event` (the pages), the area of the object for its triggers, the interpreter on the entities, one store of flags and variables, the migration of the rooms, the editor on objects only |
| #13 | The rows "Set a field" (any registered component, a picker from its table of fields) and "Call a script function" (a function of the script crate), with undo and problems |

## The way

### The event component (#12)

```ron
(
    name: "locked door",
    pose: (position: (3.0, 0.0, -3.5)),
    components: {
        "area": (width: 1.0, depth: 0.8),
        "event": (pages: [(trigger: Interact, commands: [Message("Locked.")])]),
    },
),
```

- The component `event` (`EventPages`) has the pages only. The name of the event is the name of the object; its area is the component `area` of the object (stage 50) at the pose of the object. An event with no `area` has no place (`Autorun`, `Parallel`). The list `events` of the room file goes: no second list, no second kind of area.
- The commands of a page are an open set, read with the registry (`CommandRegistry`). So the type `event` reads its text through the registry: the room loader queues the read on the object entity, with the world (the registry is a resource).
- `CommandRegistry::parse_events` reads the events from the objects of a room file, with the line of each command for the checks, as before. `EventDef` is the event as the checks and the tools see it: name, object index, area, pages.
- The migration: each event of `room_a.ron`, `room_b.ron`, `room_c.ron` becomes an object of its own, with the name of the event, the center of its area as its position, and its size as `area`.

### The interpreter on the entities (#12)

- In each step `run_events` takes the entities with `EventPages`, in the order of the objects in the file (`RoomItem`), with the area at the current pose (`Interpolated`). A script that moves its object moves the area of its event.
- What the runner kept for each event (the player was inside; the autorun page) is a component on the entity. A running event knows its entity: a `Parallel` event is found by it, not by name.
- The rules stay: none in the first step of a room, none while an event blocks, one press starts one thing (an event before a script).

### One store (#12)

- The flags and variables are `GameState`: rows read and write it, scripts by `ScriptContext::state`, and the save file holds it. A test: a script sets a flag, a page with the condition starts, the save has the flag.

### The editor on objects only (#12)

- `Item::Event` goes. The outliner lists the objects; the property panel of an object with an `event` shows "Pages and commands: the panel Events"; the event panel edits the pages of the selected object. The area is the component `area` (the inspector) and the pose of the object (the gizmo).
- The overlay draws the area of each object with `area` (events and scripts); a click on it selects the object; the gizmo changes its size.
- "Add > Event" adds an object with `area` and `event`. The problems of the events point at their object.

### The general commands (#13)

- `SetField(object, component, field, value)`: the value is a tree (`serde_json::Value`, the form of the table of fields). The handler finds the entity of the object by name, reads its component of `bevy_ecs` as a tree, writes the field, and reads it back (`ComponentType` gets `get` and `set` for components of `bevy_ecs`: from the derive, no code for each type). The check: an object of that name with that component, a field of that name in its table.
- `CallScript(name)`: a function of the script crate that a module registered (`add_script_function(app, "name", fn(&mut ScriptContext))`), called with the context of the object of the event. The check: a function of that name.
- The editor: an argument editor for each: `SetField` picks the object, then its component, then a field from the table of fields, and edits the value with the widget of its kind; `CallScript` picks from the registered functions. A change is a change of the document: undo as for each row. A wrong name shows on the row, as for each command.

## Done when

- The gate.
- Tests: the events of a room file are read from its objects, with the line of a bad command; an event on an object with a script, one area; an event starts on the area of its object after a script moved it; a script and a row share a flag, and the save has it; "Set a field" changes a field of a light and of a script, and its check finds a wrong object, component, or field; "Call a script function" runs the function with the object of the event; the editor opens, changes, and writes an event on an object; the migrated rooms read as before (a test that counts the events and pages of each room).
- Tests and clippy, static and dynamic; a validation run; the trip run; the benchmark against `s50`; the symbol count of the engine DLL.

## What to watch for

1. **The picture**: the new objects of the events have no mesh and no light. The benchmark pictures must stay byte-equal.
2. **The order of the events**: two events on one press; the first in the file wins, as before.
3. **Undo of the editor**: the pages are text in a component; an edit of a page is one change of the document.
4. **The symbol count** (58,948 of 65,535).
5. **The trip run** walks to the events by their names (`devtools` bench script): the names stay.
