//! Benchmark mode (`--bench-frames N`): the first light of the room moves on a fixed path for N frames,
//! then the game writes the timing of each frame to a CSV file and stops.
//!
//! The "Bench" panel starts the same run in a game that runs, and the game goes on
//! after it. Such a run starts from the state of the game at that moment, so only
//! the command line gives numbers to compare between runs. A run with a script
//! loads the start room again; the flags and variables stay.
//!
//! The path depends only on the frame number, so each run does the same work. The light
//! moves in each frame, so each frame starts the accumulation again: no frame gets the
//! low cost of a complete picture.
//!
//! Each frame counts as one fixed step of game time, whatever the real time was, so
//! the simulation is also the same in each run.
//!
//! With `--bench-script NAME` the light does not move, and a script plays the game. A
//! script holds actions down, as a player does with keys (`ScriptedActions`). The
//! scripts belong to the game: their frame numbers fit its room files. The game gives
//! them to `BenchModule`.
//!
//! The CSV has the column `room_load_ms`: for a frame that loaded a room, the time
//! of the load (file, meshes, GPU upload), else zero.
//!
//! With `--bench-light-frames N` the light stops after N frames: the frames after that
//! show how fast the picture follows a change of the lighting.
//!
//! With `--bench-static` the light stays at the start of the path, so the picture
//! accumulates. The path closes after `FRAMES_PER_TURN` frames: a run with a moving
//! light whose frame count is a multiple of that number ends with the light at the
//! same place.

use std::f32::consts::TAU;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Query, Res, ResMut};
use glam::Vec3;
use maki_core::{App, Input, Interpolated, Module, Startup, Time, Update, interpolate_transforms};
use maki_gameplay::{StartRoom, clamp_to_room};
use maki_platform::{Action, QuitRequested, ScriptedActions, WindowSettings};
use maki_render::{
    Format, FrameGraph, GpuMeshes, RenderGpu, RenderWorld, half_to_f32, setup_renderer,
};
use maki_scene::{CurrentRoom, Light, RoomDef, RoomRequest, load_requested_room};
use maki_ui::{Panels, add_panel, egui};

use crate::stages::RoomDirs;

/// The options of the command line that `parse_args` reads.
pub const USAGE: &str = "[--bench-frames N [--bench-dump FILE [--bench-shot]] [--bench-static | --bench-light-frames N] [--bench-script NAME]]";
/// `tools/bench.py` writes `<scenario>/room.ron` here. Relative to the working directory.
const BENCH_DIR: &str = "target/bench";

/// The CSV file, in the working directory.
pub const OUTPUT_PATH: &str = "bench.csv";
/// The picture file of a run from the panel, in the working directory.
const PANEL_DUMP_PATH: &str = "bench.dump";
/// Frames for one turn of the light around the room.
const FRAMES_PER_TURN: u64 = 240;
/// The GPU times of a frame arrive some frames later. After the last bench frame, the
/// game waits this many frames for them, then stops without them.
const MAX_WAIT_FRAMES: u64 = 8;

/// Adds the "Bench" panel, which starts a benchmark run. Needs `RoomsModule` of the
/// gameplay crate (a script starts in the start room).
pub struct BenchModule {
    /// `Some`: this run starts with the game, and the game stops after it.
    pub start: Option<BenchRun>,
    /// The scripts of the game, each with its name for the command line and the panel.
    pub scripts: Scripts,
}

/// One benchmark run.
#[derive(Debug, Clone)]
pub struct BenchRun {
    pub frames: u64,
    /// `Some`: write the picture of the last bench frame to this file. See `dump_picture`.
    pub dump: Option<PathBuf>,
    /// True: the file gets the picture as the window shows it (after the tone curve),
    /// and the run hides all panels. False: the picture before the tone curve.
    pub shot: bool,
    /// `Some`: the light moves in this many frames, then it stops. `Some(0)`: it does
    /// not move.
    pub light_frames: Option<u64>,
    /// `Some`: the script runs, and the light does not move.
    pub script: Option<Script>,
}

/// A script: from each frame number on, the actions that are held down. The actions
/// of frame N move the player in the fixed step of frame N + 1, and the picture shows
/// that step in frame N + 2 (see `maki_core::interpolation`).
pub type Script = &'static [(u64, &'static [Action])];

/// Scripts by name.
pub type Scripts = &'static [(&'static str, Script)];

