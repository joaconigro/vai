/// Build script for vai-vlc-plugin
///
/// Compiles the C shim (vlc_shim.c) that provides the VLC module descriptor
/// and wraps all VLC ABI interactions, then links it into the final .so.

use std::env;
use std::process::Command;

fn main() {
    println!("cargo:warning=Building VAI VLC Plugin");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let out_dir = env::var("OUT_DIR").unwrap();

    // ── Compile the C shim to an object file ──
    // We compile manually rather than using cc::Build::compile() because cc
    // produces a static archive (.a) and auto-links it with `-l static=...`.
    // The linker then strips unreferenced symbols — including our critical
    // vlc_entry__3_0_0ft64.  By passing the .o directly on the link line,
    // ALL symbols are unconditionally included.
    let obj_path = format!("{}/vlc_shim.o", out_dir);
    let status = Command::new("cc")
        .args([
            "-O2",
            "-fPIC",
            "-ffunction-sections",
            "-fdata-sections",
            "-Wall",
            "-I/usr/include/vlc/plugins",
            "-c",
            "src/vlc_shim.c",
            "-o",
            &obj_path,
        ])
        .status()
        .expect("Failed to run cc");
    assert!(status.success(), "vlc_shim.c compilation failed");

    // Pass the object file directly to the linker (always fully included)
    println!("cargo:rustc-cdylib-link-arg={}", obj_path);

    // VLC plugins are loaded at runtime by VLC itself, so the VLC API symbols
    // (block_Alloc, es_out_Add, vlc_stream_Read, etc.) are not available at
    // link time. We must tell the linker to allow undefined symbols.
    match target_os.as_str() {
        "macos" => {
            println!("cargo:rustc-cdylib-link-arg=-undefined");
            println!("cargo:rustc-cdylib-link-arg=dynamic_lookup");
        }
        "linux" => {
            println!("cargo:rustc-cdylib-link-arg=-Wl,--allow-shlib-undefined");
            // Add a supplementary version script to export the VLC entry
            // symbols.  Rust's cdylib linker already has its own version
            // script for Rust #[no_mangle] symbols; GNU ld merges multiple
            // --version-script directives.
            let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
            println!(
                "cargo:rustc-cdylib-link-arg=-Wl,--version-script={}/vlc_exports.map",
                manifest_dir
            );
        }
        _ => {}
    }

    // Re-run if the shim changes
    println!("cargo:rerun-if-changed=src/vlc_shim.c");
}
