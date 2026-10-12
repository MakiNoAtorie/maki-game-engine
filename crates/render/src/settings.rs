//! The `[render]` table of the config file.

use bevy_ecs::resource::Resource;
use maki_core::Settings;
use serde::{Deserialize, Serialize};

/// The tone curve of the display transform. In the config file: `"agx"` or `"neutral"`.
// The shader `tonemap.comp` has the same two values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToneCurve {
    /// AgX: the look of film. Soft colors and contrast; a bright colored light goes
    /// to white with no change of its hue.
    #[default]
    Agx,
    /// Khronos PBR Neutral: colors as the scene has them, up to bright ones.
    Neutral,
}

/// The settings of the renderer. A debug panel can write them during a run: each
/// write starts the picture again.
#[derive(Resource, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RenderSettings {
    /// The path tracer renders at the picture size (`[window]`) multiplied by this,
    /// 0.1 to 1.0. 1.0: one traced pixel for each pixel of the picture. Lower: less
    /// GPU time, and the result is scaled to the picture.
    pub scale: f32,
    /// A fixed size for the path tracer, in pixels. It does not follow the picture, and
    /// `scale` has no effect: a benchmark does the same work on each display.
    /// 0: the picture size multiplied by `scale`.
    pub width: u32,
    pub height: u32,
    /// The path tracer stops when the picture is the mean of this many samples.
    pub max_samples: u32,
    /// Light paths that the path tracer follows for each pixel in one frame. More
    /// samples cost more GPU time for each frame and give a clean picture in fewer frames.
    /// 0: the renderer chooses the count in each frame, to fill `trace_budget_ms`.
    pub samples_per_frame: u32,
    /// GPU time for the path tracer in each frame, in milliseconds, when
    /// `samples_per_frame` is 0. Keep it well below the time of one display refresh.
    pub trace_budget_ms: f32,
    /// When the lighting changes (a light, or an object that moves), the indirect
    /// light of each pixel keeps at most this many samples of the old lighting, and
    /// the new samples replace them step by step. More samples: less noise while
    /// something moves, and the bounce light follows later.
    /// 0: a change of the lighting starts the indirect light again.
    /// The direct light (the shadows) has a test for each pixel: `history_clamp`.
    pub light_history_samples: u32,
    /// A light path ends after this many diffuse bounces, 1 to 16. 1: the light of a
    /// surface comes straight from a light or by way of one other surface. More: the
    /// light that fills dark corners after many bounces, for more rays and more noise.
    /// `max_path_rays` is also a limit: each bounce is a ray.
    pub max_bounces: u32,
    /// Light of many bounces for the cost of one. The last surface of a path counts
    /// the light straight from the lamps, and with this also the bounce light that the
    /// same point had in the picture of the frame before (if that picture showed
    /// it). Dark corners get the light that they have in a real room. False: one
    /// bounce (or `max_bounces`), and corners that no lamp sees stay too dark.
    pub bounce_light: bool,
    /// Blue noise: the random numbers for the point on the light and for the bounce
    /// direction are spread evenly over the pixels, so the noise of one frame has no
    /// clumps, and the denoiser has less to do. False: white noise for all.
    /// Measured: 6 to 10 % less error for a picture of up to 64 frames, 1 % in motion,
    /// for 1 % more GPU time (`docs/part3/stage-20.md`).
    pub blue_noise: bool,
    /// The largest number of rays in one light path. Mirrors that face each other and
    /// the inside of glass can reflect a ray many times. A lower limit costs less GPU
    /// time, and deep reflections lose detail.
    pub max_path_rays: u32,
    /// Filter passes of the denoiser for the indirect light (bounce light), 0 to 6.
    /// Each pass doubles the area that the filter takes its mean from. 0: no filter,
    /// only the mean over frames.
    pub denoise_passes: u32,
    /// The same for the direct light. It has less noise and sharp shadow edges: fewer
    /// passes keep them sharp. A scene with large lights (wide soft shadows) can need
    /// more.
    pub denoise_direct_passes: u32,
    /// The same for the specular light: the reflection of a rough surface (metal,
    /// plastic). The reflection of a smooth surface and of glass gets no filter.
    pub denoise_specular_passes: u32,
    /// The denoiser filter gets narrower as the mean of a pixel gets more samples: a
    /// filter pass whose pattern has `s` pixels between its points runs for a pixel
    /// while `s * s * samples` is at most this. 4096: up to 64 samples get all four
    /// passes, and 4096 samples get none. 0: each pass runs at each pixel.
    pub denoise_reach: f32,
    /// The largest light that one bounce ray adds to a pixel, in units of the light at
    /// the surface that the pixel shows (before its color). A rare path that finds the
    /// light by way of a mirror or through glass is some thousand times brighter than
    /// the others (a "firefly"): the limit keeps it from the denoiser. It makes such
    /// light (caustics) darker. Direct light and what the camera sees have no limit.
    /// 0: no limit.
    pub indirect_clamp: f32,
    /// History confidence of the direct light. The denoiser keeps a second, short
    /// mean of the direct light of each pixel (4 frames). Where the short means and
    /// the long means of a block of pixels differ by more than this many standard
    /// deviations of the noise, the lighting changed there, and the long mean starts
    /// again from the short one. So a shadow follows its object in few frames, and
    /// where nothing changes the long mean stays. Lower: a small change shows
    /// faster, and noise alone can start a mean again. 0: no such test; a change of
    /// the lighting then cuts the direct light of each pixel to
    /// `light_history_samples` too.
    pub history_clamp: f32,
    /// Temporal anti-aliasing. Each frame moves all camera rays by a part of a pixel,
    /// and the picture is the mean of some frames: smooth edges, and with a `scale`
    /// below 1 more detail than one frame has. False: the picture is the samples of
    /// one frame, at the pixel centers (stairs at each edge).
    pub taa: bool,
    /// The part of a new frame in the picture of the anti-aliasing, above 0 to 1.
    /// Lower: smoother edges, and the picture follows motion later. 0: the plain
    /// mean of all frames since the last change, for a reference picture of a scene
    /// at rest.
    pub taa_blend: f32,
    /// The picture is multiplied by this before the tone curve. 2.0 is one stop brighter.
    /// Manual: an automatic exposure would take away the darkness that the game wants.
    pub exposure: f32,
    /// The curve that bends light (0 to far above 1) into display color (0 to 1).
    pub tone_curve: ToneCurve,
    /// Bloom: the part of a wide blur of the picture that is blended into it, 0 to 1.
    /// The lamp and its reflections get a soft glow. 0: no bloom.
    pub bloom: f32,
    /// How dark the corners of the picture get, 0 (not at all) to 1 (black).
    pub vignette: f32,
    /// The color grade, each for R, G, B. Lift moves black toward a color (0: no
    /// change), gamma bends the middle tones (1: no change, above 1: brighter), gain
    /// scales white (1: no change). Small values: 0.02 of lift in blue gives cold
    /// shadows.
    pub grade_lift: [f32; 3],
    pub grade_gamma: [f32; 3],
    pub grade_gain: [f32; 3],
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            scale: 1.0,
            width: 0,
            height: 0,
            max_samples: 1024,
            samples_per_frame: 0,
            trace_budget_ms: 6.0,
            light_history_samples: 16,
            max_bounces: 2,
            bounce_light: true,
            blue_noise: true,
            max_path_rays: 8,
            denoise_passes: 4,
            denoise_direct_passes: 1,
            denoise_specular_passes: 2,
            denoise_reach: 4096.0,
            indirect_clamp: 8.0,
            history_clamp: 2.0,
            taa: true,
            taa_blend: 0.1,
            exposure: 1.0,
            tone_curve: ToneCurve::Agx,
            bloom: 0.04,
            vignette: 0.25,
            grade_lift: [0.0; 3],
            grade_gamma: [1.0; 3],
            grade_gain: [1.0; 3],
        }
    }
}

