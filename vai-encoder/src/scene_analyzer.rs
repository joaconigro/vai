//! Scene analysis and motion detection

use crate::scene_detector::SceneSegment;
use crate::{avif_encoder, progress_tracker::ProgressTracker, EncoderConfig, Result};
use image::{ImageBuffer, Rgba, RgbaImage};
use std::thread;
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

    /// Analyzes frames and creates a VAI container (legacy, loads all frames into memory)
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

        let background = &frames[0];
        let background_data = avif_encoder::encode_avif(background, self.config.quality)?;
        let background_asset = Asset::new(0, width, height, background_data);

        let mut assets = vec![background_asset];
        let mut timeline = Vec::new();

        timeline.push(TimelineEntry::new(0, 0, duration_ms, 0, 0, 0));

        let ms_per_frame = if !frames.is_empty() {
            duration_ms / frames.len() as u64
        } else {
            duration_ms
        };

        let mut asset_id = 1;
        for (frame_idx, frame) in frames.iter().enumerate().skip(1) {
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
                        1,
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

    /// Analyzes frames in a streaming fashion (single-threaded, original behaviour).
    /// The first frame is used as the background. Subsequent frames are compared
    /// against it and only the diff regions are kept. This uses O(1) frame memory
    /// instead of O(N).
    pub fn analyze_streaming(
        &self,
        reader: &mut crate::VideoReader,
        width: u32,
        height: u32,
        fps_num: u32,
        fps_den: u32,
        duration_ms: u64,
    ) -> Result<VaiContainer> {
        let mut background: Option<RgbaImage> = None;
        let mut assets: Vec<Asset> = Vec::new();
        let mut timeline: Vec<TimelineEntry> = Vec::new();
        let mut asset_id: u32 = 1;
        let mut total_frames: usize = 0;

        let estimated_frame_count =
            ((duration_ms as f64 * fps_num as f64) / (fps_den as f64 * 1000.0)).ceil() as u64;
        let ms_per_frame = if estimated_frame_count > 0 {
            duration_ms / estimated_frame_count
        } else {
            duration_ms
        };

        let quality = self.config.quality;
        let config = self.config.clone();

        let progress = ProgressTracker::new(estimated_frame_count, "Encoding frames:");

        reader.read_frames_streaming(|frame_idx, frame| {
            total_frames = frame_idx + 1;

            if frame_idx == 0 {
                let background_data = avif_encoder::encode_avif(&frame, quality)?;
                let background_asset = Asset::new(0, width, height, background_data);
                assets.push(background_asset);
                timeline.push(TimelineEntry::new(0, 0, duration_ms, 0, 0, 0));
                background = Some(frame);
            } else if let Some(ref bg) = background {
                let diff_regions = find_diff_regions(&config, bg, &frame);

                for (x, y, region_img) in diff_regions {
                    let region_data = avif_encoder::encode_avif(&region_img, quality)?;
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
                        1,
                    ));

                    asset_id += 1;
                }
            }

            progress.increment_and_report(50);

            Ok(())
        })?;

        println!(
            "  Total: {} frames, {} assets, {} timeline entries",
            total_frames,
            assets.len(),
            timeline.len()
        );

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

    /// Two-pass chunked parallel encoding:
    ///
    /// **Pass 1** – Scene detection (already done by caller via `scene_detector`).
    /// **Pass 2** – Read frames a second time. Raw frames are buffered in
    ///   chunks of up to `CHUNK_SIZE`. Each time the buffer fills (or a segment
    ///   boundary / end-of-stream is reached) the chunk is encoded in parallel
    ///   across all CPU cores, only the compact AVIF results are kept, and the
    ///   raw frames are freed.  This bounds peak memory to roughly
    ///   `CHUNK_SIZE × frame_size` plus the (much smaller) accumulated AVIF
    ///   assets, and needs no temporary files on disk.
    pub fn analyze_parallel(
        &self,
        reader: &mut crate::VideoReader,
        segments: Vec<SceneSegment>,
        width: u32,
        height: u32,
        fps_num: u32,
        fps_den: u32,
        duration_ms: u64,
    ) -> Result<VaiContainer> {
        /// Maximum raw frames to buffer before flushing a parallel encode.
        /// At 1080p RGBA (~8 MB/frame) 500 frames ≈ 4 GB peak.
        const CHUNK_SIZE: usize = 500;

        let num_segments = segments.len();
        let n_threads = num_cpus::get().max(1);
        println!(
            "  Pass 2: encoding {} scene segment(s) in parallel ({} threads, chunk size {}) …",
            num_segments, n_threads, CHUNK_SIZE
        );

        let estimated_frame_count =
            ((duration_ms as f64 * fps_num as f64) / (fps_den as f64 * 1000.0)).ceil() as u64;
        let ms_per_frame = if estimated_frame_count > 0 {
            duration_ms as f64 / estimated_frame_count as f64
        } else {
            duration_ms as f64
        };

        let quality = self.config.quality;
        let config = self.config.clone();

        let mut all_assets: Vec<Asset> = Vec::new();
        let mut all_timeline: Vec<TimelineEntry> = Vec::new();
        let mut next_asset_id: u32 = 0;

        // ── Encode each segment's background up-front ──
        println!("  Encoding {} background(s) …", num_segments);
        for seg in &segments {
            let bg_data = avif_encoder::encode_avif(&seg.background, quality)?;
            all_assets.push(Asset::new(next_asset_id, width, height, bg_data));

            let scene_start_ms = (seg.start_frame as f64 * ms_per_frame) as u64;
            let scene_end_ms = if seg.end_frame == usize::MAX {
                duration_ms
            } else {
                (seg.end_frame as f64 * ms_per_frame) as u64
            };
            all_timeline.push(TimelineEntry::new(
                next_asset_id, scene_start_ms, scene_end_ms, 0, 0, 0,
            ));
            next_asset_id += 1;
        }

        // ── Stream frames, encoding in fixed-size chunks ──
        // Buffer: (global_frame_idx, segment_index, raw RGBA image)
        let mut chunk: Vec<(usize, usize, RgbaImage)> = Vec::with_capacity(CHUNK_SIZE);
        let progress = ProgressTracker::new(estimated_frame_count, "Processing frames:");

        reader.read_frames_streaming(|frame_idx, frame| {
            // Find the segment this frame belongs to
            for (seg_idx, seg) in segments.iter().enumerate() {
                if frame_idx >= seg.start_frame && frame_idx < seg.end_frame {
                    // First frame of each segment is the background – already encoded
                    if frame_idx != seg.start_frame {
                        chunk.push((frame_idx, seg_idx, frame));
                    }
                    break;
                }
            }

            // Flush the chunk when full
            if chunk.len() >= CHUNK_SIZE {
                flush_chunk(
                    &mut chunk,
                    &segments,
                    &config,
                    quality,
                    ms_per_frame,
                    n_threads,
                    &mut all_assets,
                    &mut all_timeline,
                    &mut next_asset_id,
                )?;
            }

            progress.increment_and_report(100);
            Ok(())
        })?;

        // Flush any remaining frames
        if !chunk.is_empty() {
            flush_chunk(
                &mut chunk,
                &segments,
                &config,
                quality,
                ms_per_frame,
                n_threads,
                &mut all_assets,
                &mut all_timeline,
                &mut next_asset_id,
            )?;
        }

        println!(
            "  Total: {} assets, {} timeline entries",
            all_assets.len(),
            all_timeline.len()
        );

        let header = VaiHeader::new(
            width,
            height,
            fps_num,
            fps_den,
            duration_ms,
            all_assets.len() as u32,
            all_timeline.len() as u32,
        );

        Ok(VaiContainer::new(header, all_assets, all_timeline))
    }

    /// Finds regions that differ from the background
    fn find_diff_regions(
        &self,
        background: &RgbaImage,
        frame: &RgbaImage,
    ) -> Vec<(u32, u32, RgbaImage)> {
        find_diff_regions(&self.config, background, frame)
    }

    /// Finds the bounding box of all true values in the mask
    fn find_bounding_box(&self, mask: &[Vec<bool>]) -> (u32, u32, u32, u32) {
        find_bounding_box(mask)
    }
}

