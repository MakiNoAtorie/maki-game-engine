# Coding standards

Judgement rules for the review of a diff. The architecture rules are in `CLAUDE.md`; format and clippy are checked by `tools/check.sh`. Each rule: what to look for, and why.

## The picture and the renderer

- **Write only a different value** to a component or resource that the renderer or a change detection reads (`Transform`, `Light`, `Interpolated`, `Camera`, a resource of a module): `set_if_neq`, or a compare before the write. A write is a change: the denoiser drops its history and the benchmark picture changes.
- **No new component on an entity of a light or a mesh of a room** for state that the renderer does not need. A new archetype changes the order of the lights for the renderer, so the noise of the picture of a room at rest. Script state goes on an object of its own.
- **A room file change keeps the benchmark pictures**: an object that moves or changes by itself is not in a scenario room, and never on the first light of a room (the benchmark moves it). The check: `bench.py same` (`docs/CONVENTIONS.md`, "Picture gate").

## The schedule

- **A system of `FixedUpdate` that has the whole world** (exclusive) needs an order against `run_events`, and so does each system that runs beside it: bevy refuses an ambiguous order at the start of the app. Prefer a hook of an existing exclusive system (`ScriptRunners`) over a new one.
- **Play state goes back on `ResetPlay`**: a module with state that a game leaves behind adds its own system there.

## Data and scripts

- **A component of a project keeps its text** when no module reads it: no code path drops or rewrites a `ComponentText` that it cannot read.
- **A new name of a component type** must not be one of the engine (`prop`, `light`, `area`, ...): the registry takes the last one, and each room reads the new type.
- **A script hook leaves its value** in the entity: code that takes a component out of an entity puts it back, and a despawned entity is not an error.
- **A command that names an object follows a rename**: each argument that is the name of an object (a light, a door, an object of `SetField` or `Signal`) has its arm in `Document::renamed` (`crates/editor/src/ops.rs`), with a test.

## The editor

- **A panel that shows a value writes it only on an edit of the user**: no default, no "first of the list" goes into the document while the panel only draws. A write makes the document dirty and a step of the history.

## Tests

- **A new test room is built by code**: `maki_scene::room_text` and `maki_events::short_events`, not a new literal room text outside the tests of the format (`crates/scene/src/room.rs`). A change of the format then changes one function, not each test.

## Comments

- One line, objective: a constraint and its reason, in Simplified Technical English. The same reason at more than two places is a rule for this file, with one short comment at each place.
