//! The distance from the camera of each pixel of the last frame, on the CPU: for a
//! tool that must know what hides a point of the room (the marks of an editor).
//!
//! The trace pass has the distance for the denoiser (the `w` of the image `normal`,
//! see `passes.rs`). This file is the one place that tells a tool where it is.

use crate::{FrameGraph, RenderGpu};

/// The image of the trace pass with the world normal and the distance.
const IMAGE: &str = "normal";
/// Bytes of a texel of `Rgba16Float`, and the place of its fourth number.
const TEXEL_BYTES: usize = 8;
const DISTANCE_BYTE: usize = 6;
/// The distances are kept for at most this many columns of the picture: each
/// second, third, ... texel of a larger one. The memory of a GPU buffer is slow to
/// read, and a tool tests lines, not pixels.
const MAX_WIDTH: usize = 1024;

/// For the places of a frame: the length of the camera ray to the first surface.
///
/// - Glass: the glass surface.
/// - A mirror: the surface that the mirror shows, with the length of the full path.
///   The distance to the mirror itself is not here.
pub struct Distances {
    width: usize,
    height: usize,
    /// 16-bit floats, row by row from the top. A number is made when it is asked for.
    halves: Vec<u16>,
}

impl Distances {
    /// From 16-bit floats, row by row from the top.
    ///
    /// Panics if their number is not that of the size.
    pub fn from_halves(width: usize, height: usize, halves: Vec<u16>) -> Self {
        assert_eq!(halves.len(), width * height);
        Self {
            width,
            height,
            halves,
        }
    }

    /// The distance at a place of the picture: `x` from the left edge and `y` from
    /// the top edge, each 0.0 to 1.0. 0.0: the ray hit nothing, or the place is not
    /// in the picture.
    pub fn at(&self, x: f32, y: f32) -> f32 {
        if !(0.0..1.0).contains(&x) || !(0.0..1.0).contains(&y) || self.halves.is_empty() {
            return 0.0;
        }
        let column = (x * self.width as f32) as usize;
        let row = (y * self.height as f32) as usize;
        half_to_f32(self.halves[row * self.width + column])
    }
}

/// Copies the distances of the last frame from the GPU. `None`: no frame was drawn.
///
/// Call between frames (a system of `Update`). It waits for the GPU and copies the
/// image: call it after the picture changed (`Accumulation::samples` started
/// again), not in each frame.
pub fn read_distances(graph: &FrameGraph, gpu: &mut RenderGpu) -> Option<Distances> {
    let (image, (width, height), state) = graph.image(graph.resource(IMAGE)?)?;
    let (width, height) = (width as usize, height as usize);
    let step = width.div_ceil(MAX_WIDTH).max(1);
    let (kept_width, kept_height) = (width.div_ceil(step), height.div_ceil(step));
    let take = |texels: &[u8]| {
        let mut halves = Vec::with_capacity(kept_width * kept_height);
        for row in (0..height).step_by(step) {
            for column in (0..width).step_by(step) {
                let at = (row * width + column) * TEXEL_BYTES + DISTANCE_BYTE;
                halves.push(u16::from_ne_bytes([texels[at], texels[at + 1]]));
            }
        }
        halves
    };
    let halves = gpu.0.read_image_with(image, state, take);
    let halves = halves.inspect_err(|error| tracing::error!("no distances: {error:#}"));
    Some(Distances::from_halves(
        kept_width,
        kept_height,
        halves.ok()?,
    ))
}

/// The value of a 16-bit float: 1 sign bit, 5 exponent bits, 10 fraction bits.
pub fn half_to_f32(half: u16) -> f32 {
    let sign = if half >> 15 == 0 { 1.0 } else { -1.0 };
    let exponent = i32::from(half >> 10 & 0x1F);
    let fraction = f32::from(half & 0x3FF) / 1024.0;
    match exponent {
        // Numbers too small for the normal form.
        0 => sign * fraction * 2f32.powi(-14),
        31 if fraction == 0.0 => sign * f32::INFINITY,
        31 => f32::NAN,
        _ => sign * (1.0 + fraction) * 2f32.powi(exponent - 15),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_of_the_picture_gives_the_distance_of_its_texel() {
        // Two places in a row: distances 1.0 and 2.0 as 16-bit floats.
        let distances = Distances::from_halves(2, 1, vec![0x3C00, 0x4000]);
        assert_eq!(distances.at(0.2, 0.5), 1.0);
        assert_eq!(distances.at(0.7, 0.5), 2.0);
        // Not in the picture.
        assert_eq!(distances.at(1.0, 0.5), 0.0);
        assert_eq!(distances.at(0.5, -0.1), 0.0);
    }
}
