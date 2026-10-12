# maki-game-engine

Top-down 3D horror game engine with path-traced lighting, written in Rust.

## Required tools

- Rust (stable), installed with [rustup](https://rustup.rs).
- [Vulkan SDK](https://vulkan.lunarg.com) 1.3 or later. It supplies `glslc` and the validation layers.

## Build and run

```
cargo build
cargo run -p maki-game                 # uses ./config.toml; run it in the repository root (room files)
cargo run -p maki-game -- my.toml      # uses the given config file
cargo run -p maki-game --release -- --bench-frames 300   # benchmark, see below
```

If the config file does not exist, the game writes one with default values.

- Log level: set `MAKI_LOG` (default `info`). Example: `MAKI_LOG=debug`.
- Tracy profiler: `cargo run -p maki-game --features profile`. Each schedule is a zone with the name of the schedule (`Input`, `FixedUpdate`, `Update`, `Extract`, `Render`, `Present`). Each frame graph pass is a zone with the name `pass`; the zone text has the pass name. The zones show CPU time. With `MAKI_LOG=trace`, the log lines show the same spans.
- Render self-test: set `MAKI_RENDER_SELFTEST`. The internal resolution changes every 7 frames; validation must stay clean.
- Scene counts: the log shows each mesh upload (vertices, indices) and a `render scene` line (meshes, instances, triangles) each time the counts change.
- Frame graph barriers: with `MAKI_LOG=debug`, the log shows the barrier list each time it changes.
- Accumulation: with `MAKI_LOG=debug`, the log shows `accumulation complete` when the sample count reaches `max_samples`, and `accumulation starts again` when a count of two or more frames starts again. With `MAKI_LOG=info,maki_render=trace`, it shows the cause of each change (`the camera changed`, `a light changed`, `an instance changed`).
- Events: with `MAKI_LOG=debug`, the log shows each event that starts and each message.

## Controls

The help panel in the game lists all controls, with the key that hides and shows the panel. It is open at start. The list is made from the binding table (`Bindings::default` in `crates/platform/src/input.rs`): to change a key, change the table.

The move keys walk the player; forward is away from the camera. Shift runs. Z or Enter is "interact": it starts the event that the player faces, and it closes a message. While an event runs (a message, a fade), the player does not move.

Debug keys: L gives the move keys to the first light of the room (and back), Q and E move that light down and up, one wheel step changes its intensity by 10 %. O opens or closes all doors. F5 reads the room file again. R starts the picture again.

## Debug panels

The panels are egui windows on top of the picture. Drag the title bar to move a panel, click the arrow to fold it.

| Panel | Content |
|---|---|
| Help | Key and description of each binding |
| Stats | Frame time (mean), FPS, sample count, internal resolution, player position, position and intensity of each light, GPU memory, GPU time of each frame graph pass |
| Debug | The view (final, one raw frame, motion vectors, depth, normals, surface color, history length, variance, direct light, indirect light before and after its filter, specular light) and the config values that have an effect during a run: vsync, render scale, exposure, path tracer, denoiser, camera, player speeds. Not saved to the config file |
| Bench | Starts a benchmark run (frames, script, light, picture dump) in the game that runs. Writes `bench.csv`; the game goes on. For numbers to compare between runs, use the command line |
| Game state | Flags and variables of the events, with a field to add one; the events that run |
| Message | The text of a `Message` command. Open only while a message waits |

While the mouse pointer is on a panel, the mouse wheel starts no action. While a text field of a panel has the keyboard focus, keys start no action.

To add a panel from a module, call `maki_ui::add_panel(app, "Name", true, |ui, world| { ... })` in `Module::build`. The closure gets the egui `Ui` of the window and the game world. `crates/ui/src/stats.rs` is an example.

## Benchmark

```
cargo run -p maki-game --release -- [CONFIG] --bench-frames N [--bench-dump FILE [--bench-shot]] [--bench-static | --bench-light-frames M] [--bench-script walk|trip]
```

The game runs N frames (1 to 1000000), writes `bench.csv` in the working directory, and stops. In this mode:

- Each frame counts as one fixed step of game time (1/60 s), whatever the real time was: the simulation is the same in each run.
- The first light of the room moves on a circle around the room center, one turn in 240 frames. The position depends only on the frame number, so each run does the same work. The keyboard does not move the light.
- The light moves in each frame, so each frame is a change of the lighting: the indirect light of each pixel keeps `light_history_samples` old samples. `--bench-static` holds the light at the start of the path, so the picture gets all its samples. `--bench-light-frames M` moves the light in M frames, then holds it: the frames after that show how fast the picture follows.
- `--bench-script NAME` holds the light and plays the game with a script of actions (`game/src/bench.rs`). `walk`: in room A the player walks and a door opens; the last motion shows in frame 180. `trip`: the full loop, 820 frames: room A, room B, the switch, back to room A, through the door that was locked.
- `tools/bench.py` sets `samples_per_frame = 1` if the scenario does not set it: the automatic count would give each run another load.
- `--bench-dump FILE` writes the picture of frame N (the picture before the tone curve, at the output resolution) to FILE: two little-endian `u32` (width, height), then four `f32` (R, G, B, A) for each pixel. The picture depends only on the config, the scene, and N. `python tools/bench.py same` compares two dumps.
- `--bench-shot` (with `--bench-dump`): the file gets the picture as the window shows it (after the tone curve), and the run hides all panels. `python tools/shot.py OUT.png [--room room_b.ron --spawn west] [--set key=value ...]` runs the game that way and writes a PNG.
- `vsync` is off, whatever the config file says.

`bench.csv` has one row for each frame:

| Column | Content |
|---|---|
| `frame` | Frame number, from 1 |
| `cpu_ms` | Time from the start of the frame to the start of the next one. It has all schedules, and the time that the CPU waits for the GPU and for the display |
| `room_load_ms` | For a frame that loaded a room: the time of the load (the room file, the generated meshes, the upload to the GPU). Else 0 |
| `gpu_<pass>_ms` | GPU time of each frame graph pass, in frame order (GPU timestamp queries). The time of a pass has the image barriers before it |

The first frames are slow (startup work): drop them when you compute a mean. An empty cell means that the value did not arrive (a skipped frame, or a GPU without timestamp queries). A debug build has the validation layers on, which makes `cpu_ms` much larger.

## Config

| Key | Default | Meaning |
|---|---|---|
| `window_title` | `maki` | Window title |
| `window_width`, `window_height` | 1280, 720 | Window size, in logical pixels |
| `render_scale` | 1.0 | Internal path-tracer resolution: the window size multiplied by this, 0.1 to 1.0 |
| `render_width`, `render_height` | 0, 0 | A fixed internal resolution that does not follow the window. 0: use `render_scale` |
| `vsync` | `true` | Vertical sync |
| `start_room` | `game/assets/rooms/room_a.ron` | The room file that the game starts in. A path that is not absolute starts at the working directory |
| `start_spawn` | `start` | The spawn point of that room where the player starts |
| `camera_position` | `[0.0, 9.0, 7.0]` | With `camera_target`: the view direction and the distance of the camera. The camera keeps them when it follows the player |
| `camera_target` | `[0.0, 0.5, 0.0]` | The height of the point that the camera looks at is the Y of this point |
| `camera_fov_degrees` | 45.0 | Vertical field of view |
| `camera_follow_time` | 0.25 | The camera target moves most of the way to the player in this time, in seconds. 0: no delay |
| `camera_margin` | 3.0 | The camera target stays this far from the walls of the room, in meters |
| `player_model_path` | not set | glTF file of the player, with the feet at its origin. Not set: a capsule |
| `player_walk_speed`, `player_run_speed` | 1.6, 3.6 | Speed of the player, in meters per second |
| `max_samples` | 1024 | The mean of a pixel has at most this many samples. While nothing changes, the path tracer stops when each pixel has them |
| `samples_per_frame` | 0 | Light paths for each pixel in one frame, 1 to 64. More samples cost more GPU time for each frame and give a clean picture in fewer frames. 0: automatic, the renderer fills `trace_budget_ms` |
| `trace_budget_ms` | 6.0 | GPU time of the path tracer in each frame, in milliseconds, for the automatic sample count. Keep it well below the time of one display refresh (16.7 ms at 60 Hz) |
| `light_history_samples` | 16 | When the lighting changes (a light, or an object that moves), the indirect light of each pixel keeps at most this many old samples for some frames, and new samples replace them. More: less noise while something moves, and the bounce light follows later. 0: a change of the lighting starts the indirect light again |
| `history_clamp` | 2.0 | History test of the direct light, in standard deviations of the noise. The denoiser keeps a second, short mean of the direct light (4 frames). Where the short and the long means of a block of pixels differ by more than this, the lighting changed there, and the long mean starts again from the short one: a shadow follows its object in few frames. 0: no test; a change of the lighting then cuts the direct light to `light_history_samples` too |
| `taa` | true | Temporal anti-aliasing. Each frame moves all camera rays by a part of a pixel, and the picture is the mean of some frames: smooth edges, and with `render_scale` below 1 more detail than one frame has. False: stairs at each edge |
| `taa_blend` | 0.1 | The part of a new frame in the picture of the anti-aliasing, at rest. Lower: smoother edges. 0: the plain mean of all frames since the last change, for a reference picture of a scene at rest |
| `max_path_rays` | 8 | The largest number of rays in one light path, 2 to 32. Only mirror and glass make long paths. A lower limit costs less GPU time and gives less noise; the last mirror or glass surface of a cut path shows as a diffuse surface, so deep reflections lose detail. Measured in `docs/perf-improve.md`, section 0 |
| `max_bounces` | 1 | A light path ends after this many diffuse bounces, 1 to 16. More: nearer to the light of a real room, for one more ray and more noise for each bounce. 2 with `bounce_light` is 20 times nearer to a picture with 8 bounces than 1 alone, for 16 % more GPU time |
| `bounce_light` | true | Light of many bounces for the cost of one: the last surface of a path also counts the bounce light that the same point had in the picture of the frame before, if that picture showed it. Corners that no lamp sees get their light. False: such corners stay too dark |
| `blue_noise` | true | The random numbers for the point on the light and for the bounce direction are spread evenly over the pixels (a blue-noise pattern), so the noise of one frame has no clumps. Measured: 6 to 10 % less error for a young picture, 1 % in motion |
| `denoise_passes` | 4 | Filter passes of the denoiser for the indirect light (bounce light), 0 to 6. Each pass doubles the area that the filter takes its mean from. 0: no filter, only the mean over frames |
| `denoise_direct_passes` | 1 | The same for the direct light. It has little noise and sharp shadow edges. A scene with large lights (wide soft shadows) can need more |
| `denoise_specular_passes` | 2 | The same for the specular light: the reflection of a rough surface (metal, plastic). The reflection of a smooth surface and of glass gets no filter |
| `denoise_reach` | 4096.0 | The filter gets narrower as the mean of a pixel gets more samples: a pass with `s` pixels between its points runs for a pixel while `s * s * samples` is at most this. 0: each pass runs at each pixel |
| `indirect_clamp` | 8.0 | The largest light that one bounce ray adds to a pixel, in units of the light at the surface that the pixel shows. It removes the bright speckle of rare paths, and it makes light that comes by way of a mirror or through glass (caustics) darker. 0: no limit |
| `exposure` | 1.0 | The picture is multiplied by this before the tone curve. Manual: an automatic exposure would take away the darkness that the game wants |
| `tone_curve` | `"agx"` | The curve from light to display color. `"agx"`: the look of film, soft colors and contrast; a bright colored light goes to white with no change of its hue. `"neutral"` (Khronos PBR Neutral): colors as the scene has them, more contrast |
| `bloom` | 0.04 | The part of a wide blur of the picture that is blended into it, 0 to 1: a soft glow around the lamp and its reflections. 0: none |
| `vignette` | 0.25 | How dark the corners of the picture get, 0 (not at all) to 1 (black) |
| `grade_lift`, `grade_gamma`, `grade_gain` | `[0, 0, 0]`, `[1, 1, 1]`, `[1, 1, 1]` | The color grade, each for R, G, B: lift moves black toward a color, gamma bends the middle tones, gain scales white. The defaults change nothing |
| `fixed_hz` | 60.0 | Rate of the simulation steps (`FixedUpdate`), in steps per second |
| `max_fixed_steps` | 8 | Limit of simulation steps in one frame |

## Picture

The picture is path traced. Each frame traces one random light path for each pixel. The path starts with a camera ray. What happens at a surface depends on the material of the mesh:

| Material | The ray |
|---|---|
| Diffuse | One shadow ray goes to a random point of the sphere light (direct light, soft shadows). Then one bounce ray goes in a random direction; the path ends at the second diffuse surface (the light that fills the shadows). |
| Mirror | Continues in the mirror direction. |
| Glass | Glass that the camera sees (also in a mirror) splits the path: one path goes through and bends, one is reflected, each with its part of the light from the Fresnel law. Other glass on a path: the ray takes one of the two ways at random. |
| Metal (roughness) | If the pixel shows the surface: the soft reflection of many small mirrors (GGX). One shadow ray gives the bright spot of a light, and the path continues in a reflected direction; the two share the light of a lamp by their chances, so a small lamp on a smooth surface and a large one on a rough surface both have little noise. The base color tints the reflection. Later on a path: as diffuse. |
| Plastic (roughness) | A diffuse surface under a clear rough coat: the reflection of metal, with no tint and some percent of the light (more at a flat angle), plus the diffuse light. |

A path has 12 rays at most. The base color of the mesh is the color of a diffuse surface and the tint of a mirror or of glass. The light shows as a white sphere, also in a mirror and through glass.

A mesh made in code gets its material with `Mesh::with_material`. A mesh from a glTF file gets it from the numbers of its glTF material:

A mesh can have a light of its own (`Mesh::with_emissive`, `look: (emissive: (r, g, b))` in a room file, the emissive factor of a glTF material): it shows with that color, and bounce rays that find it bring its light to the room. No shadow ray aims at it, so it gives soft, noisy light: for a strong light use a `Light`.

A `Light` is a sphere. With `direction` and `cone` (degrees from the direction to the edge) in the room file it sends its light only in that cone: a flashlight. `game/assets/rooms/room_c.ron` shows the rough materials, a mesh with its own light, and a cone light (`start_room` of the config).

| glTF material | Material |
|---|---|
| Transmission factor above 0.5 (`KHR_materials_transmission`) | Glass. The index of refraction is the `ior` of `KHR_materials_ior`, or 1.5 |
| Else: roughness factor 0.9 or more (also the glTF default, a rough metal) | Diffuse |
| Else: metallic factor 0.5 or more | Mirror with a roughness factor of 0.1 or less, else Metal with that roughness |
| All others | Plastic with that roughness (0.05 at least) |

Textures are not read.

Known limit: light that reaches a diffuse surface by way of a mirror or through glass (a caustic) is too dark. The shadow ray finds only light that comes straight from the light, so such light comes from a bounce ray that finds the light by chance: a rare, very bright sample. `indirect_clamp` limits it. With `indirect_clamp = 0` the light is correct in the mean, and shows as bright speckle that goes away very slowly.

One frame is noisy. The denoiser makes the picture from the frames (`crates/render/src/denoise.rs`). The path tracer gives the light of each pixel in parts, and the picture is `surface color * (direct + indirect) + specular + emission`:

- Direct light: straight from the lights. Little noise, sharp shadows.
- Indirect light: by way of a bounce. Much noise, soft.
- Specular light: the reflection on glass.
- Emission: a lamp that the pixel shows (also in a mirror, through glass, or as a reflection on glass). No noise: it does not go through the denoiser.

For a pixel that shows a mirror or glass, the surface is the one that shows there: the denoiser treats the room in a mirror as it treats the room.

The steps:

1. The mean over time. Each pixel keeps the mean of its samples, up to `max_samples`. The renderer knows where the surface point of each pixel was on the screen one frame before (the motion image), so the mean follows a moving object and a moving camera. Where the frame before showed another surface, the pixel starts again.
2. The filter, for the direct light (`denoise_direct_passes`) and for the indirect light (`denoise_passes`), each alone: the noise of the bounce light does not blur the shadow edges of the direct light. Each pixel gets the mean of the pixels around it that show the same surface (equal normal, on the same plane) with a similar brightness. The less noise a pixel has left, the less the filter does. The filter works on the light without the surface color, so surface detail stays sharp. The specular light gets the mean over time only.

A change of the camera keeps the samples of each pixel. A change of the lighting (a light, an object that moves and so its shadow) is handled for each kind of light. Direct light: the denoiser tests each pixel (`history_clamp`), so only the pixels where the light changed start again, and a shadow follows its object in some frames. Indirect light: its noise is too large for such a test at one sample for each frame, so its mean is cut to `light_history_samples` in the full picture for some frames. A change of the internal resolution, the window size, the room, or the mesh store starts the picture again, and so does R. While nothing changes and each pixel has `max_samples`, only the tonemap pass and the copy to the window run.

3. Anti-aliasing and scaling (`taa`, `crates/render/src/taa.rs`). The trace pass moves all camera rays of a frame by one part of a pixel. The pass `taa` keeps the mean of the pictures of some frames at the window resolution: an edge gets smooth, also that of the lamp and of the background, which have no mean in the denoiser. With `render_scale` below 1 it puts each sample at its true place in the window grid. A point that moves on the screen takes more of the new frame, and its history counts only if it shows the same surface. This pass is the one step between the internal resolution and the window resolution: an upscaler (FSR, XeSS, MetalFX, DLSS) takes its place with the same inputs.
4. The look (`bloom`, `tonemap`): a soft glow around the bright parts (`bloom`), darker corners (`vignette`), the exposure and the tone curve (`tone_curve`), the color grade, the sRGB curve, and dither: noise of one 8-bit step, so a dark gradient shows no bands.

The camera rays do not hit the back side of a triangle, so the camera looks into the room through the near walls. For all other rays the walls are solid.

## Rooms and events

A room is a RON file in `game/assets/rooms`: the size of the room, its doorways, props, lights, doors, movers, spawn points, and events. `RoomDef` in `crates/scene/src/room.rs` is the format of the room, `EventDef` in `crates/events/src/model.rs` that of an event. A change of a room file needs no build: start the game again, or press F5.

- A bad room file: the error names the file, the line, and the column. A bad start room stops the game at start; a transfer into a bad room does not happen.
- A generated mesh (the walls, a cube, a sphere) belongs to its room and goes away with it. A glTF file (`File("path")`, from the directory of the room file) is loaded one time.
- The player, the camera, and the game state (flags and variables) are not part of a room: they stay when the room changes.

An event has pages. A page has conditions (flags, variables), a trigger, and commands. The active page is the last one whose conditions hold.

| Trigger | The commands start when |
|---|---|
| `Interact` | the player is at the area of the event, faces it, and presses interact |
| `Touch` | the player walks into the area of the event |
| `Autorun` | the page becomes the active page (also when the room loads). One time. The player has no control while it runs |
| `Parallel` | as `Autorun`, and again each time the commands end. The player keeps control |

| Command | Effect |
|---|---|
| `SetFlag("name", true)`, `SetVariable("name", 3)`, `AddVariable("name", 1)` | Change the game state |
| `Wait(0.5)` | Seconds |
| `If("flag", [...], [...])` | The first list if the flag is set, else the second one |
| `OpenDoor("name")`, `CloseDoor("name")` | Send a door or a mover to its other pose, and wait until it is there |
| `LightOn("name")`, `LightOff("name")`, `LightIntensity("name", 4.0)` | Change a light of the room |
| `Message("text")` | Show the text until the player presses interact |
| `FadeOut(0.4)`, `FadeIn(0.4)` | To black and back, in seconds |
| `Transfer("room_b.ron", "west")` | To a spawn point of another room. The event goes on in the new room |

The interpreter runs the commands in fixed steps and never stops the frame loop: a command that waits is tried again in the next step.

## Crates

Dependencies flow in one direction: each crate can use only the crates above it in this list.

| Crate | Role |
|---|---|
| `crates/core` | Config, app and schedules, time, transform, interpolation between fixed steps |
| `crates/platform` | Window, input actions, input capture |
| `crates/assets` | Asset stores and handles, mesh asset with its material, glTF loader, mesh generators |
| `crates/scene` | The components that describe the world (mesh instance, light, camera, player, collider, mover, door), collision, the room file format, load and unload of rooms |
| `crates/events` | Game state (flags, variables), event definitions, the event interpreter |
| `crates/gpu` | Vulkan backend (resources, bindless set, acceleration structures, compute dispatch, 2D draw calls); the only crate that shows Vulkan types |
| `crates/render` | Render world, frame graph, render passes (path tracer, denoiser, anti-aliasing, bloom, tonemap), extraction of camera, lights, and mesh instances, acceleration structures, motion vectors |
| `crates/ui` | egui integration, panel registry, the overlay render pass, help and stats panels |
| `game` | The binary: player, camera, controls, debug panels, benchmark. The rooms are in `game/assets/rooms` |