fn find_script(scripts: Scripts, name: &str) -> Option<Script> {
    let named = scripts.iter().find(|(script, _)| *script == name);
    named.map(|(_, script)| *script)
}

/// Takes the benchmark options out of a command line (without the program name).
/// Returns the run that they ask for, and the arguments that are left for the game.
pub fn parse_args(
    args: impl IntoIterator<Item = OsString>,
    scripts: Scripts,
) -> anyhow::Result<(Option<BenchRun>, Vec<OsString>)> {
    let mut args = args.into_iter();
    let (mut frames, mut dump, mut shot) = (None, None, false);
    let (mut fixed_light, mut script, mut light_frames) = (false, None, None);
    let mut rest = Vec::new();
    while let Some(arg) = args.next() {
        if arg == "--bench-frames" {
            // The limit keeps a typing error from a table of rows that fills the memory.
            let count = args
                .next()
                .and_then(|value| value.to_str()?.parse::<u64>().ok())
                .filter(|frames| (1..=1_000_000).contains(frames))
                .context("--bench-frames needs a number from 1 to 1000000")?;
            frames = Some(count);
        } else if arg == "--bench-static" {
            fixed_light = true;
        } else if arg == "--bench-shot" {
            shot = true;
        } else if arg == "--bench-light-frames" {
            let count = args.next().and_then(|frames| frames.to_str()?.parse().ok());
            light_frames = Some(count.context("--bench-light-frames needs a number")?);
        } else if arg == "--bench-script" {
            let name = args.next().and_then(|name| name.into_string().ok());
            let found = name.and_then(|name| find_script(scripts, &name));
            let names: Vec<_> = scripts.iter().map(|(name, _)| *name).collect();
            script = Some(found.with_context(|| {
                format!("--bench-script needs a script name: {}", names.join(", "))
            })?);
        } else if arg == "--bench-dump" {
            let path = args.next().context("--bench-dump needs a file")?;
            dump = Some(PathBuf::from(path));
        } else {
            rest.push(arg);
        }
    }
    let Some(frames) = frames else {
        if dump.is_some() || fixed_light || shot || script.is_some() || light_frames.is_some() {
            bail!("the --bench options need --bench-frames");
        }
        return Ok((None, rest));
    };
    let run = BenchRun {
        frames,
        dump,
        shot,
        // A light that does not move: one that stops before the first frame.
        light_frames: if fixed_light { Some(0) } else { light_frames },
        script,
    };
    Ok((Some(run), rest))
}

/// The actions that a script holds in a frame.
fn script_actions(script: Script, frame: u64) -> &'static [Action] {
    let passed = script.iter().take_while(|(first, _)| *first <= frame);
    passed.last().map_or(&[], |(_, actions)| actions)
}

fn run_script(
    time: Res<Time>,
    bench: Res<Bench>,
    room: Res<CurrentRoom>,
    players: Query<&Interpolated, bevy_ecs::query::With<maki_scene::Player>>,
    mut scripted: ResMut<ScriptedActions>,
) {
    // No run, or a run with no script: nothing is held, also after a run that stopped.
    let script = bench.active.as_ref().and_then(|active| {
        let script = active.run.script?;
        Some(script_actions(script, active.bench_frame(time.frame)))
    });
    let actions = script.unwrap_or(&[]);
    // Only a change is a write.
    if scripted.0.len() != actions.len() || !actions.iter().all(|a| scripted.0.contains(a)) {
        let player = players.iter().next().map(|pose| pose.current.translation);
        tracing::debug!(
            frame = time.frame,
            ?actions,
            ?player,
            room = %room.path.display(),
            "script"
        );
        scripted.0 = actions.iter().copied().collect();
    }
}

