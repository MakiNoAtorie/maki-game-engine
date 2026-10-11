# maki-game-engine

Top-down 3D horror game engine with path-traced lighting, written in Rust.

## Required tools

- Rust (stable), installed with [rustup](https://rustup.rs).
- [Vulkan SDK](https://vulkan.lunarg.com) 1.3 or later. It supplies `glslc` and the validation layers.

## Build and run

```
cargo build
cargo run -p maki-game                 # uses ./config.toml
cargo run -p maki-game -- my.toml      # uses the given config file
cargo run -p maki-game --release -- --bench-frames 300   # benchmark, see below
```

If the config file does not exist, the game writes one with default values.

- Log level: set `MAKI_LOG` (default `info`). Example: `MAKI_LOG=debug`.
- Tracy profiler: `cargo run -p maki-game --features profile`. Each schedule is a zone with the name of the schedule (`Input`, `FixedUpdate`, `Update`, `Extract`, `Render`, `Present`). Each frame graph pass is a zone with the name `pass`; the zone text has the pass name. The zones show CPU time. With `MAKI_LOG=trace`, the log lines show the same spans.
- Render self-test: set `MAKI_RENDER_SELFTEST`. The internal resolution changes every 7 frames; validation must stay clean.
- Scene counts: the log shows each mesh upload (vertices, indices) and a `render scene` line (meshes, instances, triangles) each time the counts change.
- Frame graph barriers: with `MAKI_LOG=debug`, the log shows the barrier list each time it changes.
- Accumulation: with `MAKI_LOG=debug`, the log shows `accumulation complete` when the sample count reaches `max_samples`, and `accumulation reset` when a mean of two or more samples starts again.

## Controls

The help panel in the game lists all controls, with the key that hides and shows the panel. It is open at start. The list is made from the binding table (`Bindings::default` in `crates/platform/src/input.rs`): to change a key, change the table.

The light moves along the floor, forward is away from the camera. It stays in the room. One wheel step changes the intensity by 10 %.

## Debug panels

The panels are egui windows on top of the picture. Drag the title bar to move a panel, click the arrow to fold it.

| Panel | Content |
|---|---|
| Help | Key and description of each binding |
| Stats | Frame time (mean), FPS, sample count, internal resolution, position and intensity of each light, GPU time of each frame graph pass |

While the mouse pointer is on a panel, the mouse wheel starts no action. While a text field of a panel has the keyboard focus, keys start no action.

To add a panel from a module, call `maki_ui::add_panel(app, "Name", true, |ui, world| { ... })` in `Module::build`. The closure gets the egui `Ui` of the window and the game world. `crates/ui/src/stats.rs` is an example.

## Benchmark

```
cargo run -p maki-game --release -- [CONFIG] --bench-frames N [--bench-dump FILE] [--bench-static]
```

The game runs N frames (1 to 1000000), writes `bench.csv` in the working directory, and stops. In this mode:

- The light moves on a circle around the room center, one turn in 240 frames. The position depends only on the frame number, so each run does the same work. The keyboard does not move the light.
- The light moves in each frame. With `light_history_samples = 0` each frame starts the accumulation again; with the default the picture keeps a short history. `--bench-static` holds the light at the start of the path, so the picture accumulates.
- `tools/bench.py` sets `samples_per_frame = 1` if the scenario does not set it: the automatic count would give each run another load.
- `--bench-dump FILE` writes the picture of frame N (the picture before the tone curve) to FILE: two little-endian `u32` (width, height), then four `f32` (R, G, B, A) for each pixel. The picture depends only on the config, the scene, and N. `python tools/bench.py same` compares two dumps.
- `vsync` is off, whatever the config file says.

`bench.csv` has one row for each frame:

| Column | Content |
|---|---|
| `frame` | Frame number, from 1 |
| `cpu_ms` | Time from the start of the frame to the start of the next one. It has all schedules, and the time that the CPU waits for the GPU and for the display |
| `gpu_<pass>_ms` | GPU time of each frame graph pass, in frame order (GPU timestamp queries). The time of a pass has the image barriers before it |

The first frames are slow (startup work): drop them when you compute a mean. An empty cell means that the value did not arrive (a skipped frame, or a GPU without timestamp queries). A debug build has the validation layers on, which makes `cpu_ms` much larger.

## Config

| Key | Default | Meaning |
|---|---|---|
| `window_title` | `maki` | Window title |
| `window_width`, `window_height` | 1280, 720 | Window size, in logical pixels |
| `render_width`, `render_height` | 640, 360 | Internal path-tracer resolution |
| `vsync` | `true` | Vertical sync |
| `model_path` | not set | glTF file to show (`.gltf` or `.glb`). Its point lights (`KHR_lights_punctual`) become sphere lights with `light_radius`; they do not move. Not set: a generated test scene (cube, sphere, glass ball, two mirrors) |
| `room_width`, `room_depth`, `room_height` | 8.0, 8.0, 3.0 | Inner size of the room in meters (X, Z, Y) |
| `camera_position` | `[0.0, 9.0, 7.0]` | Camera position in meters (X, Y up, Z). The room floor has its center at the origin |
| `camera_target` | `[0.0, 0.5, 0.0]` | The point that the camera looks at |
| `camera_fov_degrees` | 45.0 | Vertical field of view |
| `light_position` | `[0.0, 2.0, 2.0]` | Start position of the light in meters |
| `light_radius` | 0.15 | Radius of the sphere light in meters. A larger light gives softer shadows |
| `light_color` | `[1.0, 0.9, 0.8]` | Linear RGB |
| `light_intensity` | 12.0 | A surface that faces the light at `d` meters gets `color * intensity / d²`. Does not change with the radius |
| `max_samples` | 1024 | The path tracer stops when the picture is the mean of this many samples |
| `samples_per_frame` | 0 | Light paths for each pixel in one frame, 1 to 64. More samples cost more GPU time for each frame and give a clean picture in fewer frames. 0: automatic, the renderer fills `trace_budget_ms` |
| `trace_budget_ms` | 6.0 | GPU time of the path tracer in each frame, in milliseconds, for the automatic sample count. Keep it well below the time of one display refresh (16.7 ms at 60 Hz) |
| `light_history_samples` | 32 | When a light changes, the picture keeps at most this many old samples, and new samples replace them step by step. More: less noise while a light moves, and the lighting follows the light later. 0: a light change starts the picture again |
| `max_path_rays` | 8 | The largest number of rays in one light path, 2 to 32. Only mirror and glass make long paths. A lower limit costs less GPU time and gives less noise; the last mirror or glass surface of a cut path shows as a diffuse surface, so deep reflections lose detail. Measured in `docs/perf-improve.md`, section 0 |
| `denoise_passes` | 4 | Filter passes of the denoiser, 0 to 6. Each pass doubles the area that the filter takes its mean from. 0: no denoiser. The filter does less as the picture gets more samples |
| `exposure` | 1.0 | The picture is multiplied by this before the tone curve |
| `fixed_hz` | 60.0 | Rate of the simulation steps (`FixedUpdate`), in steps per second |
| `max_fixed_steps` | 8 | Limit of simulation steps in one frame |

## Picture

The picture is path traced. Each frame traces one random light path for each pixel. The path starts with a camera ray. What happens at a surface depends on the material of the mesh:

| Material | The ray |
|---|---|
| Diffuse | One shadow ray goes to a random point of the sphere light (direct light, soft shadows). Then one bounce ray goes in a random direction; the path ends at the second diffuse surface (the light that fills the shadows). |
| Mirror | Continues in the mirror direction. |
| Glass | Is reflected or goes through and bends, at random, with the chance from the Fresnel law. |

A path has 12 rays at most. The base color of the mesh is the color of a diffuse surface and the tint of a mirror or of glass. The light shows as a white sphere, also in a mirror and through glass.

A mesh made in code gets its material with `Mesh::with_material`. A mesh from a glTF file gets it from the numbers of its glTF material:

| glTF material | Material |
|---|---|
| Transmission factor above 0.5 (`KHR_materials_transmission`) | Glass. The index of refraction is the `ior` of `KHR_materials_ior`, or 1.5 |
| Else: metallic factor 0.5 or more and roughness factor 0.1 or less | Mirror |
| All others, with the glTF default (a rough metal) | Diffuse |

Textures are not read.

Known limit: light that reaches a diffuse surface by way of a mirror or through glass (a caustic) shows as bright speckle that goes away very slowly. The shadow ray finds only light that comes straight from the light.

One frame is noisy. The renderer keeps the mean of all frames since the last reset, so the picture gets clean while nothing changes. A change of the camera, a light, a mesh instance, the mesh store, the internal resolution, or the window size starts the mean again, and so does R. At `max_samples` frames the renderer stops the path tracer: only the tonemap pass and the copy to the window run.

A change of a light alone keeps the last `light_history_samples` samples: the new lighting replaces the old one over some frames, with much less noise than a new start. Pixels that showed the light itself start again, so the light leaves no bright trail.

The denoiser (`denoise_passes`) then removes noise from the mean: each pixel gets the mean of the pixels around it that show the same surface (equal normal and distance) with a similar brightness. It filters the light without the surface color, so surface detail stays sharp. The less noise the mean has, the less the filter does.

The camera rays do not hit the back side of a triangle, so the camera looks into the room through the near walls. For all other rays the walls are solid.

## Crates

Dependencies flow in one direction: each crate can use only the crates above it in this list.

| Crate | Role |
|---|---|
| `crates/core` | Config, app and schedules, time, transform |
| `crates/platform` | Window, input actions, input capture |
| `crates/assets` | Asset stores and handles, mesh asset with its material, glTF loader, mesh generators |
| `crates/gpu` | Vulkan backend (resources, bindless set, acceleration structures, compute dispatch, 2D draw calls); the only crate that shows Vulkan types |
| `crates/render` | Render world, frame graph, render passes (path tracer, accumulation, denoiser, tonemap), camera, lights, meshes and their acceleration structures |
| `crates/ui` | egui integration, panel registry, the overlay render pass, help and stats panels |
| `game` | The binary |
