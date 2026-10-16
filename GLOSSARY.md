# maki-game-engine

A general 3D game engine with path-traced lighting. Its first use is a game in the style of RPG Maker, made with the workflow of Unity and Godot.

## Planning

**Vertical slice**:
A small, complete game of the owner that the plan is made from: each stage gives a feature that the slice needs, built as a general seam of the engine.
_Avoid_: Demo, prototype

## Logic and data

**Object id**:
The identity of an object of a room, which a rename or a move does not change.
_Avoid_: Object name (a name is for people)

**Object state**:
The named switches and numbers that belong to one object of one room, and that stay when the player leaves the room.
_Avoid_: Self switch, local flag

**Routine**:
A command list in a file of its own, with arguments, that events and scripts call.
_Avoid_: Common event, shared list

**Data asset**:
A file of a type that a module registers, edited in the inspector, such as an item kind or a text table.
_Avoid_: Database, ScriptableObject, resource (a `bevy_ecs` resource is another thing)

## Animation

**Step mode**:
The mode of a track or clip with no interpolation: each key holds its value until the next key, and a clip can play at a lower frame rate of its own ("on twos", "on threes").
_Avoid_: Stepped interpolation, limited animation
