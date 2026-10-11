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
- Frame graph barriers: with `MAKI_LOG=debug`, the log shows the barrier list each time it changes.

## Config

| Key | Default | Meaning |
|---|---|---|
| `window_title` | `maki` | Window title |
| `window_width`, `window_height` | 1280, 720 | Window size, in logical pixels |
| `render_width`, `render_height` | 640, 360 | Internal path-tracer resolution |
| `vsync` | `true` | Vertical sync |
| `model_path` | not set | glTF file to show |

## Crates

Dependencies flow in one direction: each crate can use only the crates above it in this list.

| Crate | Role |
|---|---|
| `crates/core` | Config, shared types |
| `crates/platform` | Window and input actions |
| `crates/assets` | Asset loading (glTF) |
| `crates/gpu` | Vulkan backend; the only crate that shows Vulkan types |
| `crates/render` | Render world, frame graph, render passes |
| `crates/ui` | egui panels |
| `game` | The binary |
