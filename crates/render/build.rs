//! Compiles the GLSL shaders in `shaders/` to SPIR-V with `glslc` (Vulkan SDK).
//!
//! - Each file in `shaders/` that is not `.glsl` is one shader. The extension gives the
//!   stage (`.comp` = compute). `name.comp` becomes `$OUT_DIR/name.comp.spv`.
//! - `.glsl` files are shared code for `#include`.
//! - Rust code embeds the result with `shader_spirv!("name.comp")` (see `src/lib.rs`).
//!
//! To add a shader, add the file. No change here is necessary.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    // Run again when a shader changes or when the SDK location changes.
    println!("cargo::rerun-if-changed=shaders");
    println!("cargo::rerun-if-env-changed=VULKAN_SDK");

    // Without VULKAN_SDK, `glslc` must be on PATH.
    let glslc = env::var_os("VULKAN_SDK").map_or(PathBuf::from("glslc"), |sdk| {
        PathBuf::from(sdk).join("bin").join("glslc")
    });
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));

    for entry in std::fs::read_dir("shaders").expect("shaders directory exists") {
        let path = entry.expect("readable directory entry").path();
        if path
            .extension()
            .is_some_and(|extension| extension == "glsl")
        {
            continue;
        }
        let name = path
            .file_name()
            .expect("a file has a name")
            .to_string_lossy();
        let output = Command::new(&glslc)
            .arg("--target-env=vulkan1.3")
            .arg("-I")
            .arg("shaders")
            .arg(&path)
            .arg("-o")
            .arg(out_dir.join(format!("{name}.spv")))
            .output()
            .unwrap_or_else(|error| {
                panic!(
                    "cannot run {}: {error}. Install the Vulkan SDK (https://vulkan.lunarg.com) \
                     and set VULKAN_SDK, or put glslc on PATH",
                    glslc.display()
                )
            });
        if !output.status.success() {
            panic!(
                "glslc failed for {name}:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
