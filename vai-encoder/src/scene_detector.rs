//! Scene change detection for multi-pass encoding
//!
//! First pass: scan all frames to detect background/scene changes.
//! This produces a list of `SceneSegment`s, each with a background frame
//! and a time range. The segments can then be encoded in parallel.

use crate::{Result, VideoReader};
use image::{Rgba, RgbaImage};

/// A detected scene segment with its background and frame range
#[derive(Debug, Clone)]
pub struct SceneSegment {
    /// Index of the first frame in this scene
    pub start_frame: usize,
    /// Index one past the last frame in this scene (exclusive)
    pub end_frame: usize,
    /// The background image for this scene
    pub background: RgbaImage,
}

impl SceneSegment {
    /// Number of frames in this segment
    pub fn frame_count(&self) -> usize {
        self.end_frame - self.start_frame
    }
}

/// Configuration for scene detection
#[derive(Debug, Clone)]
pub struct SceneDetectorConfig {
    /// Per-pixel difference threshold (0-255) used to decide if a pixel changed
    pub pixel_threshold: u8,
    /// Fraction of pixels that must differ to trigger a scene change (0.0 - 1.0)
    pub scene_change_ratio: f64,
}

impl Default for SceneDetectorConfig {
    fn default() -> Self {
        Self {
            pixel_threshold: 40,
            scene_change_ratio: 0.35,
        }
    }
}

/// Detects scene changes across the entire video.
///
/// This is the *first pass*: it reads every frame but only keeps the background
/// images and the frame indices where scene changes occur.
pub fn detect_scenes(
    reader: &mut VideoReader,
    config: &SceneDetectorConfig,
) -> Result<Vec<SceneSegment>> {
    let mut segments: Vec<SceneSegment> = Vec::new();
    let mut current_bg: Option<RgbaImage> = None;
    let mut scene_start: usize = 0;

    let pixel_threshold = config.pixel_threshold;
    let scene_change_ratio = config.scene_change_ratio;

    reader.read_frames_streaming(|frame_idx, frame| {
        match current_bg {
            None => {
                // Very first frame → start first scene
                current_bg = Some(frame);
                scene_start = 0;
            }
            Some(ref bg) => {
                let changed_ratio = compute_change_ratio(bg, &frame, pixel_threshold);

                if changed_ratio >= scene_change_ratio {
                    // Scene change detected – close the current segment
                    segments.push(SceneSegment {
                        start_frame: scene_start,
                        end_frame: frame_idx,
                        background: bg.clone(),
                    });
                    // Start a new scene with this frame as background
                    current_bg = Some(frame);
                    scene_start = frame_idx;
                }
            }
        }

        if (frame_idx + 1) % 200 == 0 {
            println!(
                "  Scene detection: scanned {} frames, {} scenes so far",
                frame_idx + 1,
                segments.len() + 1
            );
        }

        Ok(())
    })?;

    // Close the last segment
    if let Some(bg) = current_bg {
        // We don't know the exact last frame index inside the callback,
        // so we use a sentinel that the caller will clamp.
        segments.push(SceneSegment {
            start_frame: scene_start,
            end_frame: usize::MAX, // will be clamped by caller
            background: bg,
        });
    }

    Ok(segments)
}

/// Computes the fraction of pixels that differ beyond `threshold`.
fn compute_change_ratio(a: &RgbaImage, b: &RgbaImage, threshold: u8) -> f64 {
    let width = a.width().min(b.width());
    let height = a.height().min(b.height());
    let total = (width as u64) * (height as u64);
    if total == 0 {
        return 0.0;
    }

    let mut changed: u64 = 0;

    for y in 0..height {
        for x in 0..width {
            let pa = a.get_pixel(x, y);
            let pb = b.get_pixel(x, y);
            if pixel_difference(pa, pb) > threshold {
                changed += 1;
            }
        }
    }

    changed as f64 / total as f64
}

/// Average channel difference between two pixels
fn pixel_difference(a: &Rgba<u8>, b: &Rgba<u8>) -> u8 {
    let dr = (a[0] as i32 - b[0] as i32).abs();
    let dg = (a[1] as i32 - b[1] as i32).abs();
    let db = (a[2] as i32 - b[2] as i32).abs();
    ((dr + dg + db) / 3) as u8
}