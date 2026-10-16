//! Tools for the developer of a game: the benchmark runner, the list of the room
//! files, a panel for the settings of the gameplay modules, and the log panel. A game adds the
//! modules that it wants; a release build can leave them out.
//!
//! This crate is right of `ui`: the tools have panels, and the benchmark runner reads
//! the frame graph of the renderer.

mod bench;
mod gameplay_panel;
mod log_panel;
mod script;
mod stages;

pub use bench::{
    BenchModule, BenchRun, BenchScripts, OUTPUT_PATH, USAGE, add_bench_script, check_bench,
    parse_args,
};
pub use gameplay_panel::GameplayPanelModule;
pub use log_panel::{LOG, LogFilter, LogLine, LogPanelModule, LogStore, OpenFile};
pub use script::{Axis, Pace, Place, Script, Step};
pub use stages::{NEXT_STAGE, PREVIOUS_STAGE, RoomDirs, StagesModule};
