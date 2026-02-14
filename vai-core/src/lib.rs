//! VAI Core Library
//!
//! This library provides the core data structures and binary container format
//! for the VAI (Video with Artificial Intelligence) video format.

pub mod asset;
pub mod container;
pub mod timeline;

pub use asset::Asset;
pub use container::{VaiContainer, VaiHeader};
pub use timeline::TimelineEntry;

/// Result type for vai-core operations
pub type Result<T> = std::result::Result<T, Error>;

/// Error types for vai-core operations
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid magic bytes, expected 'VAI\\0'")]
    InvalidMagic,

    #[error("Unsupported version: {0}")]
    UnsupportedVersion(u16),

    #[error("Invalid asset ID: {0}")]
    InvalidAssetId(u32),

    #[error("Invalid timeline entry")]
    InvalidTimelineEntry,

    #[error("Asset not found: {0}")]
    AssetNotFound(u32),
}
