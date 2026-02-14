# VAI VLC Plugin

A VLC media player plugin that enables native playback of VAI (Video with Artificial Intelligence) video files.

## Overview

This plugin implements VLC's demuxer API to decode and play `.vai` files directly in VLC media player. It leverages the existing `vai-core` and `vai-decoder` crates to parse the VAI container format and render frames.

### Features

- Native `.vai` file playback in VLC
- Full seeking support (forward and backward)
- Proper timeline controls
- No external dependencies beyond VLC itself
- Cross-platform support (Linux, macOS, Windows)

## How It Works

The plugin:
1. Probes files by checking for the VAI magic bytes (`VAI\0`)
2. Parses the VAI container to extract video metadata and sprite assets
3. Registers a raw RGBA video stream with VLC
4. Renders frames on-demand using the VAI frame compositor
5. Delivers decoded frames to VLC for display

## Building

### Prerequisites

- Rust toolchain (1.70 or later)
- VLC media player installed (for testing)

### Build Commands

Build the plugin in release mode:

```bash
cargo build --release -p vai-vlc-plugin
```

The compiled plugin will be located at:
- **Linux**: `target/release/libvai_vlc_plugin.so`
- **macOS**: `target/release/libvai_vlc_plugin.dylib`
- **Windows**: `target/release/vai_vlc_plugin.dll`

## Installation

### Linux

**User-specific installation** (recommended):

```bash
mkdir -p ~/.local/lib/vlc/plugins/demux/
cp target/release/libvai_vlc_plugin.so ~/.local/lib/vlc/plugins/demux/
```

**System-wide installation**:

```bash
sudo cp target/release/libvai_vlc_plugin.so /usr/lib/vlc/plugins/demux/
```

Note: On some systems, the plugin directory might be `/usr/lib/x86_64-linux-gnu/vlc/plugins/demux/` or similar.

### macOS

```bash
cp target/release/libvai_vlc_plugin.dylib /Applications/VLC.app/Contents/MacOS/plugins/demux/
```

If the `demux` directory doesn't exist, you may need to create it:

```bash
mkdir -p /Applications/VLC.app/Contents/MacOS/plugins/demux/
```

### Windows

```batch
copy target\release\vai_vlc_plugin.dll "C:\Program Files\VideoLAN\VLC\plugins\demux\"
```

You may need administrator privileges to copy to the VLC installation directory.

## Usage

After installing the plugin:

1. Launch VLC media player
2. Open a `.vai` file using:
   - File → Open File
   - Drag and drop the `.vai` file onto VLC
   - Command line: `vlc video.vai`

VLC should automatically detect and use the VAI plugin to play the file.

### Troubleshooting

**Plugin not loading:**
- Ensure the plugin file is in the correct directory
- Check VLC's plugin cache: Tools → Preferences → Show settings: All → Advanced → Modules → Reset plugin cache
- Check VLC's messages log (Tools → Messages) for error messages

**Playback issues:**
- Verify the `.vai` file is valid by testing with the `vai-cli` tool
- Check VLC version compatibility (this plugin targets VLC 3.x API)

## Technical Notes

### VLC API Compatibility

This plugin is designed for VLC 3.x. The VLC plugin API is not stable across major versions, so:
- VLC 3.0 - 3.0.x: Should work
- VLC 4.x: May require API adjustments

### Architecture

The plugin consists of:
- **vlc_bindings.rs**: Manual C FFI bindings for VLC's plugin API
- **lib.rs**: Plugin implementation with entry points (Open, Close, Demux, Control)

Key functions:
- `Open()`: Probes and initializes the demuxer for VAI files
- `Close()`: Cleanup when file is closed
- `Demux()`: Called repeatedly by VLC to fetch the next frame
- `Control()`: Handles seeking and timeline queries

### Frame Delivery

The plugin delivers uncompressed RGBA frames to VLC. Each call to `Demux()`:
1. Calculates the timestamp for the current frame
2. Renders the frame using `FrameCompositor`
3. Copies the RGBA pixel data into a VLC block
4. Sends the block to VLC with proper timestamps

### Memory Management

The plugin uses Rust's `Box` for heap allocation of plugin-private data, with careful FFI boundary handling:
- `Box::into_raw()` when passing to VLC
- `Box::from_raw()` when reclaiming in Close
- Panic catching at all FFI boundaries to prevent unwinding into C code

## Development

### Testing

To test the plugin during development:

1. Build in debug mode: `cargo build -p vai-vlc-plugin`
2. Copy to plugin directory
3. Run VLC with verbose logging:
   ```bash
   vlc -vvv video.vai
   ```

### Debugging

Enable Rust backtraces:
```bash
RUST_BACKTRACE=1 vlc video.vai
```

Check VLC's message log for plugin-specific output.

## License

This plugin is part of the VAI project and is licensed under MIT OR Apache-2.0, matching the parent project's license.

## Contributing

Contributions are welcome! Please ensure:
- Code follows the existing style
- All unsafe code is properly documented
- Changes maintain compatibility with the VAI container format

## See Also

- [VAI Project](https://github.com/joaconigro/vai)
- [VLC Plugin Development](https://wiki.videolan.org/Hacker_Guide/How_To_Write_a_Module/)
- [vai-core](../vai-core/) - Core VAI format library
- [vai-decoder](../vai-decoder/) - VAI frame decoder
