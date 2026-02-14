/// Build script for vai-vlc-plugin
///
/// This provides informational messages about VLC plugin installation.

use std::env;

fn main() {
    println!("cargo:warning=Building VAI VLC Plugin");
    
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    
    match target_os.as_str() {
        "linux" => {
            println!("cargo:warning=");
            println!("cargo:warning=Installation (Linux):");
            println!("cargo:warning=  Copy the plugin to VLC's plugin directory:");
            println!("cargo:warning=    mkdir -p ~/.local/lib/vlc/plugins/demux/");
            println!("cargo:warning=    cp target/release/libvai_vlc_plugin.so ~/.local/lib/vlc/plugins/demux/");
            println!("cargo:warning=  Or system-wide:");
            println!("cargo:warning=    sudo cp target/release/libvai_vlc_plugin.so /usr/lib/vlc/plugins/demux/");
        }
        "macos" => {
            println!("cargo:warning=");
            println!("cargo:warning=Installation (macOS):");
            println!("cargo:warning=  Copy the plugin to VLC's plugin directory:");
            println!("cargo:warning=    cp target/release/libvai_vlc_plugin.dylib /Applications/VLC.app/Contents/MacOS/plugins/");
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