impl Module for BenchModule {
    fn build(&self, app: &mut App) {
        // The generated scenes of `tools/bench.py` show in the room list.
        app.init_resource::<RoomDirs>();
        app.resource_mut::<RoomDirs>()
            .0
            .push((PathBuf::from(BENCH_DIR), 1));
        app.insert_resource(Bench {
            request: self.start.clone(),
            active: None,
            quit_at_end: self.start.is_some(),
            status: String::new(),
        })
        .add_systems(Input, record_frame)
        .add_systems(
            Update,
            (
                // The light jumps to its place of the frame: no blend between fixed steps.
                move_lights.before(interpolate_transforms),
                run_script,
            )
                .after(load_requested_room),
        );
        if self.start.is_some() {
            // `record_frame` sets this at the start of a run, and that is too late
            // for the first frame of the game.
            let step = app.world().resource::<Time>().fixed_delta;
            app.set_frame_time(Some(step));
            // With vsync, each frame would have the length of one display refresh.
            // Before the renderer makes the swapchain: `record_frame` is too late.
            app.add_systems(
                Startup,
                (|mut window: ResMut<WindowSettings>| window.vsync = false).before(setup_renderer),
            );
        }
        let scripts = self.scripts;

        // The values of the panel for the next run.
        let (mut frames, mut script_name, mut static_light, mut dump) = (300, None, false, false);
        add_panel(app, "Bench", true, move |ui, world| {
            let frame = world.resource::<Time>().frame;
            let bench = world.resource::<Bench>();
            let progress = bench.active.as_ref().map(|active| {
                let (frame, frames) = (active.bench_frame(frame), active.run.frames);
                format!("frame {} of {frames}", frame.min(frames))
            });
            let busy = progress.is_some() || bench.request.is_some();
            ui.label(progress.as_deref().unwrap_or(&bench.status));

            ui.add_enabled_ui(!busy, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut frames).range(1..=1_000_000));
                    ui.label("frames");
                });
                ui.horizontal(|ui| {
                    ui.label("script");
                    ui.radio_value(&mut script_name, None, "none");
                    for (name, _) in scripts {
                        ui.radio_value(&mut script_name, Some(*name), *name);
                    }
                });
                // A script holds the light.
                ui.add_enabled(
                    script_name.is_none(),
                    egui::Checkbox::new(&mut static_light, "the light does not move"),
                );
                ui.checkbox(
                    &mut dump,
                    format!("write the last picture to {PANEL_DUMP_PATH}"),
                );
                if ui.button("Start").clicked() {
                    world.resource_mut::<Bench>().request = Some(BenchRun {
                        frames,
                        dump: dump.then(|| PathBuf::from(PANEL_DUMP_PATH)),
                        shot: false,
                        light_frames: static_light.then_some(0),
                        script: script_name.and_then(|name| find_script(scripts, name)),
                    });
                }
            });
        });
    }
}

/// The measured values of one frame. `None`: no value (shown as an empty CSV cell).
#[derive(Debug, Clone, Default)]
struct Row {
    cpu_ms: Option<f32>,
    /// One value for each name in `Active::pass_names`.
    gpu_ms: Option<Vec<f32>>,
    /// Zero for a frame that loaded no room.
    room_load_ms: f32,
}

#[derive(Resource)]
struct Bench {
    /// This run starts at the start of the next frame.
    request: Option<BenchRun>,
    /// The run that is in progress.
    active: Option<Active>,
    /// True: the game stops after the run.
    quit_at_end: bool,
    /// The result of the last run, for the panel.
    status: String,
}

/// A run in progress. A bench frame is a frame of the run: the first one has the number 1.
struct Active {
    run: BenchRun,
    /// The number of the app frame before the first bench frame.
    frames_before: u64,
    /// Row `i` is for bench frame `i + 1`.
    rows: Vec<Row>,
    /// The passes of the frame graph, in frame order.
    pass_names: Vec<&'static str>,
    /// Number and start time of the bench frame before.
    last_start: Option<(u64, Instant)>,
    /// `CurrentRoom::loads` at the start of the frame before.
    loads: u64,
    /// The vsync value of the settings before the run.
    vsync: bool,
}

impl Active {
    /// The bench frame of an app frame. 0: a frame before the run.
    fn bench_frame(&self, app_frame: u64) -> u64 {
        app_frame.saturating_sub(self.frames_before)
    }

    fn row(&mut self, bench_frame: u64) -> Option<&mut Row> {
        self.rows
            .get_mut(usize::try_from(bench_frame.checked_sub(1)?).ok()?)
    }
}

/// The position of the light in a frame: a circle around the center of the room, at
/// half of the room height.
fn path_position(frame: u64, light_radius: f32, room: &RoomDef) -> Vec3 {
    let angle = (frame % FRAMES_PER_TURN) as f32 / FRAMES_PER_TURN as f32 * TAU;
    let reach = 0.3 * room.width.min(room.depth);
    let position = Vec3::new(angle.cos() * reach, room.height / 2.0, angle.sin() * reach);
    clamp_to_room(position, light_radius, room)
}

