//! The editor program: `maki-editor [PROJECT [CONFIG]]`. All of it is in the engine
//! (`maki_engine::run_editor`); the code of a game comes from the script DLL of the
//! project that it opens.

fn main() -> maki_engine::Result {
    maki_engine::run_editor()
}
