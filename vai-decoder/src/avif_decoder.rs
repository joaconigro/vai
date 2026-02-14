//! AVIF decoding functionality

use crate::{Error, Result};
use image::{ImageBuffer, Rgba};

/// Decodes AVIF data into an RGBA image buffer
pub fn decode_avif(data: &[u8]) -> Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    let img = libavif_image::read(data)
        .map_err(|e| Error::AvifDecode(format!("{:?}", e)))?;
    
    // Convert to RGBA8
    let rgba_img = img.to_rgba8();
    Ok(rgba_img)
}