/// Encodes a chunk of buffered raw frames in parallel, appends the compact
/// AVIF results to the output vectors, then clears the buffer to free memory.
fn flush_chunk(
    chunk: &mut Vec<(usize, usize, RgbaImage)>,
    segments: &[SceneSegment],
    config: &EncoderConfig,
    quality: u8,
    ms_per_frame: f64,
    n_threads: usize,
    all_assets: &mut Vec<Asset>,
    all_timeline: &mut Vec<TimelineEntry>,
    next_asset_id: &mut u32,
) -> crate::Result<()> {
    if chunk.is_empty() {
        return Ok(());
    }

    // Each thread will produce a list of encoded regions.
    type RegionResult = (usize, u32, u32, u32, u32, Vec<u8>); // (frame_idx, x, y, w, h, avif_data)

    let per_thread = (chunk.len() + n_threads - 1) / n_threads;

    let results: Vec<crate::Result<Vec<RegionResult>>> = thread::scope(|scope| {
        let handles: Vec<_> = chunk
            .chunks(per_thread)
            .map(|sub| {
                scope.spawn(move || -> crate::Result<Vec<RegionResult>> {
                    let mut thread_results = Vec::new();
                    for (frame_idx, seg_idx, frame) in sub {
                        let bg = &segments[*seg_idx].background;
                        let diff_regions = find_diff_regions(config, bg, frame);
                        for (x, y, region_img) in diff_regions {
                            let data = avif_encoder::encode_avif(&region_img, quality)?;
                            thread_results.push((
                                *frame_idx,
                                x,
                                y,
                                region_img.width(),
                                region_img.height(),
                                data,
                            ));
                        }
                    }
                    Ok(thread_results)
                })
            })
            .collect();

        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    // Collect the compact results and assign asset IDs
    for result in results {
        let regions = result?;
        for (frame_idx, x, y, rw, rh, data) in regions {
            all_assets.push(Asset::new(*next_asset_id, rw, rh, data));

            let start_time = (frame_idx as f64 * ms_per_frame) as u64;
            let end_time = start_time + ms_per_frame as u64;

            all_timeline.push(TimelineEntry::new(
                *next_asset_id,
                start_time,
                end_time,
                x as i32,
                y as i32,
                1,
            ));

            *next_asset_id += 1;
        }
    }

    // Free all raw frames
    chunk.clear();
    Ok(())
}

/// Finds regions that differ from the background (free function for use in closures)
fn find_diff_regions(
    config: &EncoderConfig,
    background: &RgbaImage,
    frame: &RgbaImage,
) -> Vec<(u32, u32, RgbaImage)> {
    let width = background.width();
    let height = background.height();

    let mut diff_mask = vec![vec![false; width as usize]; height as usize];
    let mut has_diff = false;

    for y in 0..height {
        for x in 0..width {
            let bg_pixel = background.get_pixel(x, y);
            let frame_pixel = frame.get_pixel(x, y);

            let diff = pixel_difference(bg_pixel, frame_pixel);
            if diff > config.threshold {
                diff_mask[y as usize][x as usize] = true;
                has_diff = true;
            }
        }
    }

    if !has_diff {
        return Vec::new();
    }

    let (min_x, min_y, max_x, max_y) = find_bounding_box(&diff_mask);

    let region_width = max_x - min_x + 1;
    let region_height = max_y - min_y + 1;

    if region_width * region_height < config.min_region_size {
        return Vec::new();
    }

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
fn find_bounding_box(mask: &[Vec<bool>]) -> (u32, u32, u32, u32) {
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

/// Calculates the difference between two pixels
fn pixel_difference(a: &Rgba<u8>, b: &Rgba<u8>) -> u8 {
    let dr = (a[0] as i32 - b[0] as i32).abs();
    let dg = (a[1] as i32 - b[1] as i32).abs();
    let db = (a[2] as i32 - b[2] as i32).abs();
    ((dr + dg + db) / 3) as u8
}
