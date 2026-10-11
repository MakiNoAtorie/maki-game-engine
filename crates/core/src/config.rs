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
    /// The path tracer renders at this size; the result is scaled to the window.
    pub render_width: u32,
    pub render_height: u32,
    pub vsync: bool,
    /// glTF file to show (`.gltf` or `.glb`). `None` shows a generated test scene.
    pub model_path: Option<PathBuf>,
    /// Inner size of the room in meters: X, Z, and Y.
    pub room_width: f32,
    pub room_depth: f32,
    pub room_height: f32,
    /// Where the camera is, in meters: X, Y (up), Z. The room floor has its center at the origin.
    pub camera_position: [f32; 3],
    /// The point that the camera looks at.
    pub camera_target: [f32; 3],
    /// Vertical field of view of the camera, in degrees.
    pub camera_fov_degrees: f32,
    /// Start position of the light, in meters. The light controller keeps it in the room.
    pub light_position: [f32; 3],
    /// Radius of the sphere light, in meters. A larger light gives softer shadows.
    pub light_radius: f32,
    /// Linear RGB.
    pub light_color: [f32; 3],
    /// Brightness of the light. See `Light::intensity` in the render crate for the unit.
    pub light_intensity: f32,
    /// The path tracer stops when the picture is the mean of this many samples.
    pub max_samples: u32,
    /// Light paths that the path tracer follows for each pixel in one frame. More
    /// samples cost more GPU time for each frame and give a clean picture in fewer frames.
    /// 0: the renderer chooses the count in each frame, to fill `trace_budget_ms`.
    pub samples_per_frame: u32,
    /// GPU time for the path tracer in each frame, in milliseconds, when
    /// `samples_per_frame` is 0. Keep it well below the time of one display refresh.
    pub trace_budget_ms: f32,
    /// When a light changes, the picture keeps at most this many samples of the old
    /// lighting, and the new samples replace them step by step. More samples: less
    /// noise while a light moves, and the lighting follows the light later.
    /// 0: a light change starts the picture again.
    pub light_history_samples: u32,
    /// The largest number of rays in one light path. Mirrors that face each other and
    /// the inside of glass can reflect a ray many times. A lower limit costs less GPU
    /// time, and deep reflections lose detail.
    pub max_path_rays: u32,
    /// Filter passes of the denoiser, 0 to 6. Each pass doubles the area that the
    /// filter takes its mean from. 0: no denoiser. Read at startup.
    pub denoise_passes: u32,
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
            render_width: 640,
            render_height: 360,
            vsync: true,
            model_path: None,
            room_width: 8.0,
            room_depth: 8.0,
            room_height: 3.0,
            // Above and in front of the room, looking down into it at an angle.
            camera_position: [0.0, 9.0, 7.0],
            camera_target: [0.0, 0.5, 0.0],
            camera_fov_degrees: 45.0,
            // In front of the two default objects, so their shadows fall to the back.
            light_position: [0.0, 2.0, 2.0],
            light_radius: 0.15,
            light_color: [1.0, 0.9, 0.8],
            light_intensity: 12.0,
            max_samples: 1024,
            samples_per_frame: 0,
            trace_budget_ms: 6.0,
            light_history_samples: 32,
            max_path_rays: 8,
            denoise_passes: 4,
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
        fs::write(&path, "vsync = false\nmodel_path = \"a.glb\"\n").unwrap();
        let loaded = Config::load_or_create(&path).unwrap();
        assert!(!loaded.vsync);
        assert_eq!(loaded.model_path, Some(PathBuf::from("a.glb")));
        assert_eq!(loaded.window_width, Config::default().window_width);

        fs::remove_file(&path).unwrap();
    }
}
