# Stage 37: pictures and models

State: built. Gate: met. A model and a texture are picked by their picture in the running editor; the cabinet shows in rooms A and B, and the log of a trip (A, B, A) has one load; a click on it selects its prop.

No file of a modelling tool was used. The model and the texture of the game are placeholders of `tools/gen_models.py`.

## 1. What was built

- **Textures of a panel in the UI pass** (`crates/ui/src/overlay.rs`): the pass reads the filter and the wrap mode that egui sends with each texture (`ImageDelta::options`) and keeps a sampler for each pair. A panel gives pixels to `egui::Context::load_texture`; the pass had the code for the upload already (the font image).
- **Small pictures** (`crates/editor/src/pictures.rs`): one picture of 96 pixels for each image file and each model file, made when a list shows the file, kept by path and time of the last change. One picture in each frame at most. An image file is made small with the nearest texel. A model file is read with `load_gltf` into stores of that call, and its triangles are drawn on the CPU: from the front, a little from the right and from above, no perspective, a depth for each pixel, one brightness for each triangle, the base color, the base color texture, and the alpha cutoff.
- **Pickers**: `widgets::file` shows the picture of each file in its list and the picture of the value. "Add > Model file" shows the pictures. A list with no image and no model (sounds) is as before.
- **Content**: `tools/gen_models.py` writes `game/assets/rooms/textures/wood.png` (32 x 32, pixel art) and `game/assets/rooms/models/cabinet.glb` (4 meshes, 84 triangles, a metal material for the handles; it names `wood.png` as a file). Rooms A and B have the cabinet at the back wall.
- `tools/gen_textured.py`: `write_glb` takes the materials and the meshes; an image can be a file path. `props.glb` has the same bytes as before.

## 2. Numbers

- `cargo test --workspace`: 143 tests, 144 with `--features editor` (3 new). `cargo clippy --workspace --all-targets` clean with and without `--features editor`. `cargo fmt --check` clean.
- New tests: `two_rooms_that_name_one_model_file_load_it_one_time` (scene: the mesh handle of the model is the same in the second room, at the place of its prop; a room with no model removes it; then the file is read again), `a_click_on_a_model_from_a_file_selects_its_prop` (editor: the file is read on a thread, a click on a triangle of it selects the prop), `a_model_is_drawn_with_its_color_and_with_a_brightness_for_each_side` (editor).
- Debug build with the validation layer, the trip script, 900 frames: no error. One `model stored` line. Each `script` line (the place of the player at the end of each leg), each event, and each message is the same as at `d33b04a`.
- Time of a picture, on the thread of the editor:

| File | Debug build | Release build |
|---|---|---|
| `cabinet.glb` (84 triangles, one image file) | 4.4 ms | - |
| `props.glb` of `gen_textured.py` (5 images in the file) | 1.6 to 6.7 ms | - |
| A PNG of 16 to 64 pixels | 0.6 to 1.3 ms | - |
| A model of 97 152 triangles (`gen_scene.py --objects 44`) | 70 ms | 5 ms |

### Benchmark, `s36` against `s37`

- Before the rooms got the cabinet (side run `x37`, rows removed): `bench.py same trip_720p s36 x37`: `ok`, byte-equal. The code of the stage changes no pixel.
- With the cabinet: `same` for `textured_walk_720p`, `heavy_1080p`, `stress_720p`: `ok`. `trip_720p` and `walk_720p` differ: the cabinet is in the picture.
- GPU medians. The scenarios with room A or B have 4 more meshes and one more texture; the others did not move.

| Scenarios | `gpu_trace` | `gpu_total` |
|---|---|---|
| `test_*`, `static_1080p`, `walk_720p`, `trip_720p` (rooms A and B) | +2.8 % to +3.6 % | +0.5 % to +2.8 % |
| `stress_*`, `heavy_*`, `materials_720p`, `textured_*` | -0.1 % to +0.2 % | -0.4 % to +0.5 % |

`trip_720p`: 1.903 ms to 1.913 ms. `test_1080p`: 4.653 ms to 4.720 ms. `heavy_mixed_1080p`: 7.127 ms to 7.127 ms.

## 3. Files

