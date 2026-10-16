# Objects are named by a fixed id, not by their name

Status: accepted (owner, 2026-10-16)

## Context

Up to stage 53 an object of a room has an optional name, and each reference to an object (the parent, the target of an event, a wire, a script search) is that name. Object state (README 2.15) must stay over a change of room and go into the save, so it needs a key. A key that is the name breaks on a rename: the wires of the object stop, and its saved state is lost with no error.

## Decision

Each object has an id that the editor writes at its creation. A new name, a move, or a change of its components does not change the id; a copy gets a new one. Object state, wires, the targets of events, and the parent name the id. The name stays for people and for a search by name in a script. The rooms get ids by the migration of stage 48. Files get an identity in the same stage (item 12), the same way.

## Considered

- Names that must be unique in a room: no new field, but a rename still breaks each reference and the saved state.

## Consequences

- The room file has one more field for each object; the editor shows names and stores ids.
- A save file names objects by room and id: a room edited after a save keeps its state for each object that is still there.
