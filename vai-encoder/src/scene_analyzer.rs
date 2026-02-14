//! Scene analysis and motion detection

use crate::{avif_encoder, EncoderConfig, Result};
use image::{ImageBuffer, Rgba, RgbaImage};
use vai_core::{Asset, TimelineEntry, VaiContainer, VaiHeader};

/// Scene analyzer that extracts background and motion regions
pub struct SceneAnalyzer {
    config: EncoderConfig,
}

impl SceneAnalyzer {
    /// Creates a new scene analyzer with the given configuration
    pub fn new(config: EncoderConfig) -> Self {
        Self { config }
    }

    /// Analyzes frames and creates a VAI container
    pub fn analyze(
        &self,
        frames: Vec<RgbaImage>,
        width: u32,
        height: u32,
        fps_num: u32,
        fps_den: u32,
        duration_ms: u64,
    ) -> Result<VaiContainer> {
        if frames.is_empty() {
            return Err(crate::Error::InvalidVideo);
        }

        // For simplicity, use the first frame as the background
        // In a more sophisticated implementation, we would compute a median/mode background
        let background = &frames[0];

        // Encode background as asset 0
        let background_data = avif_encoder::encode_avif(background, self.config.quality)?;
        let background_asset = Asset::new(0, width, height, background_data);

        let mut assets = vec![background_asset];
        let mut timeline = Vec::new();

        // Add background to timeline (entire duration, z_order = 0)
        timeline.push(TimelineEntry::new(0, 0, duration_ms, 0, 0, 0));

        // For a basic implementation, we'll create overlay assets for each frame that differs significantly
        // from the background. In production, we'd implement proper motion detection and region extraction.
        let ms_per_frame = if !frames.is_empty() {
            duration_ms / frames.len() as u64
        } else {
            duration_ms
        };

        let mut asset_id = 1;
        for (frame_idx, frame) in frames.iter().enumerate().skip(1) {
            // Compute difference from background
            let diff_regions = self.find_diff_regions(background, frame);

            if !diff_regions.is_empty() {
                for (x, y, region_img) in diff_regions {
                    let region_data = avif_encoder::encode_avif(&region_img, self.config.quality)?;
                    let region_asset = Asset::new(
                        asset_id,
                        region_img.width(),
                        region_img.height(),
                        region_data,
                    );
                    assets.push(region_asset);

                    let start_time = (frame_idx as u64) * ms_per_frame;
                    let end_time = start_time + ms_per_frame;

                    timeline.push(TimelineEntry::new(
                        asset_id,
                        start_time,
                        end_time,
                        x as i32,
                        y as i32,
                        1, // Overlays have z_order > 0
                    ));

                    asset_id += 1;
                }
            }
        }

        let header = VaiHeader::new(
            width,
            height,
            fps_num,
            fps_den,
            duration_ms,
            assets.len() as u32,
            timeline.len() as u32,
        );

        Ok(VaiContainer::new(header, assets, timeline))
    }

    /// Finds regions that differ from the background
    fn find_diff_regions(&self, background: &RgbaImage, frame: &RgbaImage) -> Vec<(u32, u32, RgbaImage)> {
        let width = background.width();
        let height = background.height();

        // Create a difference mask
        let mut diff_mask = vec![vec![false; width as usize]; height as usize];
        let mut has_diff = false;

        for y in 0..height {
            for x in 0..width {
                let bg_pixel = background.get_pixel(x, y);
                let frame_pixel = frame.get_pixel(x, y);

                let diff = pixel_difference(bg_pixel, frame_pixel);
                if diff > self.config.threshold {
                    diff_mask[y as usize][x as usize] = true;
                    has_diff = true;
                }
            }
        }

        if !has_diff {
            return Vec::new();
        }

        // For simplicity, create a single bounding box around all differences
        // In production, we'd use connected component analysis to find separate regions
        let (min_x, min_y, max_x, max_y) = self.find_bounding_box(&diff_mask);

        let region_width = max_x - min_x + 1;
        let region_height = max_y - min_y + 1;

        // Check minimum size
        if region_width * region_height < self.config.min_region_size {
            return Vec::new();
        }

        // Extract the region
        let mut region_img = ImageBuffer::new(region_width, region_height);
        for y in 0..region_height {
            for x in 0..region_width {
                let src_x = min_x + x;
                let src_y = min_y + y;
                let pixel = frame.get_pixel(src_x, src_y);
                region_img.put_pixel(x, y, *pixel);
            }
        }

        vec![(min_x, min_y, region_img)]
    }

    /// Finds the bounding box of all true values in the mask
    fn find_bounding_box(&self, mask: &[Vec<bool>]) -> (u32, u32, u32, u32) {
        let height = mask.len();
        let width = if height > 0 { mask[0].len() } else { 0 };

        let mut min_x = width;
        let mut min_y = height;
        let mut max_x = 0;
        let mut max_y = 0;

        for (y, row) in mask.iter().enumerate() {
            for (x, &val) in row.iter().enumerate() {
                if val {
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }

        (min_x as u32, min_y as u32, max_x as u32, max_y as u32)
    }
}

/// Calculates the difference between two pixels
fn pixel_difference(a: &Rgba<u8>, b: &Rgba<u8>) -> u8 {
    let dr = (a[0] as i32 - b[0] as i32).abs();
    let dg = (a[1] as i32 - b[1] as i32).abs();
    let db = (a[2] as i32 - b[2] as i32).abs();
    ((dr + dg + db) / 3) as u8
}
