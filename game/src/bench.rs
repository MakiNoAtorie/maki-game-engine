//! Benchmark mode (`--bench-frames N`): the light moves on a fixed path for N frames,
//! then the game writes the timing of each frame to a CSV file and stops.
//!
//! The path depends only on the frame number, so each run does the same work. The light
//! moves in each frame, so each frame starts the accumulation again and traces one
//! sample: no frame gets the low cost of a complete picture.

use std::f32::consts::TAU;
use std::time::Instant;

use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Query, Res, ResMut};
use glam::Vec3;
use maki_core::{App, Config, Input, Module, Time, Update};
use maki_platform::QuitRequested;
use maki_render::{FrameGraph, Light};

use crate::light::clamp_to_room;

/// The CSV file, in the working directory.
pub const OUTPUT_PATH: &str = "bench.csv";
/// Frames for one turn of the light around the room.
const FRAMES_PER_TURN: u64 = 240;
/// The GPU times of a frame arrive some frames later. After the last bench frame, the
/// game waits this many frames for them, then stops without them.
const MAX_WAIT_FRAMES: u64 = 8;

/// Runs the benchmark for this many frames. Use it in place of `LightControllerModule`:
/// the two would move the same light.
pub struct BenchModule {
    pub frames: u64,
}

impl Module for BenchModule {
    fn build(&self, app: &mut App) {
        app.insert_resource(Bench {
            rows: vec![Row::default(); self.frames as usize],
            pass_names: Vec::new(),
            last_start: None,
        })
        .add_systems(Input, record_frame)
        .add_systems(Update, move_lights);
    }
}

/// The measured values of one frame. `None`: no value (shown as an empty CSV cell).
#[derive(Debug, Clone, Default)]
struct Row {
    cpu_ms: Option<f32>,
    /// One value for each name in `Bench::pass_names`.
    gpu_ms: Option<Vec<f32>>,
}

#[derive(Resource)]
struct Bench {
    /// Row `i` is for frame `i + 1`: the first frame of the app has the number 1.
    rows: Vec<Row>,
    /// The passes of the frame graph, in frame order.
    pass_names: Vec<&'static str>,
    /// Number and start time of the frame before.
    last_start: Option<(u64, Instant)>,
}

impl Bench {
    fn row(&mut self, frame: u64) -> Option<&mut Row> {
        self.rows
            .get_mut(usize::try_from(frame.checked_sub(1)?).ok()?)
    }
}

/// The position of the light in a frame: a circle around the center of the room, at
/// half of the room height.
fn path_position(frame: u64, light_radius: f32, config: &Config) -> Vec3 {
    let angle = (frame % FRAMES_PER_TURN) as f32 / FRAMES_PER_TURN as f32 * TAU;
    let reach = 0.3 * config.room_width.min(config.room_depth);
    let position = Vec3::new(
        angle.cos() * reach,
        config.room_height / 2.0,
        angle.sin() * reach,
    );
    clamp_to_room(position, light_radius, config)
}

fn move_lights(time: Res<Time>, config: Res<Config>, mut lights: Query<&mut Light>) {
    for mut light in &mut lights {
        light.position = path_position(time.frame, light.radius, &config);
    }
}

/// Runs at the start of each frame (`Input` is the first schedule).
fn record_frame(
    time: Res<Time>,
    graph: Option<Res<FrameGraph>>,
    mut bench: ResMut<Bench>,
    mut quit: ResMut<QuitRequested>,
) {
    let now = Instant::now();
    let bench = &mut *bench;

    // CPU time of a frame: from its start to the start of the next frame. It has all
    // schedules, and the time that the CPU waits for the GPU and for the display.
    // Not `Time::delta`: that value has an upper limit.
    if let Some((frame, start)) = bench.last_start.replace((time.frame, now))
        && let Some(row) = bench.row(frame)
    {
        row.cpu_ms = Some((now - start).as_secs_f32() * 1000.0);
    }

    // The GPU times name their frame. They stay in the graph until newer ones arrive,
    // so the same values can show in more than one frame.
    if let Some(timings) = graph.as_ref().and_then(|graph| graph.timings())
        && let Some(row) = bench.row(timings.frame)
        && row.gpu_ms.is_none()
    {
        row.gpu_ms = Some(timings.passes.iter().map(|(_, ms)| *ms).collect());
        bench.pass_names = timings.passes.iter().map(|(name, _)| *name).collect();
    }

    let frames = bench.rows.len() as u64;
    let complete = bench.rows.last().is_some_and(|row| row.gpu_ms.is_some());
    if !quit.0 && (complete || time.frame > frames + MAX_WAIT_FRAMES) {
        match std::fs::write(OUTPUT_PATH, csv(&bench.pass_names, &bench.rows)) {
            Ok(()) => tracing::info!(path = OUTPUT_PATH, frames, "benchmark written"),
            Err(error) => tracing::error!(path = OUTPUT_PATH, %error, "cannot write the benchmark"),
        }
        quit.0 = true;
    }
}

/// One line for each frame: frame number, CPU milliseconds, GPU milliseconds of each pass.
fn csv(pass_names: &[&str], rows: &[Row]) -> String {
    let cell = |value: Option<f32>| value.map_or(String::new(), |value| format!("{value:.3}"));
    let mut text = String::from("frame,cpu_ms");
    for name in pass_names {
        text += &format!(",gpu_{name}_ms");
    }
    text.push('\n');
    for (index, row) in rows.iter().enumerate() {
        text += &format!("{},{}", index + 1, cell(row.cpu_ms));
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
    fn csv_has_one_line_for_each_frame_and_empty_cells_for_missing_values() {
        let rows = [
            Row {
                cpu_ms: Some(16.5),
                gpu_ms: Some(vec![0.25, 3.0]),
            },
            // The GPU times of this frame did not arrive.
            Row {
                cpu_ms: Some(17.0),
                gpu_ms: None,
            },
        ];
        assert_eq!(
            csv(&["trace", "tonemap"], &rows),
            "frame,cpu_ms,gpu_trace_ms,gpu_tonemap_ms\n1,16.500,0.250,3.000\n2,17.000,,\n"
        );
    }

    #[test]
    fn the_light_path_is_the_same_in_each_run_and_stays_in_the_room() {
        let config = Config::default();
        let half = Vec3::new(config.room_width, 0.0, config.room_depth) / 2.0;
        for frame in 0..2 * FRAMES_PER_TURN {
            let position = path_position(frame, config.light_radius, &config);
            assert_eq!(position, path_position(frame, config.light_radius, &config));
            assert!(
                position.cmpge(-half).all()
                    && position.cmple(half.with_y(config.room_height)).all()
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
