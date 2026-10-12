# Stage 21: rough materials, emissive meshes, cone light

State: done. Plan: `README.md`, section 5.

## 1. What was built

- `Material::Metal { roughness }` and `Material::Plastic { roughness }` (assets crate), and `Mesh::with_emissive`. The glTF loader maps the metallic and roughness factors to the five kinds and reads the emissive factor. Room files: `material: Metal(roughness: 0.3)`, `look: (emissive: (r, g, b))`.
- Trace pass: the GGX reflection at the surface that the pixel shows. A shadow ray gives the bright spot of a light; the path continues with a facet normal from the visible normals (Dupuy and Benyoub, 2023). The two share the light of a lamp by the power rule of multiple importance sampling (`light_pdf`). Plastic takes the reflection or the diffuse bounce at random. All of this light goes to `specular`.
- Path regularisation: after a bounce, and in the reflection on glass, metal and plastic count as diffuse.
- Denoiser: `specular` has its own prepare and filter passes (`denoise_specular_*`, 2 passes). The roughness of the surface is a guide (fourth channel of `albedo`): no filter up to a roughness of 0.15, the full filter from 0.5. A filter pass does nothing for a pixel with a variance of zero.
- Emissive meshes: the light of the surface is added where a path finds it (as emission where the pixel shows it, as bounce light after a bounce).
- Cone light: `Light::direction` and `Light::cone_degrees`; `direction` and `cone` in a room file. A soft edge in the outer fifth of the cone.
- `game/assets/rooms/room_c.ron`: three metal spheres (roughness 0.15, 0.35, 0.6), two plastic ones (0.12, 0.45), a dark plastic plate, a panel with a light of its own, a cone light. Scenario `materials_720p`.

## 2. Numbers

`materials_720p`, 1 sample for each pixel and frame, against a reference of 65536 samples:

| Run | `relmse` / sharpness / noise |
|---|---|
| 4 frames | 0.00179 / 1.31 / 3.42 |
| 64 frames | 0.000308 / 1.018 / 1.17 |
| 64 frames, `denoise_specular_passes = 0` | 0.000383 / 1.029 / 1.48 |
| 1024 frames | 0.000266 / 0.999 / 0.97 |
| 64 frames, crop of the three metal spheres | 0.00155 / 1.13 / 1.21 |

Gate: the rough metal and the plastic look as in the reference (`pictures/stage-21-materials.png` is the game picture; `s21_m_f64.png` has picture and reference side by side). No new fireflies at 1 sample: the noise number is 1.17 at 64 frames, and the worst pixels are those of the lamp reflection. Met.

`test_720p` and `walk_720p` did not change (0.000201 and 0.00224).

Cost, run `s21c` against `s19`, 1080p: `gpu_trace` +17 % (`test_1080p`) and +22 % (`heavy_mixed_lights8_1080p`), also with no rough material in the scene. The specular passes: 0.19 ms (prepare 0.11, two filter passes 0.04 each: they do nothing where the variance is zero). `gpu_total`: 3.55 -> 3.89 ms and 6.20 -> 7.31 ms.

## 3. Files

- `crates/assets/src/mesh.rs`, `model.rs`: the two kinds, `emissive`, the glTF mapping.
- `crates/scene/src/lib.rs`, `room.rs`: `Light::{direction, cone_degrees}`, `LightDef::{direction, cone}`, `LookDef::emissive`.
- `crates/render/shaders/scene.glsl`: `MeshEntry` (48 bytes).
- `crates/render/shaders/trace.comp`: `LightData` (48 bytes), `cone_part`, `light_pdf`, the GGX functions, the diffuse and rough branch of the path.
- `crates/render/shaders/denoise_prepare.comp`, `denoise_filter.comp`, `denoise_temporal.comp`: the specular signal.
- `crates/render/src/denoise.rs`, `passes.rs`, `lib.rs`, `light.rs`, `meshes.rs`: `Signal::Specular`, buffer layouts.
- `crates/core/src/config.rs`, `app.rs`, `game/src/debug.rs`: `denoise_specular_passes`.
- `game/assets/rooms/room_c.ron`, `tools/bench.py` (`materials_720p`).

## 4. Deviations

- Path regularisation is "count as diffuse", not "count as rougher". A path ends at its second surface that is not a mirror or glass, so a later rough surface gets only a shadow ray: with its reflection, that ray is the path "lamp, rough surface, diffuse surface, camera", the source of bright speckle. No Russian roulette: the paths did not get longer.
- The roughness guide stops the filter for a smooth surface; it does not compare the roughness of two pixels. The surface tests of the filter (normal, plane) already keep two objects apart.
- The variance of the specular light is the 3x3 measure only (no moments over time for it).
- MIS is between the light sample and the reflection sample of the rough surface only. A diffuse surface keeps the shadow ray alone, as before (audit item 3).

## 5. Decisions

- Two material kinds (`Metal`, `Plastic`), not one with a metal amount: a surface is one or the other in most files.
- Roughness is at least 0.05; the roughness that the model uses is its square (`alpha`), as in glTF.
- Plastic reflects 4 % at a straight angle (an index of refraction of 1.5).
- glTF: a roughness factor of 0.9 or more is diffuse. This keeps the glTF default (a rough metal) and the generated benchmark scenes as they were.
- A cone light is as bright in its cone as a light for all directions with the same `intensity`.
- `denoise_specular` is a 32-bit image now (it was 16-bit), as the prepare pass reads all means from one image array.
- The glass reflection path and the paths after a bounce show an emissive surface; no shadow ray aims at one.

## 6. Uncertain

- The reference is the same code with more samples: it proves that the 1-sample picture converges to it, not that the GGX math is right. Checked by eye only: the bright spots get wider and dimmer with the roughness, metal reflections have the metal color, plastic ones are white, and the edge of a sphere reflects more.
- The cost of the trace pass in scenes with no rough material (`OPEN_ITEMS.md`, "Cost of the path state").
- The sphere of a cone light shows black from behind (`OPEN_ITEMS.md`).
- No glTF file from a modelling tool was loaded.
