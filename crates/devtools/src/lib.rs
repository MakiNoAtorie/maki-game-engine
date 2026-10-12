//! Tools for the developer of a game: the benchmark runner, the list of the room
//! files, and a panel for the settings of the gameplay modules. A game adds the
//! modules that it wants; a release build can leave them out.
//!
//! This crate is right of `ui`: the tools have panels, and the benchmark runner reads
//! the frame graph of the renderer.

mod bench;
mod gameplay_panel;
mod stages;

pub use bench::{BenchModule, BenchRun, OUTPUT_PATH, Script, Scripts, USAGE, parse_args};
pub use gameplay_panel::GameplayPanelModule;
pub use stages::{NEXT_STAGE, PREVIOUS_STAGE, RoomDirs, StagesModule};
