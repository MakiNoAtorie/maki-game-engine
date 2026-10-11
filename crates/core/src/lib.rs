//! Shared foundation: configuration, the app with its schedules, and time.
//! Math and handles come later.

pub mod app;
pub mod config;
pub mod time;

pub use app::{App, Extract, FixedUpdate, Input, Module, Present, Render, Startup, Update};
pub use config::Config;
pub use time::Time;
