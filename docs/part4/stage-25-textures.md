# Stage 25: textures

Plan: [README.md](README.md). Report: `docs/part4/stage-25.md`. Pictures: `docs/part4/pictures/`.

Goal: glTF models and room props show their textures.
Gate: a textured glTF room shows correctly, at rest and in motion, with the measured GPU cost.

## Context in the code

### Assets

- `crates/assets/src/model.rs`: the loader does not call `gltf::import`, so that no image is decoded (comment at line 58). The `gltf` crate already brings the `image` crate: decode needs no new dependency.
- A `Mesh` has one material: a kind (`Material`: diffuse, mirror, glass, metal, plastic), a base color, an emissive color. `material_kind` selects the kind from the metallic, roughness, and transmission factors. No UVs.
- Generated meshes: room box, cube, cuboid, sphere, capsule (`mesh.rs`). The sphere already has a second column of vertices at its seam, for texture coordinates later.
- A model file is loaded one time for a room (`RoomModels`). Loading waits for the file (`OPEN_ITEMS.md`, "Asynchronous loading").

### Room files

- `LookDef` (`crates/scene/src/room.rs`): `color`, `material`, `emissive`. Props and movers have one.
- The room box has `width`, `depth`, `height`, `doorways`. It has no look: floor and wall textures need new fields. `RoomDef` has `deny_unknown_fields`.

### GPU

- `ImageDesc`: "a 2D color image with one mip level". `Format` has `Rgba8Unorm` and float formats, no sRGB format.
- `SamplerDesc`: `linear` and `repeat`. No filter between mip levels.
- Bindless set: sampled images at binding 2, samplers at binding 3 (`bindless.glsl`). `Gpu::write_image`, `image_index`, `sampler_index` exist. The UI font uses them.
- `Gpu::destroy_image` is deferred, as for buffers.

### Trace pass

- Mesh data that the shader reads (`scene.glsl`): `MeshEntry`, 48 bytes for each mesh (address of the normals, material kind, parameter, base color, emissive), and `NormalBuffer`, 4 bytes for each triangle corner (a packed normal).
- Each BLAS geometry has the flag `OPAQUE` (`accel.rs:160`). Each ray query has `gl_RayFlagsOpaqueEXT`, and the loop is `while (rayQueryProceedEXT(query)) {}` with no body. A cutout needs geometry without the flag and a test in the loop.
- Camera rays cull back faces, to look into the room through the near walls (`OPEN_ITEMS.md`, "Single-sided parts of a model"). A leaf card is single-sided.
- The trace pass writes the guide `albedo`. The denoiser takes the albedo out of the light and `denoise_resolve` puts it back: texture detail in `albedo` is not filtered. Behind a mirror or glass, the guides are those of the virtual point.
- An emissive mesh lights the room only by bounce rays that find it by chance (`OPEN_ITEMS.md`, "Light of a mesh").
- The path tracer can render below the picture size (`render_scale`), and TAA moves the camera rays in each frame. The two change the right mip level.

### Budget and tools

- Budget 8 ms. With the defaults: `test_1080p` 4.69 ms; `heavy_mixed_lights8_1080p` 9.34 ms, which is over the budget before this stage. `OPEN_ITEMS.md`, "Cost of the path state": stage 21 made the trace loop 17 to 22 % slower with more live values.
- `tools/gen_scene.py` makes the glTF scenes of the benchmark. `tools/bench.py` has the list `SCENARIOS`.
- `bench.py quality` gives `relmse` and a sharpness number against a reference picture. Sharpness below the value of the run before means blur.
- Limits: `MAX_MESHES` 1024. The loader makes one mesh for each glTF node.

## Prompt to paste

