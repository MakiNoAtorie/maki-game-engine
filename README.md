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
```

If the config file does not exist, the game writes one with default values.

- Log level: set `MAKI_LOG` (default `info`). Example: `MAKI_LOG=debug`.
- Tracy profiler: `cargo run -p maki-game --features profile`.
- Render self-test: set `MAKI_RENDER_SELFTEST`. The internal resolution changes every 7 frames; validation must stay clean.
- Scene counts: the log shows each mesh upload (vertices, indices) and a `render scene` line (meshes, instances, triangles) each time the counts change.
- Frame graph barriers: with `MAKI_LOG=debug`, the log shows the barrier list each time it changes.
- Accumulation: with `MAKI_LOG=debug`, the log shows `accumulation complete` when the sample count reaches `max_samples`, and `accumulation reset` when a mean of two or more samples starts again.

## Controls

| Key | Action |
|---|---|
| W A S D, arrow keys | Move the light along the floor. Forward is away from the camera |
| Q, E | Move the light down, up |
| Mouse wheel | Light intensity, 10 % for each step |
| R | Reset the accumulation |
| Esc | Quit |

The light stays in the room.

## Config

| Key | Default | Meaning |
|---|---|---|
| `window_title` | `maki` | Window title |
| `window_width`, `window_height` | 1280, 720 | Window size, in logical pixels |
| `render_width`, `render_height` | 640, 360 | Internal path-tracer resolution |
| `vsync` | `true` | Vertical sync |
| `model_path` | not set | glTF file to show (`.gltf` or `.glb`). Not set: a generated test scene (cube, sphere, glass ball, two mirrors) |
| `room_width`, `room_depth`, `room_height` | 8.0, 8.0, 3.0 | Inner size of the room in meters (X, Z, Y) |
| `camera_position` | `[0.0, 9.0, 7.0]` | Camera position in meters (X, Y up, Z). The room floor has its center at the origin |
| `camera_target` | `[0.0, 0.5, 0.0]` | The point that the camera looks at |
| `camera_fov_degrees` | 45.0 | Vertical field of view |
| `light_position` | `[0.0, 2.0, 2.0]` | Start position of the light in meters |
| `light_radius` | 0.15 | Radius of the sphere light in meters. A larger light gives softer shadows |
| `light_color` | `[1.0, 0.9, 0.8]` | Linear RGB |
| `light_intensity` | 12.0 | A surface that faces the light at `d` meters gets `color * intensity / d²`. Does not change with the radius |
| `max_samples` | 1024 | The path tracer stops when the picture is the mean of this many frames |
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

A mesh made in code gets its material with `Mesh::with_material`. A mesh from a glTF file is diffuse.

Known limit: light that reaches a diffuse surface by way of a mirror or through glass (a caustic) shows as bright speckle that goes away very slowly. The shadow ray finds only light that comes straight from the light.

One frame is noisy. The renderer keeps the mean of all frames since the last reset, so the picture gets clean while nothing changes. A change of the camera, a light, a mesh instance, the mesh store, the internal resolution, or the window size starts the mean again, and so does R. At `max_samples` frames the renderer stops the path tracer: only the tonemap pass and the copy to the window run.

The camera rays do not hit the back side of a triangle, so the camera looks into the room through the near walls. For all other rays the walls are solid.

## Crates

Dependencies flow in one direction: each crate can use only the crates above it in this list.

| Crate | Role |
|---|---|
| `crates/core` | Config, app and schedules, time, transform |
| `crates/platform` | Window and input actions |
| `crates/assets` | Asset stores and handles, mesh asset with its material, glTF loader, mesh generators |
| `crates/gpu` | Vulkan backend (resources, bindless set, acceleration structures); the only crate that shows Vulkan types |
| `crates/render` | Render world, frame graph, render passes (path tracer, accumulation, tonemap), camera, lights, meshes and their acceleration structures |
| `crates/ui` | egui panels |
| `game` | The binary |
