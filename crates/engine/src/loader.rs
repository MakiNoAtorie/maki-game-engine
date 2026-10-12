//! The loader of the script DLL of a project.
//!
//! The DLL and the editor are two programs of one build: they agree on each type of
//! the engine only because both take the engine crates from one engine DLL. The
//! loader refuses a DLL where that does not hold, before it calls a function of it:
//! such a call has no defined result.

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use anyhow::{Context, Result, bail};
use maki_core::App;

/// The function that a script DLL exports (`export_project!`). Rust ABI: the DLL and
/// the program come from the same compiler and the same build of the engine.
pub const ENTRY: &[u8] = b"maki_project";

/// `maki_core::MAKI_ENGINE_COPY`: a DLL that has this symbol has its own engine.
const ENGINE_COPY: &[u8] = b"MAKI_ENGINE_COPY";

/// The name of the engine DLL (`maki-engine-dylib`). It is beside the program.
const ENGINE: &str = "maki_engine_dylib";

/// Loads a copy of the script DLL and lets it add its modules to the app.
///
/// A copy, because Windows locks a loaded file and the next build must write it.
/// The copy is never unloaded: the app holds systems and closures of its code.
///
/// `Err`, with no call into the DLL: the file is older than the engine DLL (it is of
/// another build of the engine), it does not load (it names functions that this
/// engine DLL does not have), or it has its own copy of the engine.
pub fn load_project(app: &mut App, dll: &Path) -> Result<()> {
    let engine = std::env::current_exe()?.with_file_name(libloading::library_filename(ENGINE));
    let written = |path: &Path| {
        let data = std::fs::metadata(path).with_context(|| format!("no file {}", path.display()));
        Ok::<_, anyhow::Error>(data?.modified()?)
    };
    // One build writes the engine DLL first, and then each DLL that links to it.
    if written(dll)? < written(&engine)? {
        bail!(
            "{} is of another build of the engine: it is older than {}. Build the project again",
            dll.display(),
            engine.display()
        );
    }

    static LOADS: AtomicU32 = AtomicU32::new(0);
    let load = LOADS.fetch_add(1, Ordering::Relaxed);
    let dir = dll
        .parent()
        .context("the DLL has no directory")?
        .join("loaded");
    if load == 0 {
        // Copies of earlier runs. One that another program still has stays.
        let _ = std::fs::remove_dir_all(&dir);
    }
    std::fs::create_dir_all(&dir)?;
    let stem = dll
        .file_stem()
        .context("the DLL has no name")?
        .to_string_lossy();
    let copy = dir.join(format!("{stem}-{}-{load}.dll", std::process::id()));
    std::fs::copy(dll, &copy).with_context(|| format!("copy of {}", dll.display()))?;

    // SAFETY: the load runs no code of the DLL (a Rust DLL has no start function).
    // The entry function is called only for a DLL that links to the engine DLL of
    // this program: `ENTRY` then has this type in it.
    unsafe {
        let library = libloading::Library::new(&copy).with_context(|| {
            format!(
                "{} does not load: it is of another build of the engine, or not a DLL of a \
                 project. Build the project again",
                dll.display()
            )
        })?;
        if library.get::<*const u8>(ENGINE_COPY).is_ok() {
            bail!(
                "{} has its own copy of the engine: it was not built with the build command \
                 of the editor",
                dll.display()
            );
        }
        let entry = library
            .get::<fn(&mut App)>(ENTRY)
            .with_context(|| format!("{} is not the DLL of a project", dll.display()))?;
        entry(app);
        std::mem::forget(library);
    }
    tracing::info!(copy = %copy.display(), "project loaded");
    Ok(())
}
