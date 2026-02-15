/// Build script for vai-vlc-plugin
///
/// This provides informational messages about VLC plugin installation
/// and configures linker flags so undefined VLC symbols are resolved
/// at runtime when VLC loads the plugin.

use std::env;

fn main() {
    println!("cargo:warning=Building VAI VLC Plugin");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    // VLC plugins are loaded at runtime by VLC itself, so the VLC API symbols
    // (block_Alloc, es_out_Add, stream_Read, etc.) are not available at link
    // time. We must tell the linker to allow undefined symbols.
    match target_os.as_str() {
        "macos" => {
            // macOS: -undefined dynamic_lookup allows symbols to be resolved at
            // dlopen() time when VLC loads the plugin.
            println!("cargo:rustc-cdylib-link-arg=-undefined");
            println!("cargo:rustc-cdylib-link-arg=dynamic_lookup");

            println!("cargo:warning=");
            println!("cargo:warning=Installation (macOS):");
            println!("cargo:warning=  Copy the plugin to VLC's plugin directory:");
            println!("cargo:warning=    cp target/release/libvai_vlc_plugin.dylib /Applications/VLC.app/Contents/MacOS/plugins/");
        }
        "linux" => {
            // Linux: shared libraries allow undefined symbols by default, but
            // adding --allow-shlib-undefined makes it explicit.
            println!("cargo:rustc-cdylib-link-arg=-Wl,--allow-shlib-undefined");

            println!("cargo:warning=");
            println!("cargo:warning=Installation (Linux):");
            println!("cargo:warning=  Copy the plugin to VLC's plugin directory:");
            println!("cargo:warning=    mkdir -p ~/.local/lib/vlc/plugins/demux/");
            println!("cargo:warning=    cp target/release/libvai_vlc_plugin.so ~/.local/lib/vlc/plugins/demux/");
            println!("cargo:warning=  Or system-wide:");
            println!("cargo:warning=    sudo cp target/release/libvai_vlc_plugin.so /usr/lib/vlc/plugins/demux/");
        }
        "windows" => {
            println!("cargo:warning=");
            println!("cargo:warning=Installation (Windows):");
            println!("cargo:warning=  Copy the plugin to VLC's plugin directory:");
            println!("cargo:warning=    copy target\\release\\vai_vlc_plugin.dll \"C:\\Program Files\\VideoLAN\\VLC\\plugins\\demux\\\"");
        }
        _ => {
            println!("cargo:warning=Unknown target OS: {}", target_os);
        }
    }

    println!("cargo:warning=");
    println!("cargo:warning=After installation, VLC should be able to open .vai files directly.");
}
