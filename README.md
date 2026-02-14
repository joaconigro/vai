# VAI - Video with Artificial Intelligence

A sprite-sheet-like video compression format that separates scenes into static backgrounds and moving overlay layers, all compressed as AVIF images with a timeline script.

## Overview

VAI is an innovative video format that achieves compression by:

- Extracting a **static background** from video scenes
- Identifying **moving regions** as separate overlay layers
- Compressing all assets using **AVIF** (AV1 Image File Format)
- Using a **timeline script** to describe when and where each layer appears

This approach is particularly effective for videos with:

- Static or mostly static backgrounds
- Small moving objects or characters
- Screen recordings with UI elements
- Animation with limited motion

## Features

- **Efficient compression**: Reuses static backgrounds across frames
- **AVIF encoding**: Leverages modern AV1 image compression
- **Layer-based composition**: Separates foreground and background
- **Binary container format**: Compact `.vai` file format
- **FFmpeg integration**: Supports common video formats (MP4, MKV, AVI, WebM)
- **Frame extraction**: Decode VAI files back to PNG frames

## Project Structure

```
vai/
├── vai-core/          # Core library: binary format and data structures
├── vai-encoder/       # Encoder: video → VAI conversion
├── vai-decoder/       # Decoder: VAI → frames
└── vai-cli/           # Command-line interface
```

## Installation

### Prerequisites

