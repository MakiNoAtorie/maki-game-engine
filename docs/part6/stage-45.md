# Stage 45: a changed asset file shows at once

State: done. Stage file: [stage-45-asset-reload.md](stage-45-asset-reload.md) (a proposal that the owner did not change: taken as given). Plan: `README.md`, 2.5.

## 1. What was built

- **The watch** (`maki_assets`, `watch.rs`): each store tells the watch the files that it loaded, each with a check (a function that reads the file as the store does). The watch looks at the time of each file 4 times in a second, some files in each frame. A file whose time changed waits until the time did not change for 0.3 s; then its check runs.
- **Two messages.** `AssetChanged(path)`: the file reads. `AssetFailed { path, error }`: it does not, and nothing changes.
- **Each store forgets the file in its own module**: the texture store (`maki_assets`), the models of the room (`maki_scene`), the model of the player (`maki_gameplay`), the sounds (`maki_audio`). No module names the store of another.
- **The room loads again** (`maki_scene::reload_changed`), from the text that it shows and with the player in place. A generated mesh with a texture of the file is made again, and a model of the file (or with an image file that changed) is read again. Each other mesh is kept.
- **Sound**: a sound at a place that plays the file starts again with the new file, music from its start. No room load.
- **The editor** turns the watch on, while a room is edited and while a game runs. The status row has one line for a file that was loaded again, and for a file that does not read. "Assets > Load all again" forgets each file.
- **Off in the game**: `[assets] watch` is false by default. A game that ships, a benchmark, and a test look at no file.

## 2. Results

| Check | Result |
|---|---|
| The gate by a test with real files (`maki_scene`: `a_file_that_is_written_again_shows_...`) | A texture file painted again: the floor has the color in less than 1 s (the test fails at 1 s), one room load, the cube is the mesh of before, the old texture is gone from the store. A file that is not a picture: 1 s with no change. The model file written again: a new model |
| Written in two parts, 0.2 s apart (`maki_assets`) | Told one time, 0.3 s after the second part. With a clock and file times of the test |
| A file that does not read; a store that tells its files again; a file that is gone and comes back (`maki_assets`) | `AssetFailed`; the change is not lost; told again |
| A sound file (`maki_audio`) | The lamp hums the new file (another length); the music starts again |
| The model of the player (`maki_gameplay`) | A new mesh on the player; the old one is gone from the store (watch 3) |
| By hand, in the editor, a room with a wall texture, the cabinet, and a lamp that hums; a script writes the files as another program does | Wall texture in another color: 0.68 s from the write to the upload of the new texture. Model of the cabinet with another height: 0.57 s. Sound while the game runs: 0.65 s, "sound at a place starts". The small picture of the inspector follows |
| Half of a picture file | The status row: "wall.png changed, but it does not read: unexpected end of file. The old content stays". The walls keep their color |
| 20 times a new texture (watch 2) | GPU memory in the stats panel: 1716.0 MB before, 1716.0 MB after. `MAKI_LOG=debug`: no validation error |
| "Assets > Load all again" | The room loads again; the status row has the count of the files |
| `cargo test`, static; dynamic | 158 and 5 passed, 0 failed |
| `cargo clippy --all-targets`, static and dynamic | Clean |
| The game (no watch) | `bench.py same trip_720p s44 s45`: ok. `gpu_total` of the 26 scenarios against `s44`: -0.8 % to +0.1 %; `trip_720p` 1.917 ms in the two runs |

The cost of the watch (watch 5): one look at a file time is 30 microseconds on this machine. Room A has 4 files: 16 looks in a second, one look in each fourth frame at 60 frames in a second. A room with 100 files: 7 looks in a frame, 0.2 ms. In the editor only.

The history of the picture (watch 1): the load starts the samples again, as each room load does. With 2 samples in a frame, the 1024 samples of a still picture are full after 512 frames. A picture taken 2 s after the change showed no noise at half size; the first frames were not looked at one by one.

## 3. Files

| File | Role |
|---|---|
| `crates/assets/src/watch.rs` (new) | `AssetWatch`, `AssetChanged`, `AssetFailed`, the looks and the settle time, the systems of the texture store, the two tests |
| `crates/assets/src/texture.rs`, `model.rs`, `lib.rs` | `Textures::files` and `forget`, `check_image`, `check_model`; `[assets] watch`; the systems |
| `crates/scene/src/room.rs`, `lib.rs` | `reload_changed`, `watch_models`, what `RoomModels` holds as old; the loader does not take old meshes and models; the test with real files |
| `crates/audio/src/output.rs`, `lib.rs`, `tests.rs` | `Audio::forget` and `files`, the volume of the music; the two systems; the test |
| `crates/gameplay/src/player.rs` | `PlayerModel`, `reload_player_model`; the test |
| `crates/editor/src/lib.rs`, `panels.rs` | The watch is on; the lines of the status row; the command and the menu "Assets" |
| `README.md`, `OPEN_ITEMS.md`, `docs/part6/README.md`, `docs/part6/HANDOVER.md` | The `[assets]` table and the editor section; one row; the stage table; the state |

## 4. Deviations from the stage file

- **"One time by hand with a paint program"**: the files were written by a script (Pillow for the picture, the model generator of `tools/` for the cabinet, the `wave` module for the sound), not by a person in a paint program. Photoshop and Blender were not at hand (watch 7): a save that writes a new file and gives it the name of the old one was not tried with them. The watch looks at the time of the path, so it sees such a save; a test covers a file that is gone for a moment.
- **The room is loaded from the text that it shows** (`CurrentRoom::source`), not from its file, and by the scene module also in the editor. The stage file said "the editor makes its world again from the document": this is the same room, and the editor needs no code for it.
- **The looks are spread over the frames** (each file one time in 0.25 s), not all files in one frame each 0.25 s: one frame with all looks would show in the frame time.

## 5. Decisions where the stage file was silent

- **Who tells the watch.** Each store tells its own files when they are other ones (`AssetWatch::set` with the name of the store). A file that two stores name is watched one time.
- **How the scene module finds what is old**, with no look into the texture store: a texture of an image file has the name of its file. A texture that did not load is tried again too: a file that was not there, and comes, shows.
- **An image file of a model** (the cabinet names `textures/wood.png`): the model is read again when the image changes.
- **A room change that waits** when a file changes: no second request; the room change reads the file.
- **A sound of `PlaySound`** that plays goes on to its end; the next one is the new file.
- **Room files and prefab files are not watched** (watch 4): the editor has the document of the open room, with changes that are not saved, and a room file that another program writes is a conflict that needs a question. "Assets > Load all again" loads the room again, and so reads its prefab files.
- **`[assets] watch = true` in a game** turns the watch on there too (a developer who wants it). The editor does not read the key.
- **The check runs on the thread of the game**: it decodes the file one more time. A picture takes a millisecond, the cabinet some.

## 6. Uncertain

- A game that runs in the editor goes back to the room as it was loaded (a door, a mover, a light that an event changed) when a file changes: the room is loaded again.
- The times above are of a small room. A room with large textures takes longer by the time of the decode, two times (the check and the load).
- A program that holds the file open for a long time while it writes: the watch waits for 0.3 s with no change of the time, and then the check must pass. A program that sets the time first and writes later than 0.3 s gives one line "does not read" and then the load.