impl Settings for RenderSettings {
    const TABLE: &'static str = "render";

    fn check(&self) -> Result<(), String> {
        let passes = [
            self.denoise_passes,
            self.denoise_direct_passes,
            self.denoise_specular_passes,
        ];
        let not_negative = [self.denoise_reach, self.indirect_clamp, self.history_clamp];
        let grade = [self.grade_lift, self.grade_gamma, self.grade_gain];
        let limits = [
            (
                (0.1..=1.0).contains(&self.scale),
                "scale must be from 0.1 to 1.0",
            ),
            // With no sample the picture would never get content.
            (self.max_samples > 0, "max_samples must be 1 or more"),
            (
                self.samples_per_frame <= 64,
                "samples_per_frame must be from 0 (automatic) to 64",
            ),
            (
                self.trace_budget_ms.is_finite() && self.trace_budget_ms > 0.0,
                "trace_budget_ms must be a positive number",
            ),
            (
                (1..=16).contains(&self.max_bounces),
                "max_bounces must be from 1 to 16",
            ),
            // Two rays: a camera ray and one bounce.
            (
                (2..=32).contains(&self.max_path_rays),
                "max_path_rays must be from 2 to 32",
            ),
            (
                passes
                    .iter()
                    .all(|passes| *passes <= crate::MAX_DENOISE_PASSES),
                "denoise_passes, denoise_direct_passes, and denoise_specular_passes must be from 0 to 6",
            ),
            (
                not_negative
                    .iter()
                    .all(|value| value.is_finite() && *value >= 0.0),
                "denoise_reach, indirect_clamp, and history_clamp must be zero or positive numbers",
            ),
            (
                (0.0..=1.0).contains(&self.taa_blend),
                "taa_blend must be from 0 to 1",
            ),
            (
                (0.0..=1.0).contains(&self.bloom) && (0.0..=1.0).contains(&self.vignette),
                "bloom and vignette must be from 0 to 1",
            ),
            (
                self.grade_gamma.iter().all(|value| *value > 0.0)
                    && grade.iter().flatten().all(|value| value.is_finite()),
                "grade_lift, grade_gamma, and grade_gain must be numbers, and grade_gamma above 0",
            ),
            (
                self.exposure.is_finite() && self.exposure > 0.0,
                "exposure must be a positive number",
            ),
        ];
        match limits.iter().find(|(holds, _)| !holds) {
            Some((_, reason)) => Err((*reason).to_owned()),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_out_of_their_limits_are_rejected() {
        assert_eq!(RenderSettings::default().check(), Ok(()));
        let bad = [
            RenderSettings {
                exposure: f32::NAN,
                ..Default::default()
            },
            RenderSettings {
                max_samples: 0,
                ..Default::default()
            },
            RenderSettings {
                samples_per_frame: 65,
                ..Default::default()
            },
            RenderSettings {
                max_path_rays: 1,
                ..Default::default()
            },
        ];
        for settings in bad {
            assert!(settings.check().is_err(), "{settings:?}");
        }
    }
}
