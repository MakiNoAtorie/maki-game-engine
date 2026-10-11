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

## Config

| Key | Default | Meaning |
|---|---|---|
| `window_title` | `maki` | Window title |
| `window_width`, `window_height` | 1280, 720 | Window size, in logical pixels |
| `render_width`, `render_height` | 640, 360 | Internal path-tracer resolution |
| `vsync` | `true` | Vertical sync |
| `model_path` | not set | glTF file to show (`.gltf` or `.glb`). Not set: a generated cube and sphere |
| `room_width`, `room_depth`, `room_height` | 8.0, 8.0, 3.0 | Inner size of the room in meters (X, Z, Y) |
| `camera_position` | `[0.0, 9.0, 7.0]` | Camera position in meters (X, Y up, Z). The room floor has its center at the origin |
| `camera_target` | `[0.0, 0.5, 0.0]` | The point that the camera looks at |
| `camera_fov_degrees` | 45.0 | Vertical field of view |

The picture shows the world normal of each surface as a color (`0.5 + 0.5 * normal`): a floor is green, a wall that faces +X is red, a wall that faces +Z is blue. The camera rays do not hit the back side of a triangle, so the camera looks into the room through the near walls.

## Crates

Dependencies flow in one direction: each crate can use only the crates above it in this list.

| Crate | Role |
|---|---|
| `crates/core` | Config, app and schedules, time, transform |
| `crates/platform` | Window and input actions |
| `crates/assets` | Asset stores and handles, mesh asset, glTF loader, mesh generators |
| `crates/gpu` | Vulkan backend (resources, bindless set, acceleration structures); the only crate that shows Vulkan types |
| `crates/render` | Render world, frame graph, render passes, camera, meshes and their acceleration structures |
| `crates/ui` | egui panels |
| `game` | The binary |
