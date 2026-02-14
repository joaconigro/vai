//! VAI VLC Plugin
//!
//! A VLC media player plugin that allows playback of VAI video files.
//! This plugin implements VLC's demuxer API to decode and render VAI format.

mod vlc_bindings;

use image::RgbaImage;
use std::io::Cursor;
use std::os::raw::{c_int, c_void};
use std::panic;
use std::ptr;
use vai_core::VaiContainer;
use vai_decoder::FrameCompositor;
use vlc_bindings::*;

/// Plugin private data
struct DemuxSys {
    compositor: FrameCompositor,
    es_id: *mut es_out_id_t,
    current_frame: u64,
    fps: f64,
    duration_ms: u64,
    width: u32,
    height: u32,
    total_frames: u64,
}

/// VLC module entry point - Open function
///
/// This is called by VLC to check if we can handle the given stream.
#[no_mangle]
pub unsafe extern "C" fn Open(obj: *mut vlc_object_t) -> c_int {
    let result = panic::catch_unwind(|| unsafe { open_impl(obj) });
    match result {
        Ok(code) => code,
        Err(_) => {
            eprintln!("VAI plugin: panic in Open");
            VLC_EGENERIC
        }
    }
}

unsafe fn open_impl(obj: *mut vlc_object_t) -> c_int {
    if obj.is_null() {
        return VLC_EGENERIC;
    }
    
    let demux = obj as *mut demux_t;
    let stream = (*demux).s;
    let es_out = (*demux).out;
    
    if stream.is_null() || es_out.is_null() {
        return VLC_EGENERIC;
    }
    
    // Probe: Read first 4 bytes to check magic
    let mut magic = [0u8; 4];
    let bytes_read = stream_Read(stream, magic.as_mut_ptr() as *mut c_void, 4);
    if bytes_read != 4 {
        return VLC_EGENERIC;
    }
    
    // Check if it's a VAI file
    if magic != [b'V', b'A', b'I', 0] {
        return VLC_EGENERIC;
    }
    
    // Seek back to start
    if stream_Seek(stream, 0) != VLC_SUCCESS {
        return VLC_EGENERIC;
    }
    
    // Read entire file into buffer
    let file_size = stream_GetSize(stream);
    if file_size == 0 || file_size > 1024 * 1024 * 1024 {
        // Sanity check: no empty files or files > 1GB
        return VLC_EGENERIC;
    }
    
    let mut buffer = vec![0u8; file_size as usize];
    let bytes_read = stream_Read(stream, buffer.as_mut_ptr() as *mut c_void, file_size as usize);
    if bytes_read != file_size as isize {
        return VLC_EGENERIC;
    }
    
    // Parse VAI container
    let container = match VaiContainer::read(Cursor::new(buffer)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("VAI plugin: failed to parse container: {}", e);
            return VLC_EGENERIC;
        }
    };
    
    // Create frame compositor
    let compositor = FrameCompositor::new(container.clone());
    
    // Get video parameters
    let width = container.header.width;
    let height = container.header.height;
    let fps_num = container.header.fps_num;
    let fps_den = container.header.fps_den;
    let duration_ms = container.header.duration_ms;
    let fps = container.fps();
    
    // Calculate total frames
    let total_frames = ((duration_ms as f64 * fps) / 1000.0).ceil() as u64;
    
    // Set up ES format for RGBA video
    let mut fmt: es_format_t = std::mem::zeroed();
    es_format_Init(&mut fmt, VIDEO_ES, VLC_CODEC_RGBA);
    
    fmt.video.i_width = width;
    fmt.video.i_height = height;
    fmt.video.i_visible_width = width;
    fmt.video.i_visible_height = height;
    fmt.video.i_sar_num = 1;
    fmt.video.i_sar_den = 1;
    fmt.video.i_frame_rate = fps_num;
    fmt.video.i_frame_rate_base = fps_den;
    
    // Register the elementary stream
    let es_id = es_out_Add(es_out, &fmt);
    es_format_Clean(&mut fmt);
    
    if es_id.is_null() {
        return VLC_EGENERIC;
    }
    
    // Allocate and initialize plugin private data
    let sys = Box::new(DemuxSys {
        compositor,
        es_id,
        current_frame: 0,
        fps,
        duration_ms,
        width,
        height,
        total_frames,
    });
    
    // Store private data in demux_t
    (*demux).p_sys = Box::into_raw(sys) as *mut c_void;
    
    // Set callbacks
    (*demux).pf_demux = Some(Demux);
    (*demux).pf_control = Some(Control);
    
    VLC_SUCCESS
}

/// Close function - cleanup
#[no_mangle]
pub unsafe extern "C" fn Close(obj: *mut vlc_object_t) {
    let _ = panic::catch_unwind(|| unsafe { close_impl(obj) });
}

unsafe fn close_impl(obj: *mut vlc_object_t) {
    if obj.is_null() {
        return;
    }
    
    let demux = obj as *mut demux_t;
    let p_sys = (*demux).p_sys;
    
    if !p_sys.is_null() {
        // Recover and drop the Box
        let _sys = Box::from_raw(p_sys as *mut DemuxSys);
        (*demux).p_sys = ptr::null_mut();
    }
}

/// Demux function - called to read the next frame
#[no_mangle]
pub unsafe extern "C" fn Demux(demux: *mut demux_t) -> c_int {
    let result = panic::catch_unwind(|| unsafe { demux_impl(demux) });
    match result {
        Ok(code) => code,
        Err(_) => {
            eprintln!("VAI plugin: panic in Demux");
            VLC_DEMUXER_EOF
        }
    }
}

