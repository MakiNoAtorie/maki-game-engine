//! Shared foundation: configuration, the app with its schedules, time, the transform,
//! its interpolation between fixed steps, and the fields that describe themselves.
//! Math types come from `glam`. Asset handles are in the assets crate.

// The code that the derive `Fields` writes names this crate as `::maki_core`, also
// in this crate.
extern crate self as maki_core;

pub mod app;
pub mod config;
pub mod fields;
pub mod interpolation;
pub mod time;
pub mod transform;

pub use app::{
    App, Extract, FixedFirst, FixedLast, FixedUpdate, Input, Module, Present, Render, ResetPlay,
    Startup, Update,
};
pub use config::{ConfigFile, Settings, TimeSettings};
// The trait and its derive have one name, as `Serialize` of serde.
pub use fields::{FieldTypes, Fields, add_field_type};
pub use interpolation::{Interpolated, interpolate_transforms};
pub use maki_macros::Fields;
pub use time::Time;
pub use transform::Transform;

/// A mark that each program or DLL with its own copy of the engine crates has. The
/// loader of the editor looks for it in the script DLL of a project: a DLL that has
/// it was not linked to the engine DLL, and its types are not those of the editor.
#[unsafe(no_mangle)]
pub static MAKI_ENGINE_COPY: u8 = 1;
