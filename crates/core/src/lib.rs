//! Shared foundation: configuration, the app with its schedules, time, the transform,
//! and its interpolation between fixed steps.
//! Math types come from `glam`. Asset handles are in the assets crate.

pub mod app;
pub mod config;
pub mod interpolation;
pub mod time;
pub mod transform;

pub use app::{
    App, Extract, FixedFirst, FixedLast, FixedUpdate, Input, Module, Present, Render, ResetPlay,
    Startup, Update,
};
pub use config::{ConfigFile, Settings, TimeSettings};
pub use interpolation::{Interpolated, interpolate_transforms};
pub use time::Time;
pub use transform::Transform;

/// A mark that each program or DLL with its own copy of the engine crates has. The
/// loader of the editor looks for it in the script DLL of a project: a DLL that has
/// it was not linked to the engine DLL, and its types are not those of the editor.
#[unsafe(no_mangle)]
pub static MAKI_ENGINE_COPY: u8 = 1;