```text
Stage 25: textures.

Goal: glTF models and room props show their textures. The target look is
stylized (Animal Crossing, Pokémon Let's Go): clean color maps, some
pixel-art textures, cutouts for foliage and fences. Read
docs/part4/stage-25-textures.md first: it lists what the loader, the gpu
crate, and the trace pass have today.

Assets
- The glTF loader reads, per material: base color texture, emissive
  texture, alpha mode (opaque, mask with cutoff; blend may be treated as
  mask, say so), and the texture coordinate set 0. Texture transforms and
  other coordinate sets may be left out; log once when a file uses them.
- Images are decoded at load and kept as a texture asset with a
  generational handle. The same image file used by two materials or two
  models loads once. A room change removes the textures that the next room
  does not use, as it removes models.
- Generated meshes (room box, cube, cuboid, sphere, capsule) get UVs. Room
  walls and floor get UVs in meters, so a tiling texture keeps its size on
  any room size.
- Room files can give a generated mesh a texture: a path, a tiling scale,
  and the filter, in LookDef. The floor and the walls of the room box can
  each have one (new fields of RoomDef).

GPU
- The gpu crate gets images with mip levels and an sRGB format. Textures
  upload to sampled images in the bindless set, with mip maps (generated at
  load is fine). sRGB for color, linear for data.
- A material chooses linear or nearest filtering, so pixel-art textures stay
  crisp. Default: linear. Room files and glTF (sampler of the material) set
  it. SamplerDesc gets what the mip levels need.
- Free the GPU image when the texture asset is removed. Deferred
  destruction as for buffers.

Trace pass
- The mesh data the shader reads gets UVs and the texture indices of each
  material. Today it is 48 bytes for each mesh and 4 bytes for each
  triangle corner. Keep it small; say what you added and its size. UVs in
  meters go past 0..1: say how they are packed.
- Mip level: compute shaders have no derivatives. Choose the level from the
  ray (a ray cone or a simpler distance-and-angle estimate). Take
  render_scale into account. Bounce hits use a coarse level. Say which
  method and why.
- Alpha cutout: camera rays, bounce rays, and shadow rays pass through cut
  out texels (a test in the ray query loop). Only meshes with a mask
  material lose the OPAQUE geometry flag and pay for the test; opaque
  meshes stay on the fast path.
- Camera rays cull back faces. Say what a cutout card shows from behind,
  and limit the cull to the room box if that is the simple fix.
- The albedo guide is the textured color, so the filter does not blur
  texture detail. Check that TAA and the denoiser keep texture detail at
  rest and in motion, also behind a mirror and behind glass.
- Emissive textures add to the light of the surface the same way emissive
  factors do now.

Test content
- Test textures and a textured glTF scene are made by code (tools/gen_scene.py
  or a test), not committed as hand-made files: a checker, a pixel-art
  pattern with nearest filtering, a leaf cutout, an emissive sign.
- Add a benchmark scenario with this scene to tools/bench.py, at 720p and
  1080p.

Out of scope: normal maps, metallic/roughness/occlusion textures, texture
compression, streaming. List them in OPEN_ITEMS.md.

Done when the textured scene shows correctly (no shimmer at rest, no
blur of texture detail by the denoiser, crisp nearest textures, correct
cutout shadows), the room box takes floor and wall textures from a room
file, the existing scenarios keep their numbers (gpu_trace and gpu_total
of test_1080p, heavy_mixed_lights8_1080p, walk_720p, materials_720p),
the GPU cost and the sharpness number of the new scenario are reported,
and validation is clean. Update OPEN_ITEMS.md ("Textures from glTF",
"Single-sided parts of a model", "Material kinds from glTF"). Stop and
report, with pictures in docs/part4/pictures/.
```

## What to watch for

- **Blurred textures.** If the denoiser is guided by the color without the texture, detail is smeared. Toggle the raw and the denoised debug views on the checker floor, and compare the sharpness number of `bench.py quality`.
- **Shimmer.** A checker floor far from the camera that sparkles means the mip level is too sharp. One that turns gray early means it is too coarse. Check with `render_scale` below 1 too.
- **Cutout cost on everything.** Opaque meshes must not run the alpha test. Compare `gpu_trace` on the old scenarios. `heavy_mixed_lights8_1080p` is already over the budget: any rise there is a loss.
- **Vertex data bloat.** The trace pass reads mesh data at each hit. If the report adds much more than UVs and an index, ask why.
- **Pixel-art filtering.** Nearest filtering looks crisp at rest, but the TAA jitter can make it swim in motion. Look at it while the player walks.
- **Textures that stay loaded.** Go from a textured room to another and back some times, and read the "GPU memory" row of the stats panel. It must not grow.
- **Old room files.** `room_a.ron`, `room_b.ron`, and `room_c.ron` have no texture field and must load with no change.
