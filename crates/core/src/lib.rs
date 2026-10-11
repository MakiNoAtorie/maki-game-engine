//! Shared foundation: configuration, the app with its schedules, time, and the transform.
//! Math types come from `glam`. Asset handles are in the assets crate.

pub mod app;
pub mod config;
pub mod time;
pub mod transform;

pub use app::{
    App, Extract, FixedLast, FixedUpdate, Input, Module, Present, Render, Startup, Update,
};
pub use config::Config;
pub use time::Time;
pub use transform::Transform;