fn move_lights(
    time: Res<Time>,
    room: Res<CurrentRoom>,
    bench: Res<Bench>,
    // The first light of the room file: only those have a pose for fixed steps.
    mut lights: Query<(&mut Interpolated, &Light)>,
) {
    let Some(active) = &bench.active else { return };
    // The light path depends on the frame number, a script on game time: a picture
    // of the two together would differ from run to run.
    let frame = if active.run.script.is_some() {
        0
    } else {
        let frame = active.bench_frame(time.frame);
        frame.min(active.run.light_frames.unwrap_or(u64::MAX))
    };
    if let Some((mut pose, light)) = lights.iter_mut().next() {
        let position = path_position(frame, light.radius, &room.def);
        // A write marks the light as changed, and a changed light starts the picture again.
        if pose.current.translation != position {
            let mut next = pose.current;
            next.translation = position;
            pose.snap(next);
        }
    }
}

/// Runs at the start of each frame (`Input` is the first schedule). Starts a run,
/// records its frames, and stops it.
// What it writes for other systems (`Time::frame_time`, the settings, the room request)
// is read in a later schedule or in the next frame.
#[allow(clippy::too_many_arguments)]
fn record_frame(
    mut time: ResMut<Time>,
    mut config: ResMut<WindowSettings>,
    start_room: Res<StartRoom>,
    mut room_request: ResMut<RoomRequest>,
    graph: Option<Res<FrameGraph>>,
    gpu: Option<ResMut<RenderGpu>>,
    room: Res<CurrentRoom>,
    render_world: Option<Res<RenderWorld>>,
    mut bench: ResMut<Bench>,
    mut quit: ResMut<QuitRequested>,
    panels: Option<ResMut<Panels>>,
) {
    let now = Instant::now();
    let bench = &mut *bench;

    if bench.active.is_none()
        && let Some(run) = bench.request.take()
    {
        // Each frame counts as one fixed step, and no frame waits for the display.
        time.frame_time = Some(time.fixed_delta);
        let vsync = config.vsync;
        if vsync {
            config.vsync = false;
        }
        if let (true, Some(mut panels)) = (run.shot, panels) {
            panels.close_all();
        }
        if run.script.is_some() {
            // The frame numbers of a script start at the spawn point of the start room.
            room_request.0 = Some(start_room.target());
        }
        bench.active = Some(Active {
            frames_before: time.frame - 1,
            rows: vec![Row::default(); run.frames as usize],
            pass_names: Vec::new(),
            last_start: None,
            loads: room.loads,
            vsync,
            run,
        });
    }
    let Some(active) = &mut bench.active else {
        return;
    };
    let frame = active.bench_frame(time.frame);
    let frames = active.run.frames;

    // At the start of the frame after the last bench frame: the picture of that frame
    // is recorded and no newer frame has changed it. A fixed frame, so two runs dump
    // the same picture.
    if frame == frames + 1
        && let (Some(path), Some(graph), Some(mut gpu)) = (&active.run.dump, &graph, gpu)
    {
        let image = if active.run.shot {
            "output"
        } else {
            "resolved"
        };
        match dump_picture(path, graph, &mut gpu, image) {
            Ok(()) => tracing::info!(path = %path.display(), "picture dumped"),
            Err(error) => {
                tracing::error!(path = %path.display(), "cannot dump the picture: {error:#}")
            }
        }
    }

    // CPU time of a frame: from its start to the start of the next frame. It has all
    // schedules, and the time that the CPU waits for the GPU and for the display.
    // Not `Time::delta`: that value has an upper limit.
    // The frame before loaded a room: the time of the loader, and of the upload of
    // the meshes of the room in the same frame.
    let loaded = std::mem::replace(&mut active.loads, room.loads) != room.loads;
    let upload = render_world
        .as_ref()
        .and_then(|world| world.0.get_resource::<GpuMeshes>())
        .map_or(Duration::ZERO, GpuMeshes::last_sync_time);
    if let Some((before, start)) = active.last_start.replace((frame, now))
        && let Some(row) = active.row(before)
    {
        row.cpu_ms = Some((now - start).as_secs_f32() * 1000.0);
        if loaded {
            row.room_load_ms = (room.load_time + upload).as_secs_f32() * 1000.0;
        }
    }

    // The GPU times name their frame. They stay in the graph until newer ones arrive,
    // so the same values can show in more than one frame.
    if let Some(timings) = graph.as_ref().and_then(|graph| graph.timings()) {
        let timed = active.bench_frame(timings.frame);
        let names = timings.passes.iter().map(|(name, _)| *name).collect();
        if let Some(row) = active.row(timed)
            && row.gpu_ms.is_none()
        {
            row.gpu_ms = Some(timings.passes.iter().map(|(_, ms)| *ms).collect());
            active.pass_names = names;
        }
    }

    let complete = active.rows.last().is_some_and(|row| row.gpu_ms.is_some());
    if complete || frame > frames + MAX_WAIT_FRAMES {
        bench.status = match std::fs::write(OUTPUT_PATH, csv(&active.pass_names, &active.rows)) {
            Ok(()) => {
                tracing::info!(path = OUTPUT_PATH, frames, "benchmark written");
                format!("{OUTPUT_PATH} written: {frames} frames")
            }
            Err(error) => {
                tracing::error!(path = OUTPUT_PATH, %error, "cannot write the benchmark");
                format!("cannot write {OUTPUT_PATH}: {error}")
            }
        };
        // The game goes on as before the run. `run_script` releases the actions.
        time.frame_time = None;
        if active.vsync {
            config.vsync = true;
        }
        bench.active = None;
        quit.0 |= bench.quit_at_end;
    }
}

