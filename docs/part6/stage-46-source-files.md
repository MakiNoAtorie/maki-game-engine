# Stage 46: source files of other programs

Plan: [README.md](README.md), 2.5. Report: `docs/part6/stage-46.md`. Needs: stage 45.

Goal: the owner saves a `.blend`, `.obj`, or `.psd` file in the asset folder of the project, and the editor shows the result. The engine still reads only glTF, PNG, and JPEG.
Gate: a cube is changed and saved in Blender, and the prop in the open room has the new form with no other step. The same for a `.psd` file as a texture. The game that ships has no converter and needs no Blender.

State: proposal, not approved.

## The way

The editor converts; the loader does not change.

- A source file has a result file next to it, with the same name: `chair.blend` gives `chair.glb`, `wall.psd` gives `wall.png`. A room names the result file. The result file is a part of the project (it is committed and exported), so the game and a machine with no Blender need nothing.
- The editor watches the source files of the asset folder (the watch of stage 45). A source that is newer than its result is converted. The new result file is a changed asset: stage 45 does the rest.
- A converter is a row of a table: extensions, and what makes the result.

| Source | Result | Converter | Needs |
|---|---|---|---|
| `.blend` | `.glb` | Blender with no window: open the file, export glTF | Blender; its path is a setting of the editor |
| `.obj` (with `.mtl`) | `.glb` | The same, with the OBJ import of Blender | Blender |
| `.psd` | `.png` | The flat picture of the file, in the editor | A crate that reads PSD: choose one and check its current version and API first |

## Prompt

```text
Stage 46: source files of other programs.
Read docs/part6/README.md and docs/part6/stage-46-source-files.md first.

- A table of converters in the editor crate: source extensions, result
  extension, and the function that writes the result.
- `.blend` and `.obj`: Blender as a child process with no window, on a
  thread. `[editor] blender` has its path; with none, the editor looks
  at the usual places, and says in the status row what is missing.
- `.psd`: the flat picture, written as PNG.
- A source that is newer than its result is converted: at the start of
  the editor, and when the watch sees a change. The result is written
  to a temporary name and then renamed.
- The file lists of the editor show a result file, and mark one that
  has a source. "Add > Model file" takes a source file too: it is
  converted, and the room names the result.
- An error of a converter: the status row, with the output of the
  program in the log. The old result stays.

Done when
- Each line of the gate, by hand with Blender and a paint program.
- A test with a converter that code gives (no Blender in a test): the
  order of the steps, the temporary name, the error path.
- The time from the save in Blender to the new picture, in the report.
```

## What to watch for

1. **The start time of Blender** (some seconds): the editor must not wait in a frame. Two saves in a short time: one conversion at a time for a file, and the last save wins.
2. **What the export of Blender writes**: the up axis, the unit, modifiers applied or not, lights and cameras of the file. The loader reads the lights of a glTF file (`KHR_lights_punctual`): decide if the export has them.
3. **Textures of a `.blend` file**: in the `.glb` (one file) or as files next to it. One rule.
4. **A result file that a person also edits**: the next conversion overwrites it. The editor must not convert when the result is newer.
5. **Layers of a `.psd` file** that the flat picture does not have (a file saved with no "maximize compatibility"): say what the crate does then.
6. **The symbol count of the engine DLL** with the new crate (stage 41, section 3).
7. **Git**: a result file is binary and changes with each save of the source. Say it in the README; the owner decides what is committed.
