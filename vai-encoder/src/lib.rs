//! VAI Encoder Library
//!
//! This library provides functionality to encode video files into VAI format.

pub mod avif_encoder;
pub mod scene_analyzer;
pub mod video_reader;

pub use scene_analyzer::SceneAnalyzer;
pub use video_reader::VideoReader;

/// Result type for vai-encoder operations
pub type Result<T> = std::result::Result<T, Error>;

/// Error types for vai-encoder operations
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("VAI core error: {0}")]
    Core(#[from] vai_core::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Image error: {0}")]
    Image(#[from] image::ImageError),

    #[error("FFmpeg error: {0}")]
    Ffmpeg(#[from] ffmpeg_next::Error),

    #[error("AVIF encode error: {0}")]
    AvifEncode(String),

    #[error("Invalid video file")]
    InvalidVideo,

    #[error("No video stream found")]
    NoVideoStream,
}

/// Encoder configuration
#[derive(Debug, Clone)]
pub struct EncoderConfig {
    /// AVIF encoding quality (0-100)
    pub quality: u8,
    /// Optional output frame rate (None = use source FPS)
    pub fps: Option<f64>,
    /// Motion detection threshold (0-255)
    pub threshold: u8,
    /// Minimum region size in pixels
    pub min_region_size: u32,
}

impl Default for EncoderConfig {
    fn default() -> Self {
        Self {
            quality: 80,
            fps: None,
            threshold: 30,
            min_region_size: 64,
        }
    }
}
