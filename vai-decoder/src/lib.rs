//! VAI Decoder Library
//!
//! This library provides functionality to decode VAI video files back into frames.

pub mod avif_decoder;
pub mod frame_compositor;

pub use frame_compositor::FrameCompositor;

/// Result type for vai-decoder operations
pub type Result<T> = std::result::Result<T, Error>;

/// Error types for vai-decoder operations
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("VAI core error: {0}")]
    Core(#[from] vai_core::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Image error: {0}")]
    Image(#[from] image::ImageError),

    #[error("AVIF decode error: {0}")]
    AvifDecode(String),

    #[error("Asset not found: {0}")]
    AssetNotFound(u32),

    #[error("Invalid timestamp: {0}")]
    InvalidTimestamp(u64),
}
