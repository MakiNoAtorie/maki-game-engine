# Stage 37: pictures and models

Plan: [README.md](README.md). Report: `docs/part5/stage-37.md`. Item: M10 of [`../part4/MISSING.md`](../part4/MISSING.md).

Goal: a texture and a model are picked by a small picture, and a room of the game has a model from a file.
Gate: a model and a texture are picked by their picture; the model shows in two rooms with one load; a click selects it.

Written after the report of stage 36, from the code at `d33b04a`.

## Context in the code

### The UI pass

- `UiOverlay` (`crates/ui/src/overlay.rs`) keeps one GPU image for each texture that egui sends (`TexturesDelta`): today the font image only. A draw with a texture that the pass does not have is skipped (`overlay.rs:164`).
- One sampler for all textures (`overlay.rs:59`, the `ponytail` comment). egui sends a filter and a wrap mode with each texture (`ImageDelta::options`); the pass does not read them. The sampler index is a field of the push block of each draw.
- egui has two kinds of texture. A managed texture: a panel gives pixels to `Context::load_texture`, and egui sends them in `TexturesDelta`. A user texture (`TextureId::User`): the renderer gives an id for an image that it has.

### The pickers

- `widgets::file` (`crates/editor/src/widgets.rs:187`) lists the files of the directory of the room file and one level below it, by extension, in the list with a search field of `event_panel::picker`. `texture` and `mesh` call it with `PICTURES` and `MODELS`.
- `described_list` (`crates/editor/src/event_panel.rs:252`) draws the list: a text for each name.
- "Add > Model file" (`crates/editor/src/panels.rs:296`) is a menu of file names.

### Models and textures

- `load_gltf` (`crates/assets/src/model.rs:39`) reads a model into stores that the caller gives, on the calling thread. `Textures::load` does the same for an image file. The stores can be ones that a tool makes for one call.
- `Mesh` gives `positions`, `indices`, `uvs`, `base_color`, `base_color_texture`, `alpha_cutoff`.
- No pass draws one model into a small image.

### The room loader

- `Spawner::prop` (`crates/scene/src/room.rs:819`) loads a `MeshDef::File` one time for each path (`RoomModels`). `load_requested_room` removes the models that the next room does not name: two rooms keep a model only if the two name the same file.
- `spawn_loaded_models` gives each mesh of a model the `RoomItem` of its prop: `view::pick` (`crates/editor/src/view.rs:194`) selects the prop by a hit on a mesh.
- No room of the game names a model. No test has two rooms with one file.

### Content

- `tools/gen_textured.py` writes PNG files and a `.glb` with its images in the file, with no library. `tools/gen_sounds.py` is the form of a placeholder script for the game.
- The trip script (`game/src/scripts.rs`) holds frame numbers that fit rooms A and B. A collider in the way of the player breaks it.

## Prompt

```text
Stage 37: pictures and models.
Read docs/part5/stage-37-pictures-and-models.md and item 10 of
docs/part4/MISSING.md first.

Textures of a panel in the UI pass
- A panel shows an image with pixels of its own. The pass draws it
  with the filter and the wrap mode that the texture asks for.

Small pictures
- The editor makes a small picture of an image file and of a model
  file, one time for each file, and again when the file changes.
- The picture of a model: its triangles, drawn on the CPU from one
  fixed direction, with the colors and the textures of the model.

Pickers
- The file list of a texture and of a model shows the picture of each
  file beside its name. The value that is set shows its picture too.
- "Add > Model file" shows the pictures.

Content
- tools/gen_models.py writes a placeholder model and a placeholder
  texture into game/assets/rooms. No file of a modelling tool.
- Rooms A and B name the model. The trip script still works.

Out of scope: a pass that draws a model into an image, pictures of
room files and sounds, a picture cache on disk, mip levels of a UI
texture, materials of glTF that the renderer does not have.

Done when: a test loads two rooms that name one model file and the
meshes of the model stay; a test clicks a model in the editor and its
prop is selected; a test makes the picture of a model; a texture and
a model are picked by their picture in the running editor; the log of
a trip shows one load of the model; validation is silent. Update
OPEN_ITEMS.md ("Room file props from files", "egui features that are
off", "Player model from glTF"). Report.
```

## What to watch for

- **The picture of the game.** The code of the stage changes no pixel: `trip_720p` byte-equal before the rooms get the model. With the model, the pictures of the trip change: the reason is the content.
- **The trip script.** Each leg ends at the same place as before; the switch and the door work.
- **GPU time.** The scenarios with no room of the game do not move. Rooms A and B have more triangles: the step is named in the report.
- **One load.** The log of a trip (A, B, A) has one `model stored` line. The model is not read again for its picture while the game runs with no editor.
- **A long frame in the editor.** The first open of a file list makes the pictures: measure the time of the largest model.
- **A file that is not a picture or a model.** The list shows its name with no picture; the error is logged one time, not in each frame.
- **A file that changes.** The owner puts a real model in the place of the placeholder: the picture follows with no restart.
- **Texture memory.** A picture is small (one size for all); a picture of a file that is gone is freed.
- **The filter.** The font text is as sharp as before. A pixel-art texture shows hard edges in its picture.
- **The display scale.** `[ui] scale = 1.5`: the pictures are sharp and the list fits.
