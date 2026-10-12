//! Shared foundation: configuration, the app with its schedules, time, the transform,
//! and its interpolation between fixed steps.
//! Math types come from `glam`. Asset handles are in the assets crate.

pub mod app;
pub mod config;
pub mod interpolation;
pub mod time;
pub mod transform;

pub use app::{
    App, Extract, FixedFirst, FixedLast, FixedUpdate, Input, Module, Present, Render, Startup,
    Update,
};
pub use config::{Config, ToneCurve};
pub use interpolation::{Interpolated, interpolate_transforms};
pub use time::Time;
pub use transform::Transform;