unsafe fn demux_impl(demux: *mut demux_t) -> c_int {
    if demux.is_null() {
        return VLC_DEMUXER_EOF;
    }
    
    let p_sys = (*demux).p_sys;
    if p_sys.is_null() {
        return VLC_DEMUXER_EOF;
    }
    
    let sys = &mut *(p_sys as *mut DemuxSys);
    
    // Calculate timestamp for current frame
    let timestamp_ms = ((sys.current_frame as f64 * 1000.0) / sys.fps) as u64;
    
    // Check if we've reached the end
    if timestamp_ms >= sys.duration_ms {
        return VLC_DEMUXER_EOF;
    }
    
    // Render the frame
    let frame: RgbaImage = match sys.compositor.render_frame(timestamp_ms) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("VAI plugin: failed to render frame: {}", e);
            return VLC_DEMUXER_EOF;
        }
    };
    
    // Allocate block for frame data
    let frame_size = (sys.width * sys.height * 4) as usize;
    let block = block_Alloc(frame_size);
    if block.is_null() {
        return VLC_DEMUXER_EOF;
    }
    
    // Copy frame data to block
    let frame_data = frame.as_raw();
    ptr::copy_nonoverlapping(
        frame_data.as_ptr(),
        (*block).p_buffer,
        frame_size,
    );
    
    // Set timestamps
    let pts = vlc_tick_from_ms(timestamp_ms);
    (*block).i_pts = pts;
    (*block).i_dts = pts;
    (*block).i_length = vlc_tick_from_ms((1000.0 / sys.fps) as u64);
    
    // Send to VLC
    let es_out = (*demux).out;
    es_out_Send(es_out, sys.es_id, block);
    
    // Update PCR (Program Clock Reference)
    es_out_Control(es_out, ES_OUT_SET_PCR, pts);
    
    // Advance to next frame
    sys.current_frame += 1;
    
    VLC_DEMUXER_SUCCESS
}

/// Control function - handle seeks and queries
/// 
/// Note: VLC passes variadic arguments as a pointer. We access them by casting
/// to the appropriate pointer type for each query.
#[no_mangle]
pub unsafe extern "C" fn Control(
    demux: *mut demux_t,
    query: c_int,
    args: *mut libc::c_void,  // Pointer to variadic arguments
) -> c_int {
    if demux.is_null() {
        return VLC_EGENERIC;
    }
    
    let p_sys = (*demux).p_sys;
    if p_sys.is_null() {
        return VLC_EGENERIC;
    }
    
    let sys = &mut *(p_sys as *mut DemuxSys);
    
    match query {
        DEMUX_GET_LENGTH => {
            // Return total duration in microseconds
            // va_list points to a pointer to the output variable
            let p_length = *(args as *const *mut vlc_tick_t);
            if !p_length.is_null() {
                *p_length = vlc_tick_from_ms(sys.duration_ms);
                return VLC_SUCCESS;
            }
        }
        
        DEMUX_GET_TIME => {
            // Return current time in microseconds
            let p_time = *(args as *const *mut vlc_tick_t);
            if !p_time.is_null() {
                let current_ms = ((sys.current_frame as f64 * 1000.0) / sys.fps) as u64;
                *p_time = vlc_tick_from_ms(current_ms);
                return VLC_SUCCESS;
            }
        }
        
        DEMUX_GET_POSITION => {
            // Return position as 0.0 to 1.0
            let p_position = *(args as *const *mut f64);
            if !p_position.is_null() {
                if sys.duration_ms > 0 {
                    let current_ms = ((sys.current_frame as f64 * 1000.0) / sys.fps) as u64;
                    *p_position = current_ms as f64 / sys.duration_ms as f64;
                } else {
                    *p_position = 0.0;
                }
                return VLC_SUCCESS;
            }
        }
        
        DEMUX_SET_POSITION => {
            // Seek to position (0.0 to 1.0)
            let position = *(args as *const f64);
            let target_frame = (position * sys.total_frames as f64) as u64;
            sys.current_frame = target_frame.min(sys.total_frames.saturating_sub(1));
            return VLC_SUCCESS;
        }
        
        DEMUX_SET_TIME => {
            // Seek to time in microseconds
            let time_us = *(args as *const vlc_tick_t);
            let time_ms = (time_us / 1000) as u64;
            let target_frame = ((time_ms as f64 * sys.fps) / 1000.0) as u64;
            sys.current_frame = target_frame.min(sys.total_frames.saturating_sub(1));
            return VLC_SUCCESS;
        }
        
        _ => {}
    }
    
    VLC_EGENERIC
}

// Module descriptor
//
// VLC 3.x uses a specific module descriptor format. We need to export
// vlc_entry__* symbols that VLC will look for.

// Note: The actual VLC plugin registration is complex and version-specific.
// For a production plugin, you would need to match your VLC version's exact
// module descriptor format. This simplified version demonstrates the concept.

// Export plugin metadata as weak symbols that VLC can discover
#[no_mangle]
#[used]
pub static vlc_module_name: &[u8] = b"vai\0";

#[no_mangle]
#[used]
pub static vlc_module_help: &[u8] = b"VAI sprite-sheet video demuxer\0";

/// VLC plugin capabilities - this tells VLC what file extensions we handle
#[no_mangle]
pub unsafe extern "C" fn vlc_entry_capability() -> *const libc::c_char {
    b"demux\0".as_ptr() as *const libc::c_char
}

/// VLC plugin priority
#[no_mangle]
pub unsafe extern "C" fn vlc_entry_priority() -> c_int {
    100 // Higher priority than default
}

// For a full VLC plugin, you would need additional module descriptor exports.
// The exact format depends on your VLC version. This provides the core
// functionality; consult VLC's plugin development docs for complete integration.
