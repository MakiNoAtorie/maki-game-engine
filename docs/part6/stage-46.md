# Stage 46: source files of other programs

State: done. Stage file: [stage-46-source-files.md](stage-46-source-files.md) (a proposal that the owner did not change: taken as given). Plan: `README.md`, 2.5.

## 1. What was built

- **A table of converters** in the editor crate (`sources.rs`, `Converters`): the extensions of the source files, the extension of the result, and the function that writes the result. A module adds a row with `add_converter`.
- **Three rows**: `.blend` to `.glb` and `.obj` (with its `.mtl`) to `.glb`, by Blender as a child process with no window; `.psd` to `.png`, the flat picture, in the editor.
- **One thread** looks at the folder of the room files (and the folders in it) 2 times in a second, and converts each source that is newer than its result or has none: at the start of the editor, and after each save. The result is written under a temporary name (`chair.tmp.glb`) and then gets its name. The new result is a changed asset file: stage 45 does the rest.
- **Blender**: `[editor] blender` has its program file; with none, the editor takes `blender` of the path of the system, then the newest one in the usual folder. A `.blend` or `.obj` file with no Blender gives a message that names the setting.
- **The file lists** mark a result file that has a source (the hover text names it), and show the result name of a source that is not converted yet: "Add > Model file" and each file field take it, and the room names the result.
- **An error of a converter**: a notice and one line in the status row; the log has the output of the program. The old result stays, and the source is not tried again until it is saved again.
- The game has no converter: all of it is in the editor crate.

Picture: [the check room: a model from a `.blend` file, a wall texture from a `.psd` file of Photoshop](pictures/s46_blend_and_psd.png).

## 2. Results

| Check | Result |
|---|---|
| The gate: a cube is changed and saved in Blender | Blender 5.2 (run by a script, with no window) scaled the cube of `box.blend` and saved it. The prop of the open room has the new form with no other step: 2.74 s from the save to the load of the new model (0.38 s until the thread saw the file, 1.66 s Blender, 0.70 s the watch of stage 45) |
| The gate: a `.psd` file as a texture | A file that Photoshop 26.6 saved, with two layers (packed, 17 kB of resources, 3 kB of layers): the walls have its picture 0.97 s after the save. The picture is the same as the one that Pillow reads from the file |
| At the start of the editor | `box.blend` and `wall.psd` with no result: converted (Blender: 2.74 s for the first run), and the room, which named the two results before they were there, shows them |
| `.obj` with `.mtl` | A box written by hand: `crate.glb` in 1.79 s, with the color of the `.mtl` file and +Y up |
| A file that is not a `.blend` file | "broken.blend was not converted: Blender ended with an error (exit code: 1)"; the log has "File format is not supported". No result file, no second try |
| Test: the converter that code gives (`sources::tests`) | The steps in their order; the temporary name beside the source; no temporary file is left, also after an error; the old result stays after an error; no second try until the source is saved again; a result that is newer than its source stays; a save while the converter runs is converted again |
| Test: the flat picture of a `.psd` file | RGB and gray, 8 and 16 bits, packed and not packed, with one channel more; what is not read gives its reason |
| `cargo test`, static; dynamic | 160 and 5 passed, 0 failed |
| `cargo clippy --all-targets`, static and dynamic | Clean |
| Symbols of the engine DLL (watch 6) | 56,599 of 65,535 (stage 43: 54,858). The editor crate now has the PNG writer of the `image` crate. 14 % is left |
| The game | `bench.py same trip_720p s45 s46`: ok (the same pictures). `gpu_total` against `s45`: -1.0 % to +0.3 % over the scenarios; `trip_720p` 1.917 to 1.916 ms |

## 3. What Blender writes (watch 2 and 3)

- The modifiers are applied: the room shows what Blender shows.
- +Y is up, as in the engine; one unit is one meter.
- No camera and no light of the Blender file: a room has its own lights. (The loader would read the lights of a glTF file.)
- The images are in the `.glb` file: one result file for one source.
- The scale and the turn of an object are on the node of the glTF file; the loader takes them.
- Blender runs with `--factory-startup`: no add-on and no setting of the user changes the result.