- `crates/ui/src/overlay.rs`: the sampler of each texture.
- `crates/editor/src/pictures.rs` (new): the picture store, the two ways to make a picture, the widget.
- `crates/editor/src/event_panel.rs`: `described_list` and `picker_with` draw a picture at the left of a name. `name_list` is removed (one caller).
- `crates/editor/src/widgets.rs`: `file` shows pictures. `crates/editor/src/panels.rs`: "Add > Model file". `crates/editor/src/lib.rs`: the module.
- `crates/editor/src/tests.rs`, `crates/scene/src/room.rs`: the tests, with a glTF file of one triangle as a text.
- `tools/gen_models.py` (new), `tools/gen_textured.py`.
- `game/assets/rooms/models/cabinet.glb`, `game/assets/rooms/textures/wood.png` (new), `room_a.ron`, `room_b.ron`.
- `OPEN_ITEMS.md`, `tools/bench_history.csv`, `tools/bench_env.csv`, `docs/part5/stage-37-pictures-and-models.md`, `docs/part5/pictures/s37_*.png`.

## 4. Deviations

- **No `TextureId::User`.** The plan says "egui user textures". egui has two kinds of texture: one with pixels of a panel (`load_texture`) and one for an image that the renderer has (`TextureId::User`). The pictures are pixels of the editor, so the first kind does the work with no new API of the UI crate. A draw with `TextureId::User` is skipped as before. It is needed when a panel shows an image of the renderer; the row "egui features that are off" says so.
- **"A model from a modelling tool"** is a placeholder of a script (decision of the owner).

## 5. Decisions

- **The picture of a model is drawn on the CPU**, in the editor. No render pass, no GPU image, no change of the renderer. It does not show the light of the game or the material kind (a mirror shows as its color).
- **The file is read a second time for its picture**, into stores that are dropped: the stores of the game do not get a model that no room names. The game with no editor makes no picture.
- **On the thread of the editor, one picture in each frame.** See the times above.
- **The store is in the memory of egui** (`Context::data`): the widgets get a `Ui` only, also the public `widgets::file` of a module.
- **A picture follows its file** by the time of the last change. The pictures of files that are gone are freed when the next picture is made.
- **96 pixels, shown at 48 points**: sharp up to a display scale of 200 %. The rows of a list with pictures are 48 points high.
- **The filter of a UI texture**: the magnification filter for the two directions, no mip levels.
- **The cabinet names its image as a file** (`../textures/wood.png`): one file is a texture to pick and the image of the model.
- **Rooms A and B** get the cabinet: they are the two rooms with a way between them. The pictures of `trip_720p` and `walk_720p` change from this stage on.

## 6. Uncertain

- A file of a modelling tool can have what the placeholder does not: a node tree, a scale in centimeters, texture transforms, many materials. The loader has tests for nodes and materials; the picture code uses what the loader gives.
- A model with one very far part (a light stand, a stray vertex) is small in its picture: the picture shows the box around all meshes.
- The editor does not load a model again when its file changes while the room is open; only the picture follows. A reload of the room with the same path keeps the model (`RoomModels`).
- A texture checkbox that is set with no file makes the room loader log `cannot load texture <directory>` at each reload, until a file is picked. Seen in the hand session; it is older than this stage.
- In the debug build on a display of 3840 x 2160, the log has `slow frame` lines of 57 ms while an item is selected. Not caused by the pictures (their times are in the log); not looked into.
- The selection of a model draws one box for each of its meshes (4 for the cabinet). As before.

## 7. What to watch for: results

- **The picture of the game**: byte-equal before the content (`x37`); with the cabinet the trip pictures change.
- **The trip script**: the same place at the end of each leg; the switch, the messages, and the door as before.
- **GPU time**: +3 % in `gpu_trace` for the scenarios with rooms A and B; no step in the others.
- **One load**: one `model stored` line for A, B, A. The game with no editor has no picture code.
- **A long frame in the editor**: 70 ms for 97 152 triangles in the debug build, 5 ms in the release build; 4 ms for the cabinet.
- **A file that is not a picture or a model**: the loader logs the error one time; the store keeps "no picture" for the file until it changes. A value with no file is not tried.
- **A file that changes**: `cabinet.glb` was replaced while the list was open: the new picture showed in the next frame.
- **Texture memory**: 96 x 96 x 4 bytes for each file. A file that is gone loses its picture at the next picture that is made.
- **The filter**: the font texture asks for the linear filter, as the one sampler before. Pixel art keeps its hard edges (`s37_texture_list.png`).
- **The display scale**: `[ui] scale = 1.5`: the pictures are sharp and the menu fits.

Pictures: `pictures/s37_room_a.png`, `s37_room_b.png` (the game), `s37_model_list.png`, `s37_texture_list.png`, `s37_texture_picked.png` (the editor, 3840 x 2160).

## 8. Open items

- "Room file props from files" and "Player model from glTF" stay, with the facts of this stage: no file of a modelling tool.
- "egui features that are off": textures of a panel are drawn; `TextureId::User`, paint callbacks, the clipboard are not.
