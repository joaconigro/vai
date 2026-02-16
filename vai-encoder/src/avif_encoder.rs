//! AVIF encoding functionality

use crate::{Error, Result};
use image::RgbaImage;
use ravif::{Encoder, Img, RGBA8};

/// Encode an RGBA image to AVIF, dispatching to the FFmpeg backend when
/// `use_ffmpeg` is true (and falling back to ravif if FFmpeg is unavailable).
pub fn encode_avif_auto(image: &RgbaImage, quality: u8, use_ffmpeg: bool) -> Result<Vec<u8>> {
    if use_ffmpeg {
        match crate::ffmpeg_encoder::encode_avif_ffmpeg(image, quality) {
            Ok(data) => return Ok(data),
            Err(e) => {
                eprintln!("FFmpeg AV1 encode failed ({e}), falling back to ravif");
            }
        }
    }
    encode_avif(image, quality)
}

/// Encodes an RGBA image to AVIF format using the pure-Rust ravif encoder
pub fn encode_avif(image: &RgbaImage, quality: u8) -> Result<Vec<u8>> {
    let width = image.width() as usize;
    let height = image.height() as usize;

    // Prepare the image data for ravif
    let img = Img::new(image.as_raw().as_rgba(), width, height);

    // Create encoder
    let encoder = Encoder::new()
        .with_quality(quality as f32)
        .with_speed(4)
        .with_alpha_quality(quality as f32)
        .with_num_threads(Some(num_cpus::get()));

    // Encode to AVIF
    let encoded = encoder.encode_rgba(img)
        .map_err(|e| Error::AvifEncode(format!("{:?}", e)))?;

    Ok(encoded.avif_file)
}

// Helper trait to convert byte slices to RGBA slices
trait AsRgba {
    fn as_rgba(&self) -> &[RGBA8];
}

impl AsRgba for [u8] {
    fn as_rgba(&self) -> &[RGBA8] {
        // Ensure the slice has valid length (multiple of 4)
        assert_eq!(
            self.len() % 4,
            0,
            "Byte slice length must be multiple of 4 for RGBA conversion"
        );
        
        // Check alignment
        let align_offset = self.as_ptr().align_offset(std::mem::align_of::<RGBA8>());
        assert_eq!(
            align_offset, 0,
            "Byte slice must be properly aligned for RGBA8"
        );

        unsafe {
            std::slice::from_raw_parts(
                self.as_ptr() as *const RGBA8,
                self.len() / 4,
            )
        }
    }
}
