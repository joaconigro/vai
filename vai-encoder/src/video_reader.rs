//! Video reading and frame extraction using FFmpeg

use crate::{Error, Result};
use ffmpeg_next as ffmpeg;
use image::{ImageBuffer, Rgba};
use std::sync::Once;

static FFMPEG_INIT: Once = Once::new();

/// Initialize FFmpeg (call once per application)
fn init_ffmpeg() {
    FFMPEG_INIT.call_once(|| {
        ffmpeg::init().expect("Failed to initialize FFmpeg");
    });
}

/// Video reader that extracts frames from video files
pub struct VideoReader {
    path: Option<String>,
    input: ffmpeg::format::context::Input,
    video_stream_index: usize,
    decoder: ffmpeg::codec::decoder::Video,
    scaler: Option<ffmpeg::software::scaling::Context>,
}

impl VideoReader {
    /// Opens a video file
    pub fn open(path: &str) -> Result<Self> {
        init_ffmpeg();

        let input = ffmpeg::format::input(&path)?;

        // Find the video stream
        let video_stream = input
            .streams()
            .best(ffmpeg::media::Type::Video)
            .ok_or(Error::NoVideoStream)?;

        let video_stream_index = video_stream.index();

        // Create decoder
        let context =
            ffmpeg::codec::context::Context::from_parameters(video_stream.parameters())?;
        let decoder = context.decoder().video()?;

        Ok(Self {
            path: Some(path.to_string()),
            input,
            video_stream_index,
            decoder,
            scaler: None,
        })
    }

    /// Returns the file path so the reader can be re-opened for a second pass
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// Gets the video width
    pub fn width(&self) -> u32 {
        self.decoder.width()
    }

    /// Gets the video height
    pub fn height(&self) -> u32 {
        self.decoder.height()
    }

    /// Gets the frame rate as a rational number (numerator, denominator)
    pub fn frame_rate(&self) -> (u32, u32) {
        let stream = self.input.stream(self.video_stream_index).unwrap();
        let rate = stream.rate();
        (rate.numerator() as u32, rate.denominator() as u32)
    }

    /// Gets the total duration in milliseconds
    pub fn duration_ms(&self) -> u64 {
        let stream = self.input.stream(self.video_stream_index).unwrap();
        let duration = stream.duration();
        let time_base = stream.time_base();

        if duration > 0 {
            (duration as f64 * time_base.numerator() as f64 / time_base.denominator() as f64
                * 1000.0) as u64
        } else {
            // Fallback to container duration
            let duration =
                self.input.duration() as f64 / ffmpeg::ffi::AV_TIME_BASE as f64 * 1000.0;
            duration as u64
        }
    }

    /// Ensures the scaler is initialized
    fn ensure_scaler(&mut self) -> Result<()> {
        if self.scaler.is_none() {
            self.scaler = Some(ffmpeg::software::scaling::Context::get(
                self.decoder.format(),
                self.decoder.width(),
                self.decoder.height(),
                ffmpeg::format::Pixel::RGB24,
                self.decoder.width(),
                self.decoder.height(),
                ffmpeg::software::scaling::Flags::BILINEAR,
            )?);
        }
        Ok(())
    }

    /// Converts a decoded video frame to an RGBA ImageBuffer
    fn frame_to_rgba(
        scaler: &mut ffmpeg::software::scaling::Context,
        decoded: &ffmpeg::frame::Video,
        width: u32,
        height: u32,
    ) -> Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
        let mut rgb_frame = ffmpeg::frame::Video::empty();
        scaler.run(decoded, &mut rgb_frame)?;

        let rgb_data = rgb_frame.data(0);
        let stride = rgb_frame.stride(0);
        let mut rgba_data = Vec::with_capacity((width * height * 4) as usize);

        for y in 0..height as usize {
            let row_start = y * stride;
            for x in 0..width as usize {
                let offset = row_start + x * 3;
                rgba_data.push(rgb_data[offset]);     // R
                rgba_data.push(rgb_data[offset + 1]); // G
                rgba_data.push(rgb_data[offset + 2]); // B
                rgba_data.push(255);                   // A
            }
        }

        ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width, height, rgba_data)
            .ok_or(Error::InvalidVideo)
    }

    /// Reads all frames from the video, processing each frame with the given callback.
    /// This avoids storing all frames in memory at once.
    pub fn read_frames_streaming<F>(&mut self, mut callback: F) -> Result<()>
    where
        F: FnMut(usize, ImageBuffer<Rgba<u8>, Vec<u8>>) -> Result<()>,
    {
        let width = self.decoder.width();
        let height = self.decoder.height();
        self.ensure_scaler()?;

        let mut frame_index: usize = 0;

        // Destructure self so we can iterate packets from `input` while
        // simultaneously sending them to `decoder`, without collecting
        // all packets into memory first.
        let VideoReader {
            input,
            video_stream_index,
            decoder,
            scaler,
            ..
        } = self;
        let stream_idx = *video_stream_index;

        for (stream, packet) in input.packets() {
            if stream.index() != stream_idx {
                continue;
            }

            decoder.send_packet(&packet)?;

            let mut decoded = ffmpeg::frame::Video::empty();
            while decoder.receive_frame(&mut decoded).is_ok() {
                if let Some(ref mut sc) = scaler {
                    let img = Self::frame_to_rgba(sc, &decoded, width, height)?;
                    callback(frame_index, img)?;
                    frame_index += 1;
                }
            }
        }

        // Flush decoder
        decoder.send_eof()?;
        let mut decoded = ffmpeg::frame::Video::empty();
        while decoder.receive_frame(&mut decoded).is_ok() {
            if let Some(ref mut sc) = scaler {
                let img = Self::frame_to_rgba(sc, &decoded, width, height)?;
                callback(frame_index, img)?;
                frame_index += 1;
            }
        }

        Ok(())
    }

    /// Reads all frames from the video into memory.
    /// WARNING: For large videos this may use excessive memory.
    /// Prefer `read_frames_streaming` for large files.
    pub fn read_frames(&mut self) -> Result<Vec<ImageBuffer<Rgba<u8>, Vec<u8>>>> {
        let mut frames = Vec::new();
        self.read_frames_streaming(|_idx, frame| {
            frames.push(frame);
            Ok(())
        })?;
        Ok(frames)
    }
}