/// Writes the image `name` of the frame graph: `resolved` is the picture before the tone
/// curve (the result of the anti-aliasing pass), `output` is what the window shows. two
/// little-endian `u32` (width, height), then four `f32` (R, G, B, A) for each pixel.
/// `tools/bench.py` reads the file.
fn dump_picture(
    path: &Path,
    graph: &FrameGraph,
    gpu: &mut RenderGpu,
    name: &str,
) -> anyhow::Result<()> {
    let id = graph.resource(name).context("no picture image")?;
    let format = graph.transient_desc(id).context("not an image")?.format;
    let (image, (width, height), state) = graph.image(id).context("no frame was drawn")?;
    let pixels = gpu.0.read_image(image, state)?;
    let mut bytes = [width.to_le_bytes(), height.to_le_bytes()].concat();
    match format {
        Format::Rgba32Float => bytes.extend(pixels),
        Format::Rgba16Float => {
            let halves = pixels
                .as_chunks::<2>()
                .0
                .iter()
                .copied()
                .map(u16::from_ne_bytes);
            bytes.extend(halves.flat_map(|half| half_to_f32(half).to_le_bytes()));
        }
        // 0 to 255 as 0.0 to 1.0.
        Format::Rgba8Unorm => {
            let values = pixels.iter().map(|value| f32::from(*value) / 255.0);
            bytes.extend(values.flat_map(f32::to_le_bytes));
        }
        other => anyhow::bail!("cannot dump an image of format {other:?}"),
    }
    Ok(std::fs::write(path, bytes)?)
}

