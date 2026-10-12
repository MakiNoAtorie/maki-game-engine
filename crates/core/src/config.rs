use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use bevy_ecs::resource::Resource;
use serde::{Deserialize, Serialize};

/// Engine settings, read from a TOML file at startup.
// `default` lets an old config file omit fields that were added later.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Resource)]
#[serde(default)]
pub struct Config {
    pub window_title: String,
    /// Window size in logical pixels: the OS multiplies it by the display scale factor.
    pub window_width: u32,
    pub window_height: u32,
    /// The path tracer renders at the window size multiplied by this, 0.1 to 1.0.
    /// 1.0: one traced pixel for each pixel of the window. Lower: less GPU time, and
    /// the result is scaled to the window.
    pub render_scale: f32,
    /// A fixed size for the path tracer, in pixels. It does not follow the window, and
    /// `render_scale` has no effect: a benchmark does the same work on each display.
    /// 0: the window size multiplied by `render_scale`.
    pub render_width: u32,
    pub render_height: u32,
    pub vsync: bool,
    /// The room file that the game starts in. See `game/assets/rooms`.
    pub start_room: PathBuf,
    /// The spawn point of that room where the player starts.
    pub start_spawn: String,
    /// The view of the camera: where it is when it looks at `camera_target`, in
    /// meters: X, Y (up), Z. The camera keeps this direction and distance when it
    /// follows the player.
    pub camera_position: [f32; 3],
    /// The point that the camera looks at before it follows the player. Its height
    /// stays the height of the point that the camera looks at.
    pub camera_target: [f32; 3],
    /// Vertical field of view of the camera, in degrees.
    pub camera_fov_degrees: f32,
    /// The camera follows the player: its target moves most of the way to the player in
    /// this time, in seconds. 0: no delay. The view direction and the distance stay
    /// those of `camera_position` and `camera_target`.
    pub camera_follow_time: f32,
    /// The camera target stays this far from the walls, in meters, so the view shows
    /// less of the outside of the room.
    pub camera_margin: f32,
    /// glTF file of the player (`.gltf` or `.glb`), with the feet at its origin.
    /// `None` shows a generated capsule.
    pub player_model_path: Option<PathBuf>,
    /// Speed of the player, in meters per second.
    pub player_walk_speed: f32,
    /// Speed of the player with the run action.
    pub player_run_speed: f32,
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
    /// and the picture is the mean of some frames: smooth edges, and with a
    /// `render_scale` below 1 more detail than one frame has. False: the picture is
    /// the samples of one frame, at the pixel centers (stairs at each edge).
    pub taa: bool,
    /// The part of a new frame in the picture of the anti-aliasing, above 0 to 1.
    /// Lower: smoother edges, and the picture follows motion later. 0: the plain
    /// mean of all frames since the last change, for a reference picture of a scene
    /// at rest.
    pub taa_blend: f32,
    /// The picture is multiplied by this before the tone curve. 2.0 is one stop brighter.
    pub exposure: f32,
    /// Rate of the FixedUpdate schedule, in steps per second.
    pub fixed_hz: f64,
    /// Limit of FixedUpdate steps in one frame. A longer frame counts as this many steps.
    pub max_fixed_steps: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            window_title: "maki".to_owned(),
            window_width: 1280,
            window_height: 720,
            render_scale: 1.0,
            render_width: 0,
            render_height: 0,
            vsync: true,
            start_room: PathBuf::from("game/assets/rooms/room_a.ron"),
            start_spawn: "start".to_owned(),
            // Above and in front of the room, looking down into it at an angle.
            camera_position: [0.0, 9.0, 7.0],
            camera_target: [0.0, 0.5, 0.0],
            camera_fov_degrees: 45.0,
            camera_follow_time: 0.25,
            camera_margin: 3.0,
            player_model_path: None,
            player_walk_speed: 1.6,
            player_run_speed: 3.6,
            max_samples: 1024,
            samples_per_frame: 0,
            trace_budget_ms: 6.0,
            light_history_samples: 16,
            max_path_rays: 8,
            denoise_passes: 4,
            denoise_direct_passes: 1,
            denoise_reach: 4096.0,
            indirect_clamp: 8.0,
            history_clamp: 2.0,
            taa: true,
            taa_blend: 0.1,
            exposure: 1.0,
            fixed_hz: 60.0,
            max_fixed_steps: 8,
        }
    }
}

impl Config {
    pub const DEFAULT_PATH: &str = "config.toml";

    /// Loads the config. If the file does not exist, writes the defaults there and returns them.
    pub fn load_or_create(path: &Path) -> Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => {
                toml::from_str(&text).with_context(|| format!("invalid config {}", path.display()))
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {
                let config = Self::default();
                fs::write(path, toml::to_string_pretty(&config)?)
                    .with_context(|| format!("cannot write default config {}", path.display()))?;
                Ok(config)
            }
            Err(e) => Err(e).with_context(|| format!("cannot read config {}", path.display())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_created_then_read_back() {
        let path = std::env::temp_dir().join(format!("maki-config-{}.toml", std::process::id()));
        let _ = fs::remove_file(&path);

        let created = Config::load_or_create(&path).unwrap();
        assert_eq!(created, Config::default());

        // A partial file keeps defaults for the fields it omits.
        fs::write(&path, "vsync = false\nstart_room = \"a.ron\"\n").unwrap();
        let loaded = Config::load_or_create(&path).unwrap();
        assert!(!loaded.vsync);
        assert_eq!(loaded.start_room, PathBuf::from("a.ron"));
        assert_eq!(loaded.window_width, Config::default().window_width);

        fs::remove_file(&path).unwrap();
    }
}
