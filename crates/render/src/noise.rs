//! Blue noise: random numbers for the trace pass that are spread evenly over the
//! pixels.
//!
//! The trace pass makes its random numbers with a hash: white noise. By chance, some
//! pixels near each other get almost the same numbers and so the same error: the
//! noise of one frame has clumps, and a filter over the pixels around (the denoiser)
//! cannot take a clump away. A blue-noise pattern has no clumps: pixels near each
//! other have numbers far from each other, so their errors differ, and the mean of a
//! small block of pixels is near the true value.
//!
//! `build.rs` makes one pattern of 64 x 64 values. The trace pass reads it with an
//! offset for each of the four random numbers that matter most (the point on the
//! light and the bounce direction at the surface that the pixel shows) and adds a
//! step for each frame, so a pixel gets a new, well spread number in each frame
//! (see `blue()` in `shaders/trace.comp`). All other random numbers are white noise.

use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::ResMut;
use maki_core::{App, Startup};
use maki_gpu::{BufferDesc, BufferHandle};

use crate::{RenderGpu, RenderWorld, setup_renderer};

/// The pattern: `f32` values, row by row. See `blue_noise` in `build.rs`.
const PATTERN: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/blue_noise.bin"));

/// The buffer with the pattern. A resource of the render world.
#[derive(Resource)]
pub(crate) struct NoiseBuffer(pub BufferHandle);

/// Adds the noise buffer. Part of `RenderModule`.
pub(crate) fn build(app: &mut App) {
    app.add_systems(Startup, setup_noise.after(setup_renderer));
}

fn setup_noise(mut gpu: ResMut<RenderGpu>, mut render_world: ResMut<RenderWorld>) {
    let buffer = gpu
        .0
        .create_buffer(&BufferDesc {
            name: "blue noise",
            size: PATTERN.len() as u64,
            cpu_writable: true,
        })
        .expect("noise buffer");
    gpu.0
        .write_buffer(buffer, 0, PATTERN)
        .expect("noise buffer write");
    render_world.0.insert_resource(NoiseBuffer(buffer));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pattern_has_each_value_one_time_and_no_clumps() {
        let values: Vec<f32> = PATTERN
            .as_chunks::<4>()
            .0
            .iter()
            .map(|bytes| f32::from_ne_bytes(*bytes))
            .collect();
        assert_eq!(values.len(), 64 * 64);
        let mut sorted = values.clone();
        sorted.sort_by(f32::total_cmp);
        for (index, value) in sorted.iter().enumerate() {
            assert!((value - (index as f32 + 0.5) / 4096.0).abs() < 1e-6);
        }
        // Two pixels side by side differ much more than two random values do (a mean
        // difference of 1/3 for white noise).
        let difference = |a: usize, b: usize| (values[a] - values[b]).abs();
        let side_by_side: f32 = (0..4096)
            .map(|i| difference(i, i / 64 * 64 + (i + 1) % 64))
            .sum();
        assert!(side_by_side / 4096.0 > 0.38, "{}", side_by_side / 4096.0);
    }
}