/// One line for each frame: frame number, CPU milliseconds, room load milliseconds, GPU
/// milliseconds of each pass.
fn csv(pass_names: &[&str], rows: &[Row]) -> String {
    let cell = |value: Option<f32>| value.map_or(String::new(), |value| format!("{value:.3}"));
    let mut text = String::from("frame,cpu_ms,room_load_ms");
    for name in pass_names {
        text += &format!(",gpu_{name}_ms");
    }
    text.push('\n');
    for (index, row) in rows.iter().enumerate() {
        let load = cell(Some(row.room_load_ms));
        text += &format!("{},{},{load}", index + 1, cell(row.cpu_ms));
        for pass in 0..pass_names.len() {
            let ms = row.gpu_ms.as_ref().and_then(|ms| ms.get(pass).copied());
            text += &format!(",{}", cell(ms));
        }
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_16_bit_float_gets_its_value() {
        assert_eq!(half_to_f32(0x0000), 0.0);
        assert_eq!(half_to_f32(0x3C00), 1.0);
        assert_eq!(half_to_f32(0xC000), -2.0);
        assert_eq!(half_to_f32(0x3555), 0.333_251_95);
        assert_eq!(half_to_f32(0x7BFF), 65504.0);
        assert_eq!(half_to_f32(0x0001), 2f32.powi(-24));
        assert_eq!(half_to_f32(0x7C00), f32::INFINITY);
        assert!(half_to_f32(0x7E00).is_nan());
    }

    const LEFT: Action = Action("left");
    const UP: Action = Action("up");
    const SCRIPT: Script = &[(40, &[LEFT]), (95, &[UP, LEFT]), (120, &[])];

    #[test]
    fn a_script_holds_the_actions_of_the_last_entry_that_started() {
        let held = |frame| script_actions(SCRIPT, frame);
        assert_eq!(held(1), &[]);
        assert_eq!(held(40), &[LEFT]);
        assert_eq!(held(94), &[LEFT]);
        assert_eq!(held(95), &[UP, LEFT]);
        // Nothing is held from the last entry on, and after the script.
        assert_eq!(held(120), &[]);
        assert_eq!(held(620), &[]);
    }

    #[test]
    fn the_command_line_gives_the_run_and_leaves_the_other_arguments() {
        let scripts: Scripts = &[("walk", SCRIPT)];
        let parse = |args: &[&str]| parse_args(args.iter().map(OsString::from), scripts);

        let (run, rest) = parse(&["my.toml"]).unwrap();
        assert!(run.is_none());
        assert_eq!(rest, ["my.toml"]);
        // In any order.
        for args in [
            ["my.toml", "--bench-frames", "300"],
            ["--bench-frames", "300", "my.toml"],
        ] {
            let (run, rest) = parse(&args).unwrap();
            let run = run.unwrap();
            assert_eq!((run.frames, run.light_frames), (300, None));
            assert!(run.dump.is_none() && run.script.is_none() && !run.shot);
            assert_eq!(rest, ["my.toml"]);
        }
        let run = parse(&[
            "--bench-frames",
            "3",
            "--bench-dump",
            "a.bin",
            "--bench-static",
        ]);
        let run = run.unwrap().0.unwrap();
        assert_eq!(run.dump, Some(PathBuf::from("a.bin")));
        assert_eq!(run.light_frames, Some(0));
        let run = parse(&["--bench-frames", "3", "--bench-script", "walk"]);
        assert!(run.unwrap().0.unwrap().script.is_some());
        for bad in [
            &["--bench-frames"][..],
            &["--bench-frames", "0"],
            &["--bench-frames", "many"],
            &["--bench-dump", "a.bin"],
            &["--bench-static"],
            &["--bench-script", "walk"],
            &["--bench-frames", "3", "--bench-script"],
            &["--bench-frames", "3", "--bench-script", "fly"],
            &["--bench-frames", "3", "--bench-dump"],
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn csv_has_one_line_for_each_frame_and_empty_cells_for_missing_values() {
        let rows = [
            Row {
                cpu_ms: Some(16.5),
                gpu_ms: Some(vec![0.25, 3.0]),
                room_load_ms: 0.0,
            },
            // The GPU times of this frame did not arrive. It loaded a room.
            Row {
                cpu_ms: Some(17.0),
                gpu_ms: None,
                room_load_ms: 2.5,
            },
        ];
        assert_eq!(
            csv(&["trace", "tonemap"], &rows),
            "frame,cpu_ms,room_load_ms,gpu_trace_ms,gpu_tonemap_ms\n\
             1,16.500,0.000,0.250,3.000\n2,17.000,2.500,,\n"
        );
    }

    #[test]
    fn the_light_path_is_the_same_in_each_run_and_stays_in_the_room() {
        let config = RoomDef {
            width: 8.0,
            depth: 6.0,
            height: 3.0,
            ..RoomDef::default()
        };
        let half = Vec3::new(config.width, 0.0, config.depth) / 2.0;
        for frame in 0..2 * FRAMES_PER_TURN {
            let position = path_position(frame, 0.15, &config);
            assert_eq!(position, path_position(frame, 0.15, &config));
            assert!(
                position.cmpge(-half).all() && position.cmple(half.with_y(config.height)).all()
            );
        }
        // The light moves, and the path closes after one turn.
        assert_ne!(
            path_position(1, 0.1, &config),
            path_position(2, 0.1, &config)
        );
        assert_eq!(
            path_position(7, 0.1, &config),
            path_position(7 + FRAMES_PER_TURN, 0.1, &config)
        );
    }
}
