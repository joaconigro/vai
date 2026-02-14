//! VAI CLI Tool
//!
//! Command-line interface for encoding and decoding VAI video files.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use vai_core::VaiContainer;
use vai_decoder::FrameCompositor;
use vai_encoder::{EncoderConfig, SceneAnalyzer, VideoReader};

#[derive(Parser)]
#[command(name = "vai")]
#[command(about = "VAI (Video with Artificial Intelligence) - Sprite-sheet-like video compression")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Encode a video file to VAI format
    Encode {
        /// Input video file path
        input: PathBuf,

        /// Output VAI file path
        #[arg(short, long)]
        output: PathBuf,

        /// AVIF encoding quality (0-100)
        #[arg(long, default_value = "80")]
        quality: u8,

        /// Override output frame rate
        #[arg(long)]
        fps: Option<f64>,

        /// Motion detection threshold (0-255)
        #[arg(long, default_value = "30")]
        threshold: u8,

        /// Minimum region size in pixels
        #[arg(long, default_value = "64")]
        min_region: u32,
    },

    /// Decode a VAI file to frames
    Decode {
        /// Input VAI file path
        input: PathBuf,

        /// Output directory for frames or single frame file
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Show file information only
        #[arg(long)]
        info: bool,

        /// Extract a single frame by frame number
        #[arg(long)]
        frame: Option<u64>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Encode {
            input,
            output,
            quality,
            fps,
            threshold,
            min_region,
        } => encode_video(input, output, quality, fps, threshold, min_region)?,

        Commands::Decode {
            input,
            output,
            info,
            frame,
        } => decode_video(input, output, info, frame)?,
    }

    Ok(())
}

fn encode_video(
    input: PathBuf,
    output: PathBuf,
    quality: u8,
    fps: Option<f64>,
    threshold: u8,
    min_region: u32,
) -> Result<()> {
    println!("Encoding video: {}", input.display());
    println!("Output: {}", output.display());

    // Open video file
    let mut reader = VideoReader::open(
        input
            .to_str()
            .context("Invalid input path")?,
    )
    .context("Failed to open video file")?;

    let width = reader.width();
    let height = reader.height();
    let (fps_num, fps_den) = reader.frame_rate();
    let duration_ms = reader.duration_ms();

    println!(
        "Video info: {}x{} @ {}/{} fps, {} ms",
        width, height, fps_num, fps_den, duration_ms
    );

    // Analyze using streaming (processes one frame at a time)
    println!("Analyzing scene and encoding (streaming)...");
    let config = EncoderConfig {
        quality,
        fps,
        threshold,
        min_region_size: min_region,
    };

    let analyzer = SceneAnalyzer::new(config);
    let container = analyzer
        .analyze_streaming(&mut reader, width, height, fps_num, fps_den, duration_ms)
        .context("Failed to analyze video")?;

    println!(
        "Created {} assets and {} timeline entries",
        container.assets.len(),
        container.timeline.len()
    );

    // Write VAI file
    println!("Writing VAI file...");
    let file = File::create(&output).context("Failed to create output file")?;
    let mut writer = BufWriter::new(file);
    container
        .write(&mut writer)
        .context("Failed to write VAI container")?;

    println!("Successfully encoded to {}", output.display());

    Ok(())
}

fn decode_video(
    input: PathBuf,
    output: Option<PathBuf>,
    info: bool,
    frame_num: Option<u64>,
) -> Result<()> {
    println!("Decoding VAI file: {}", input.display());

    // Read VAI container
    let file = File::open(&input).context("Failed to open VAI file")?;
    let container = VaiContainer::read(file).context("Failed to read VAI container")?;

    // Show info if requested
    if info || output.is_none() {
        print_info(&container);
        if info {
            return Ok(());
        }
    }

    // Create compositor
    let mut compositor = FrameCompositor::new(container.clone());

    if let Some(frame_num) = frame_num {
        // Extract single frame
        let output_path = output.context("Output path required for frame extraction")?;
        
        // Calculate timestamp for frame number
        let fps = container.fps();
        let timestamp_ms = (frame_num as f64 * 1000.0 / fps) as u64;

        println!("Extracting frame {} at {}ms", frame_num, timestamp_ms);
        let frame = compositor
            .render_frame(timestamp_ms)
            .context("Failed to render frame")?;

        frame.save(&output_path).context("Failed to save frame")?;
        println!("Saved frame to {}", output_path.display());
    } else {
        // Extract all frames
        let output_dir = output.context("Output directory required")?;
        std::fs::create_dir_all(&output_dir).context("Failed to create output directory")?;

        let fps = container.fps();
        let frame_count = ((container.header.duration_ms as f64 * fps / 1000.0).floor() as u64).max(1);

        println!("Extracting {} frames to {}", frame_count, output_dir.display());

        for i in 0..frame_count {
            let timestamp_ms = (i as f64 * 1000.0 / fps) as u64;
            let frame = compositor
                .render_frame(timestamp_ms)
                .context("Failed to render frame")?;

            let frame_path = output_dir.join(format!("frame_{:06}.png", i));
            frame.save(&frame_path).context("Failed to save frame")?;

            if (i + 1) % 10 == 0 {
                println!("Extracted {} / {} frames", i + 1, frame_count);
            }
        }

        println!("Successfully extracted all frames");
    }

    Ok(())
}

fn print_info(container: &VaiContainer) {
    println!("\n=== VAI File Information ===");
    println!("Version: {}", container.header.version);
    println!("Resolution: {}x{}", container.header.width, container.header.height);
    println!(
        "Frame rate: {}/{} ({:.2} fps)",
        container.header.fps_num,
        container.header.fps_den,
        container.fps()
    );
    println!("Duration: {} ms ({:.2} seconds)", 
        container.header.duration_ms,
        container.header.duration_ms as f64 / 1000.0
    );
    println!("Assets: {}", container.assets.len());
    println!("Timeline entries: {}", container.timeline.len());

    // Calculate total compressed size
    let total_size: usize = container.assets.iter().map(|a| a.data_size()).sum();
    println!("Total compressed asset size: {} bytes ({:.2} KB)", 
        total_size,
        total_size as f64 / 1024.0
    );

    println!("\n=== Assets ===");
    for asset in &container.assets {
        println!(
            "  Asset {}: {}x{}, {} bytes",
            asset.id,
            asset.width,
            asset.height,
            asset.data_size()
        );
    }

    println!("\n=== Timeline (first 10 entries) ===");
    for (i, entry) in container.timeline.iter().take(10).enumerate() {
        println!(
            "  [{}] Asset {} from {}ms to {}ms at ({}, {}) z={}",
            i,
            entry.asset_id,
            entry.start_time_ms,
            entry.end_time_ms,
            entry.position_x,
            entry.position_y,
            entry.z_order
        );
    }
    if container.timeline.len() > 10 {
        println!("  ... and {} more entries", container.timeline.len() - 10);
    }
}
