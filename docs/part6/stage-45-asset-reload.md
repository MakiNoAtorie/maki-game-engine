# Stage 45: a changed asset file shows at once

Plan: [README.md](README.md), 2.5. Report: `docs/part6/stage-45.md`. Needs: stage 43 (the project folder). Not stage 44.

Goal: the owner saves a texture, a model, or a sound in another program, and the editor shows or plays the new file, while a room is edited and while a game runs in the editor. No restart, no room change.
Gate: with the editor on room A: a texture of a wall is saved with another color from a paint program, and the wall has the color in less than 1 s. The same for the model file of the cabinet and for a sound. A file that is half written, or that does not decode, changes nothing and gives one line in the status row. The game that ships watches no file: `bench.py same trip_720p` says ok.

State: proposal, not approved.

Written from the code at `1c3158a`.

## Context in the code

- `Textures` (`crates/assets/src/texture.rs:68`) gives one handle for a path, and reads the file one time (`load_with`, line 86). `remove_unused` (line 124) removes the textures that no mesh uses, after a room change.
- `GpuTextures::sync` (`crates/render/src/textures.rs:47`) uploads a texture that it does not have, and frees one that the store does not have. It does not look at a texture a second time.
- The table entry of a mesh, with its texture slots, is written one time (the note at `texture.rs:76`).
- A generated mesh of a room has a key, the text of the numbers that it is made from, and a reload of the room keeps a mesh with the same key (`Spawner::mesh`, `crates/scene/src/room.rs:838`). The key has the path of a texture, not the time of the file.
- `RoomModels` (`room.rs:669`) holds the model of each path of the room. A model that the next room names again stays loaded. A model file is read on a thread (`ModelLoads`, `crates/assets/src/model.rs:101`).
- `Audio::load` (`crates/audio/src/output.rs:163`) gives one handle for a path. Music is a stream from its file.
- A reload of the same room with no spawn point keeps the player where it is (`room.rs`, the test at line 1502). The editor makes the world from the text of its document.
- The small pictures of the editor look at the time of the file already (`crates/editor/src/pictures.rs:48`).

## The way

A changed file is forgotten by its store, and the room is loaded again. The stores and the renderer keep their rule "one handle, one content": no content is replaced under a handle that is in use.

1. **Watch** (`assets`): the stores know the paths that they loaded. Each 0.25 s, one system reads the time of each of those files. No new crate: some tens of files.
2. **Settle**: a path whose time changed waits until the time did not change for 0.3 s. Then the file is decoded as a test. An error: one error line, and the old content stays.
3. **Forget**: a message `AssetChanged(path)`. Each store removes its entry for the path in its own module (`Textures`, the models, `Audio`). A handle that a mesh still has stays good until the mesh goes.
4. **Load again**: the scene module asks for the current room again, with no spawn point. A mesh whose key names a changed file is not kept. The editor makes its world again from the document. A sound needs no room load: the next start reads the file.
5. **Button**: "Assets > Load all again" forgets each file and does step 4. For a program whose save does not change the time of the file.

## Prompt

```text
Stage 45: a changed asset file shows at once.
Read docs/part6/README.md and docs/part6/stage-45-asset-reload.md first.

- A watch of the files that the stores loaded: time of the file, each
  0.25 s, a change counts after 0.3 s with no further change, and only
  if the file decodes. `[assets] watch`: true in the editor, false in
  the game.
- `AssetChanged(path)`: the texture store, the model store, and the
  audio module each forget the path. No module knows the store of
  another.
- The room loads again with the player in place. A generated mesh with
  a changed texture is made again; each other mesh is kept.
- A sound at a place that plays starts again with the new file. Music
  that plays starts again from its start.
- The editor: the same while a room is edited and while a game runs;
  the menu item "Assets > Load all again"; a file that does not decode
  is one line in the status row, and the old content stays.
- The small pictures of the file lists follow, as today.

Done when
- Each line of the gate, by a test with files that code makes and
  writes again, and one time by hand with a paint program.
- A test: a file that is written in two parts, 0.2 s apart, loads one
  time, after the second part.
- `bench.py run`, `compare`, and `bench.py same trip_720p` for the game.
- Validation clean while a texture is replaced 20 times.
```

## What to watch for

1. **The history of the picture.** A room load resets the denoiser for the frames after it. A changed texture needs new samples in any case; say in the report how many frames the noise is seen.
2. **GPU memory.** The old image is freed when the last mesh with it is gone, and not before the frame that uses it has ended. Replace a texture 20 times and compare the memory before and after.
3. **A model in use by two rooms, and the player model.** The player model has no room as its owner (`OPEN_ITEMS.md`, "Owner of the meshes of a model"): it must change too, or the report says why not.
4. **A file of a prefab** and a room file that another program changes: the same watch, or not. Decide, and say it.
5. **The cost of the watch** in a room with many files: measure the system in the frame. It must not show in `cpu_ms`.
6. **Determinism.** No watch in a benchmark run and in a test that does not ask for it.
7. **Photoshop and Blender write a new file and rename it** in some settings: the path is the same and the time is new. Check with the two programs if they are at hand.
