//! Compiles the GLSL shaders in `shaders/` to SPIR-V with `glslc` (Vulkan SDK).
//!
//! - Each file in `shaders/` that is not `.glsl` is one shader. The extension gives the
//!   stage (`.comp` = compute, `.vert` = vertex, `.frag` = fragment). `name.comp` becomes `$OUT_DIR/name.comp.spv`.
//! - `.glsl` files are shared code for `#include`.
//! - Rust code embeds the result with `shader_spirv!("name.comp")` (see `src/lib.rs`).
//!
//! To add a shader, add the file. No change here is necessary.
//!
//! The ui crate uses this file as its build script too (`build` in its `Cargo.toml`), so
//! the two crates have the same rules. The working directory is that of the crate that
//! is built.

use std::env;
use std::path::PathBuf;
use std::process::Command;

/// Side of the blue-noise pattern, in pixels. Must agree with `noise.rs` and
/// `trace.comp`.
const NOISE_SIZE: usize = 64;

/// A blue-noise pattern: `NOISE_SIZE` x `NOISE_SIZE` values from 0.0 to 1.0, each value
/// one time, in an order such that pixels near each other have values far from each
/// other. As bytes of `f32`. See `src/noise.rs` for its use.
///
/// The method is "void and cluster" (Ulichney, 1993). Each point that is set adds a
/// soft bump (a Gaussian) to an energy map; the map wraps at the edges, so the pattern
/// can repeat. The energy is high in a cluster of points and low in a void.
/// 1. Start: some random points, then move the point in the tightest cluster to the
///    largest void until that changes nothing more.
/// 2. Take the points away, each time the one in the tightest cluster: the last one
///    taken gets the rank 0, the first one the highest rank of the start points.
/// 3. With all start points again: fill the largest void, each new point with the
///    next rank, until each pixel has a rank.
// ponytail: step 3 runs to the end. The paper fills the second half of the pixels by
// "clusters of the empty pixels", which gives a little better high values.
fn blue_noise() -> Vec<u8> {
    const N: usize = NOISE_SIZE;
    const SIGMA: f32 = 1.9; // width of the bump, in pixels: the value of the paper
    // The bump of a point at (0, 0), for each pixel.
    let wrapped = |d: usize| d.min(N - d) as f32;
    let bump: Vec<f32> = (0..N * N)
        .map(|i| {
            let (x, y) = (wrapped(i % N), wrapped(i / N));
            (-(x * x + y * y) / (2.0 * SIGMA * SIGMA)).exp()
        })
        .collect();
    // Adds (sign 1.0) or takes away (-1.0) the bump of the point `at`.
    let add = |energy: &mut [f32], at: usize, sign: f32| {
        let (px, py) = (at % N, at / N);
        for (i, value) in energy.iter_mut().enumerate() {
            let (dx, dy) = ((i % N + N - px) % N, (i / N + N - py) % N);
            *value += sign * bump[dy * N + dx];
        }
    };
    // The set point with the most energy, or the free pixel with the least.
    let extreme = |energy: &[f32], set: &[bool], want_set: bool| {
        let candidates = (0..N * N).filter(|&i| set[i] == want_set);
        let by_energy = |a: &usize, b: &usize| energy[*a].total_cmp(&energy[*b]);
        if want_set {
            candidates.max_by(by_energy)
        } else {
            candidates.min_by(by_energy)
        }
        .expect("the pattern has set and free pixels")
    };

    // 1. A tenth of the pixels, from a small fixed random sequence.
    let (mut set, mut energy) = (vec![false; N * N], vec![0.0_f32; N * N]);
    let mut state = 12345_u32;
    let mut count = 0;
    while count < N * N / 10 {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let at = (state >> 8) as usize % (N * N);
        if !set[at] {
            set[at] = true;
            add(&mut energy, at, 1.0);
            count += 1;
        }
    }
    loop {
        let cluster = extreme(&energy, &set, true);
        set[cluster] = false;
        add(&mut energy, cluster, -1.0);
        let void = extreme(&energy, &set, false);
        set[void] = true;
        add(&mut energy, void, 1.0);
        if void == cluster {
            break;
        }
    }
    let mut rank = vec![0_usize; N * N];
    // 2. On a copy.
    let (mut left, mut left_energy) = (set.clone(), energy.clone());
    for next in (0..count).rev() {
        let cluster = extreme(&left_energy, &left, true);
        left[cluster] = false;
        add(&mut left_energy, cluster, -1.0);
        rank[cluster] = next;
    }
    // 3.
    for next in count..N * N {
        let void = extreme(&energy, &set, false);
        set[void] = true;
        add(&mut energy, void, 1.0);
        rank[void] = next;
    }
    // The middle of each of the N * N steps from 0 to 1.
    let values = rank.iter().map(|&r| (r as f32 + 0.5) / (N * N) as f32);
    values.flat_map(f32::to_ne_bytes).collect()
}

fn main() {
    // Run again when a shader changes or when the SDK location changes.
    println!("cargo::rerun-if-changed=shaders");
    println!("cargo::rerun-if-changed=../render/shaders/bindless.glsl");
    println!("cargo::rerun-if-env-changed=VULKAN_SDK");

    // Without VULKAN_SDK, `glslc` must be on PATH.
    let glslc = env::var_os("VULKAN_SDK").map_or(PathBuf::from("glslc"), |sdk| {
        PathBuf::from(sdk).join("bin").join("glslc")
    });
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    // Generated, not a file of the repository. The same in each build.
    std::fs::write(out_dir.join("blue_noise.bin"), blue_noise()).expect("write blue_noise.bin");

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
            // `bindless.glsl` is here. The path is also correct from the ui crate.
            .arg("-I")
            .arg("../render/shaders")
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
