//! Asset data structures for VAI format

/// Represents a single AVIF-compressed image asset
#[derive(Debug, Clone)]
pub struct Asset {
    /// Unique identifier for this asset
    pub id: u32,
    /// Width of the asset in pixels
    pub width: u32,
    /// Height of the asset in pixels
    pub height: u32,
    /// AVIF-compressed image data
    pub data: Vec<u8>,
}

impl Asset {
    /// Creates a new asset
    pub fn new(id: u32, width: u32, height: u32, data: Vec<u8>) -> Self {
        Self {
            id,
            width,
            height,
            data,
        }
    }

    /// Returns the size of the asset data in bytes
    pub fn data_size(&self) -> usize {
        self.data.len()
    }
}
