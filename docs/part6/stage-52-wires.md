# Stage 52: wires between objects, and "Send a signal"

Plan: [README.md](README.md), 2.1, 2.8 (the general command "Send a signal"), 2.14 ("a wire is as in Source"), item 3 of 3.2. Report: `docs/part6/stage-52.md`. Needs: stage 51 (events as components, the general commands).

Goal: "When this button is pressed, open that door", set in the editor with no code. A component names its outputs; an input is a registered command; a wire joins an output of an object to a list of commands.
Gate: a room has a button that opens a door by a wire, in the game and in the editor; the row "Send a signal" starts a wire. The game plays as before: the trip run passes, the benchmark against the stage before shows the same pictures.

State: written by the session from 2.1, 2.8, 2.14, item 3, and issue #14. Not seen by the owner.

## Tickets

| Ticket | Content |
|---|---|
| #14 | The outputs of a component, the component `wires`, the row "Send a signal", the wires in the editor (make, change, delete, undo), an example room |

## The way

### Outputs

- A component type names its outputs: `add_outputs(app, "area", &["entered", "left", "used"])`. They are in the command registry (as the script functions of stage 51), so the checks and the editor find them.
- The area (`TriggerArea`) has three: `entered` and `left` (the player walks in and out), `used` (the player faces the area and presses interact). The rules of the events hold: none in the first step of a room, none while an event blocks the player, one press starts one thing.
- A script names its outputs with `add_outputs` and sends one with `ScriptContext::signal("pressed")`.

### The component `wires`

```ron
(
    name: "button",
    pose: (position: (1.0, 0.0, -2.0)),
    components: {
        "area": (width: 0.6, depth: 0.6),
        "wires": (list: [(output: "used", commands: [OpenDoor("door")])]),
    },
)
```

- A wire: an output of the object, and a command list. An input is a registered command (`OpenDoor("door")`): no second set of inputs. A wire that starts runs as an event runs (it blocks the player while a command waits), with the name of its object.
- `wires` is read with the registry, as `event` is (`add_component_reader`).
- Each wire of an output starts when the output is sent, in the order of the list.

### "Send a signal"

- The row `Signal(object, output)` sends an output of another object: its wires start. The check: an object of that name, an output of one of its components.

### The editor

- The event panel shows the wires of the selected object beside its pages: a tab for each wire, the output picked from the outputs of the components of the object, and the command rows of the wire with the same widgets as a page. "Add a wire", "Delete". A change is a change of the document: undo.
- `Signal` has a picker of the object and of its outputs. A wrong name shows on the row.
- "Add > Button": an object with an area and a wire on `used`.

### The example room

- A new room `room_d.ron` (not in a benchmark scenario: the pictures of the other rooms stay): a door in a wall, a button (a small cube with an area) that opens it by a wire, and a lamp whose event sends the output `used` of the button with "Send a signal" when the player walks under it.

## Done when

- The gate.
- Tests: the area sends `entered`, `left`, `used` with the rules of the events; a wire on `used` opens a door; `Signal` starts the wires of another object; a script sends its own output; the check finds a wrong object or output of a wire and of `Signal`; the editor adds, changes, and deletes a wire with undo; the example room reads and its button opens the door.
- Tests and clippy, static and dynamic; a validation run; the trip run; the benchmark against the stage before; the symbol count of the engine DLL.

## What to watch for

1. **The pictures**: no wire in a room of the benchmark.
2. **One press, one thing**: an event, a script, and a wire on `used` of one area: the event first, as in stage 50.
3. **A loop of signals**: a wire that sends its own output runs again in the next step, not in the same one (no endless loop in a step).
4. **The symbol count** (60,011 of 65,535 after stage 51).