- **Rust** (1.70 or later): Install from [rustup.rs](https://rustup.rs/)
- **FFmpeg 7** development libraries: Required for video reading (`ffmpeg-next 7.1` is **not** compatible with FFmpeg 8+)
- **meson** and **ninja**: Required to build the `dav1d` AV1 decoder (used by `libdav1d-sys`)
- **cmake**: Required to build `libavif` (used by `libavif-sys`)
- **pkg-config**: Required for locating native libraries
- **nasm** (optional): May be required for optimized assembly in dav1d

#### macOS (Homebrew)

```bash
brew install ffmpeg@7 pkg-config meson ninja cmake nasm
```

Since `ffmpeg@7` is a keg-only formula, you need to tell `pkg-config` where to find it. Add this to your `~/.zshrc` (or `~/.bash_profile`):

```bash
export PKG_CONFIG_PATH="/opt/homebrew/opt/ffmpeg@7/lib/pkgconfig:$PKG_CONFIG_PATH"
```

Then reload your shell:

```bash
source ~/.zshrc
```

> **Note:** Homebrew's default `ffmpeg` formula installs FFmpeg 8, which removed the `avfft.h` header that `ffmpeg-sys-next 7.x` expects. You must use `ffmpeg@7` specifically.

#### Ubuntu/Debian

```bash
sudo apt-get update
sudo apt-get install -y ffmpeg libavformat-dev libavcodec-dev libavutil-dev \
    libswscale-dev libavfilter-dev pkg-config clang \
    meson ninja-build cmake nasm
```

#### Windows

Download pre-built FFmpeg 7.x binaries from [ffmpeg.org](https://ffmpeg.org/download.html) and ensure they're in your PATH. Then install the build tools:

```bash
pip install meson
choco install ninja cmake nasm
```

### Building from Source

```bash
git clone https://github.com/joaconigro/vai.git
cd vai
cargo build --release
```

The compiled binary will be available at `target/release/vai`.

## Usage

### Encoding Videos to VAI

Convert a standard video file to VAI format:

```bash
vai encode input.mp4 -o output.vai
```

With custom options:

```bash
vai encode input.mp4 -o output.vai --quality 80 --threshold 30 --min-region 64
```

**Options:**

- `--quality <0-100>`: AVIF encoding quality (default: 80)
  - Higher values = better quality, larger files
  - 80-90 recommended for most content
- `--threshold <0-255>`: Motion detection sensitivity (default: 30)
  - Lower values = more sensitive to changes
  - Higher values = only detect significant motion
- `--min-region <pixels>`: Minimum region size to track (default: 64)
  - Filters out very small motion artifacts
- `--fps <rate>`: Override output frame rate (optional)

### Decoding VAI to Frames

#### View File Information

```bash
vai decode input.vai --info
```

#### Extract All Frames

```bash
vai decode input.vai -o output_frames/
```

This creates PNG files: `frame_000000.png`, `frame_000001.png`, etc.

#### Extract a Single Frame

```bash
vai decode input.vai --frame 100 -o frame100.png
```

## VAI Binary Format Specification

The `.vai` file uses a custom binary container format:

### 1. Header (44 bytes)

| Field | Type | Size | Description |
|-------|------|------|-------------|
| Magic | `u8[4]` | 4 bytes | Magic bytes: `VAI\0` |
| Version | `u16` | 2 bytes | Format version (currently 1) |
| Width | `u32` | 4 bytes | Frame width in pixels |
| Height | `u32` | 4 bytes | Frame height in pixels |
| FPS Numerator | `u32` | 4 bytes | Frame rate numerator |
| FPS Denominator | `u32` | 4 bytes | Frame rate denominator |
| Duration | `u64` | 8 bytes | Total duration in milliseconds |
| Asset Count | `u32` | 4 bytes | Number of AVIF assets |
| Timeline Count | `u32` | 4 bytes | Number of timeline entries |

All integers are stored in **little-endian** format.

### 2. Asset Table

For each asset (count specified in header):

| Field | Type | Description |
|-------|------|-------------|
| Asset ID | `u32` | Unique identifier |
| Width | `u32` | Asset width in pixels |
| Height | `u32` | Asset height in pixels |
| Data Length | `u32` | Size of AVIF data in bytes |
| AVIF Data | `u8[]` | Raw AVIF-compressed image |

### 3. Timeline Entries

For each timeline entry (count specified in header):

| Field | Type | Description |
|-------|------|-------------|
| Asset ID | `u32` | Which asset to display |
| Start Time | `u64` | Start timestamp in milliseconds |
| End Time | `u64` | End timestamp in milliseconds |
| Position X | `i32` | X coordinate (can be negative) |
| Position Y | `i32` | Y coordinate (can be negative) |
| Z-Order | `i32` | Layer depth (0 = background) |

## Architecture Overview

### vai-core

The core library provides:

- **Binary format serialization/deserialization**
- **Data structures**: `VaiHeader`, `Asset`, `TimelineEntry`, `VaiContainer`
- **Low-level I/O**: Reading and writing `.vai` files

### vai-encoder

The encoder converts videos to VAI format:

1. **Video Reading** (`video_reader.rs`): Uses FFmpeg to extract RGBA frames
2. **Scene Analysis** (`scene_analyzer.rs`):
   - Computes background image (currently uses first frame)
   - Detects motion regions by comparing frames to background
   - Creates bounding boxes around changed areas
3. **AVIF Encoding** (`avif_encoder.rs`): Compresses images using ravif
4. **Timeline Generation**: Creates entries for each moving region with timestamps

### vai-decoder

The decoder reconstructs frames from VAI files:

1. **Container Parsing**: Reads header, assets, and timeline
2. **AVIF Decoding** (`avif_decoder.rs`): Decompresses images using libavif
3. **Frame Composition** (`frame_compositor.rs`):
   - Starts with background layer (z-order = 0)
   - Overlays active sprites in z-order
   - Performs alpha blending

### vai-cli

The command-line tool provides user-facing commands:

- `encode`: Video → VAI conversion
- `decode`: VAI → frames extraction
- `--info`: Display VAI file metadata

## Current Limitations

This is an initial implementation with room for improvement:

1. **Background Detection**: Currently uses the first frame as background. A production version would:
   - Compute median/mode background from multiple frames
   - Handle scene changes with multiple backgrounds
   - Detect static vs. dynamic background regions

2. **Motion Detection**: Uses simple pixel differencing. Could be enhanced with:
   - Connected component analysis for better region extraction
   - Optical flow for motion prediction
   - Temporal coherence for smoother tracking

3. **Compression**: Each frame diff is stored separately. Could optimize by:
   - Deduplicating identical sprites across frames
   - Using motion vectors instead of full regions
   - Implementing keyframe strategies

4. **Video Codec**: Direct frame-to-frame comparison. Future versions could:
   - Support multiple scenes with different backgrounds
   - Implement smart keyframe selection
   - Add audio track support

## Dependencies

### Core Libraries

- **byteorder**: Binary serialization
- **thiserror**: Error handling

### Image Processing

- **image**: Image manipulation and compositing
- **ravif**: Pure Rust AVIF encoder
- **libavif-image**: AVIF decoder

### Video Processing

- **ffmpeg-next**: FFmpeg bindings for video reading

### CLI

- **clap**: Command-line argument parsing
- **anyhow**: Error handling

## Contributing

Contributions are welcome! Areas for improvement:

- Better background detection algorithms
- Advanced motion tracking and region extraction
- Sprite deduplication and optimization
- Scene change detection
- Audio support
- Progressive decoding
- GPU acceleration

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

## Acknowledgments

- **AVIF**: Built on the AV1 image format for excellent compression
- **FFmpeg**: Video reading and frame extraction
- **Rust community**: Amazing ecosystem of crates
