//! Spike of part 6. All engine crates as one shared library, and the loader of the
//! script DLL of a project.

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use anyhow::{Context, Result};
use maki_core::App;

// A `dylib` holds each crate that it names, in full. Nothing here uses them.
extern crate maki_assets as _;
extern crate maki_audio as _;
extern crate maki_events as _;
extern crate maki_gameplay as _;
extern crate maki_platform as _;
extern crate maki_scene as _;
extern crate maki_gpu as _;
extern crate maki_render as _;
extern crate maki_ui as _;
extern crate maki_devtools as _;
extern crate maki_editor as _;

/// The function that a script DLL exports. Rust ABI: the DLL and the program must
/// come from the same compiler and the same build of the engine.
pub const ENTRY: &[u8] = b"maki_project";

/// Loads a copy of the script DLL and lets it add its modules to the app.
///
/// A copy, because Windows locks a loaded file and the next build must write it.
/// The copy is never unloaded: the app holds systems and closures of its code.
pub fn load_project(app: &mut App, dll: &Path) -> Result<()> {
    static LOADS: AtomicU32 = AtomicU32::new(0);
    let load = LOADS.fetch_add(1, Ordering::Relaxed);
    let dir = dll.parent().context("the DLL has no directory")?.join("loaded");
    if load == 0 {
        // Copies of earlier runs. One that another program still has stays.
        let _ = std::fs::remove_dir_all(&dir);
    }
    std::fs::create_dir_all(&dir)?;
    let stem = dll.file_stem().context("the DLL has no name")?.to_string_lossy();
    let copy = dir.join(format!("{stem}-{}-{load}.dll", std::process::id()));
    std::fs::copy(dll, &copy).with_context(|| format!("copy of {}", dll.display()))?;

    // SAFETY: the DLL is a build of this workspace, and `ENTRY` has this type in it.
    unsafe {
        let library = libloading::Library::new(&copy)
            .with_context(|| format!("load of {}", copy.display()))?;
        let entry = library.get::<fn(&mut App)>(ENTRY)?;
        entry(app);
        std::mem::forget(library);
    }
    tracing::info!(copy = %copy.display(), "project loaded");
    Ok(())
}
