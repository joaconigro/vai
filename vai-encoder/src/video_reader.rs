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
        let context = ffmpeg::codec::context::Context::from_parameters(video_stream.parameters())?;
        let decoder = context.decoder().video()?;

        Ok(Self {
            input,
            video_stream_index,
            decoder,
            scaler: None,
        })
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
            (duration as f64 * time_base.numerator() as f64 / time_base.denominator() as f64 * 1000.0) as u64
        } else {
            // Fallback to container duration
            let duration = self.input.duration() as f64 / ffmpeg::ffi::AV_TIME_BASE as f64 * 1000.0;
            duration as u64
        }
    }

    /// Reads all frames from the video
    pub fn read_frames(&mut self) -> Result<Vec<ImageBuffer<Rgba<u8>, Vec<u8>>>> {
        let mut frames = Vec::new();
        
        // Setup scaler for RGBA conversion
        if self.scaler.is_none() {
            self.scaler = Some(ffmpeg::software::scaling::Context::get(
                self.decoder.format(),
                self.decoder.width(),
                self.decoder.height(),
                ffmpeg::format::Pixel::RGBA,
                self.decoder.width(),
                self.decoder.height(),
                ffmpeg::software::scaling::Flags::BILINEAR,
            )?);
        }

        let mut receive_and_process_decoded_frames = |decoder: &mut ffmpeg::decoder::Video| -> Result<()> {
            let mut decoded = ffmpeg::frame::Video::empty();
            while decoder.receive_frame(&mut decoded).is_ok() {
                let mut rgb_frame = ffmpeg::frame::Video::empty();
                if let Some(ref mut scaler) = self.scaler {
                    scaler.run(&decoded, &mut rgb_frame)?;
                    
                    // Convert frame to ImageBuffer
                    let data = rgb_frame.data(0).to_vec();
                    let img = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(
                        rgb_frame.width(),
                        rgb_frame.height(),
                        data,
                    )
                    .ok_or_else(|| Error::InvalidVideo)?;
                    
                    frames.push(img);
                }
            }
            Ok(())
        };

        // Read packets and decode
        for (stream, packet) in self.input.packets() {
            if stream.index() == self.video_stream_index {
                self.decoder.send_packet(&packet)?;
                receive_and_process_decoded_frames(&mut self.decoder)?;
            }
        }

        // Flush decoder
        self.decoder.send_eof()?;
        receive_and_process_decoded_frames(&mut self.decoder)?;

        Ok(frames)
    }
}
