//! Frame compositor for blending layers

use crate::{avif_decoder, Error, Result};
use image::{ImageBuffer, Rgba, RgbaImage};
use vai_core::VaiContainer;

/// Frame compositor that can render frames from a VAI container
pub struct FrameCompositor {
    container: VaiContainer,
    decoded_assets: std::collections::HashMap<u32, RgbaImage>,
}

impl FrameCompositor {
    /// Creates a new frame compositor for the given container
    pub fn new(container: VaiContainer) -> Self {
        Self {
            container,
            decoded_assets: std::collections::HashMap::new(),
        }
    }

    /// Decodes and caches an asset
    fn decode_asset(&mut self, asset_id: u32) -> Result<&RgbaImage> {
        // Check if already cached
        if !self.decoded_assets.contains_key(&asset_id) {
            // Find the asset
            let asset = self
                .container
                .get_asset(asset_id)
                .ok_or(Error::AssetNotFound(asset_id))?;

            // Decode the AVIF data
            let image = avif_decoder::decode_avif(&asset.data)?;
            self.decoded_assets.insert(asset_id, image);
        }

        Ok(self.decoded_assets.get(&asset_id).unwrap())
    }

    /// Renders a frame at the given timestamp
    pub fn render_frame(&mut self, timestamp_ms: u64) -> Result<RgbaImage> {
        let width = self.container.header.width;
        let height = self.container.header.height;

        // Create a blank frame
        let mut frame = ImageBuffer::from_pixel(width, height, Rgba([0, 0, 0, 255]));

        // Get active entries sorted by z_order (collect to avoid borrow issues)
        let entries: Vec<_> = self.container.get_active_entries(timestamp_ms)
            .into_iter()
            .map(|e| (e.asset_id, e.position_x, e.position_y))
            .collect();

        // Composite each layer
        for (asset_id, position_x, position_y) in entries {
            let asset_image = self.decode_asset(asset_id)?;

            // Overlay the asset at the specified position
            overlay_image(&mut frame, asset_image, position_x, position_y);
        }

        Ok(frame)
    }

    /// Gets a reference to the underlying container
    pub fn container(&self) -> &VaiContainer {
        &self.container
    }
}

/// Overlays one image onto another at the specified position
fn overlay_image(base: &mut RgbaImage, overlay: &RgbaImage, x: i32, y: i32) {
    let base_width = base.width() as i32;
    let base_height = base.height() as i32;
    let overlay_width = overlay.width() as i32;
    let overlay_height = overlay.height() as i32;

    // Calculate the region to copy
    let src_x_start = 0.max(-x);
    let src_y_start = 0.max(-y);
    let src_x_end = overlay_width.min(base_width - x);
    let src_y_end = overlay_height.min(base_height - y);

    if src_x_start >= src_x_end || src_y_start >= src_y_end {
        return; // Nothing to overlay
    }

    // Copy pixels with alpha blending
    for src_y in src_y_start..src_y_end {
        for src_x in src_x_start..src_x_end {
            let dest_x = (x + src_x) as u32;
            let dest_y = (y + src_y) as u32;

            if dest_x < base.width() && dest_y < base.height() {
                let overlay_pixel = overlay.get_pixel(src_x as u32, src_y as u32);
                let base_pixel = base.get_pixel(dest_x, dest_y);

                // Alpha blending
                let alpha = overlay_pixel[3] as f32 / 255.0;
                let inv_alpha = 1.0 - alpha;

                let blended = Rgba([
                    (overlay_pixel[0] as f32 * alpha + base_pixel[0] as f32 * inv_alpha) as u8,
                    (overlay_pixel[1] as f32 * alpha + base_pixel[1] as f32 * inv_alpha) as u8,
                    (overlay_pixel[2] as f32 * alpha + base_pixel[2] as f32 * inv_alpha) as u8,
                    255,
                ]);

                base.put_pixel(dest_x, dest_y, blended);
            }
        }
    }
}