## 4. Deviations from the stage file

- **No crate for the `.psd` file.** The stage file says "a crate that reads PSD: choose one". `zune-psd` 0.5.1 was the choice (its one dependency is in the tree), and it reads a picture that is not packed wrong (the loop of `decode_raw` ends after a third of the pixels). `psd` 0.3.5 reads the layers too and brings a second version of `thiserror`. The flat picture is the simple part of the format: the editor reads it with its own code (90 lines, `read_psd`), checked against Pillow with a file of Photoshop. The `image` crate, which the workspace has, writes the PNG file.
- **The converter thread looks at the folder itself**, not by the watch of stage 45: that watch knows the files that a store loaded, and a new source file is in no store. The rule is the same (the time of the file, and 0.3 s of rest).
- **The asset folder is the folder of the room files** (`project.toml`, `rooms`), with the folders in it: a model, a texture, and a sound are named from the room file.
- **"By hand with Blender and a paint program"**: scripts ran Blender with no window (the save of the cube is Blender's own save) and wrote the files. One file came from Photoshop itself: see section 7.
- **An error is also a notice**: the status row has one line, and the next file writes over it.

## 5. Decisions where the stage file was silent

- A source is converted when its result is older or not there, and only when it did not change for 0.3 s. No list of what was converted is kept: the times of the two files say it, also after a new start of the editor.
- One conversion at a time, in the order of the file names. A save while the converter runs: the result of the save before gets the time of that save, so the next look converts again, and the last save wins.
- The thread ends with its app (a flag): the editor makes a new app when it loads the code of a project again (stage 44).
- A result file with the mark of its source is not locked: a person can change it, and it stays until the source is saved again (watch 4).
- The message of a converter in the status row is its first line.
- Git (watch 7): the README says that a result file is binary and changes with each save of its source, and that the owner decides what is committed.

## 6. Files

| File | Role |
|---|---|
| `crates/editor/src/sources.rs` (new) | The table, the thread, the three converters, the reader of the flat picture of a `.psd` file, where Blender is, the reports for the status row; the tests |
| `crates/editor/src/lib.rs` | `[editor] blender`; the thread starts with the module; the table for the file lists |
| `crates/editor/src/widgets.rs`, `panels.rs`, `event_panel.rs` | The file lists: the result of a source that is not converted yet, and the mark of a file with a source |
| `crates/editor/Cargo.toml` | `image` |
| `crates/ui/src/theme.rs` | The icon of the mark |
| `README.md`, `OPEN_ITEMS.md`, `docs/part6/README.md`, `docs/part6/HANDOVER.md` | The editor section, the `[editor]` table, the note for git; one row; the stage table; the state |

## 7. What went wrong in the check: Photoshop

To have a file of a real paint program, the session started Photoshop 2025 by automation (it was not running) and let it save a file of two layers. At its start Photoshop restored a document of the owner that it had recovered, with changes that are not saved (`202610 duuuuuud-Recovered.psd`). The quit by the script did not close Photoshop, and a close of its window by the session opened its "save changes?" dialog. The session did not answer the dialog and did not touch Photoshop again: it is open, and the answer is the owner's. The script also set the dialog mode of Photoshop for scripts to "no dialogs" and could not set it back (Photoshop was busy with the dialog).

A program with documents of the owner is not started by a later session: a note is in the memory of the assistant.

## 8. Uncertain

- A `.psd` file that Photoshop saved with "maximize compatibility" off (watch 5): not tried. The setting of this machine is "ask", and the file of the check had the flat picture. The reader gives what Photoshop wrote as the flat picture, whatever it is.
- No person used Blender or a paint program in the check: a save by hand from the window of Blender (with a backup file, with the file in use) can behave in another way than a save by a script. Blender wrote `box.blend1` beside the file; the converter does not take it for a source.
- The start of Blender is 1.4 to 2.7 s on this machine; each `.blend` file pays it.
