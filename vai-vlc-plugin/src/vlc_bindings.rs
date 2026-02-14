//! VLC Plugin C API Bindings
//!
//! Manual bindings for VLC's plugin API since there's no Rust crate available.

#![allow(non_camel_case_types)]
#![allow(dead_code)]

use std::os::raw::{c_char, c_int, c_longlong, c_uchar, c_uint, c_void};

// Opaque types
pub enum vlc_object_t {}
pub enum es_out_t {}
pub enum es_out_id_t {}
pub enum stream_t {}

// VLC return codes
pub const VLC_SUCCESS: c_int = 0;
pub const VLC_EGENERIC: c_int = -1;
pub const VLC_DEMUXER_SUCCESS: c_int = 1;
pub const VLC_DEMUXER_EOF: c_int = 0;

// Demux control queries
pub const DEMUX_GET_LENGTH: c_int = 0x1001;
pub const DEMUX_GET_TIME: c_int = 0x1002;
pub const DEMUX_GET_POSITION: c_int = 0x1003;
pub const DEMUX_SET_POSITION: c_int = 0x1004;
pub const DEMUX_SET_TIME: c_int = 0x1005;

// ES out control queries
pub const ES_OUT_SET_PCR: c_int = 0x100;

// VLC codec fourcc
pub const VLC_CODEC_RGBA: u32 = 0x41424752; // "RGBA" in little-endian

// VLC tick type (microseconds)
pub type vlc_tick_t = c_longlong;

// Convert milliseconds to VLC ticks (microseconds)
#[inline]
pub fn vlc_tick_from_ms(ms: u64) -> vlc_tick_t {
    (ms * 1000) as vlc_tick_t
}

// Elementary stream format
#[repr(C)]
pub struct es_format_t {
    pub i_cat: c_int,
    pub i_codec: u32,
    pub i_original_fourcc: u32,
    pub i_id: c_int,
    pub i_group: c_int,
    pub i_priority: c_int,
    pub psz_language: *mut c_char,
    pub psz_description: *mut c_char,
    
    // Video specific
    pub video: video_format_t,
    
    // Audio specific (we don't use this)
    _audio: [u8; 128],
    
    // Sub specific (we don't use this)
    _subs: [u8; 128],
    
    // Extra data
    pub i_extra: c_int,
    pub p_extra: *mut c_void,
}

#[repr(C)]
pub struct video_format_t {
    pub i_width: c_uint,
    pub i_height: c_uint,
    pub i_visible_width: c_uint,
    pub i_visible_height: c_uint,
    pub i_x_offset: c_uint,
    pub i_y_offset: c_uint,
    pub i_bits_per_pixel: c_uint,
    pub i_sar_num: c_uint,
    pub i_sar_den: c_uint,
    pub i_frame_rate: c_uint,
    pub i_frame_rate_base: c_uint,
    // ... many other fields we don't need
    _padding: [u8; 256],
}

// Block structure for sending data
#[repr(C)]
pub struct block_t {
    pub p_next: *mut block_t,
    pub p_buffer: *mut c_uchar,
    pub i_buffer: usize,
    pub i_size: usize,
    pub i_flags: c_uint,
    pub i_nb_samples: c_uint,
    pub i_pts: vlc_tick_t,
    pub i_dts: vlc_tick_t,
    pub i_length: vlc_tick_t,
    // ... other fields we don't need
    _padding: [u8; 128],
}

// Demuxer structure
#[repr(C)]
pub struct demux_t {
    pub p_module: *mut c_void,
    pub psz_access: *mut c_char,
    pub psz_demux: *mut c_char,
    pub psz_location: *mut c_char,
    pub psz_file: *mut c_char,
    
    pub s: *mut stream_t,
    pub out: *mut es_out_t,
    
    pub pf_demux: Option<unsafe extern "C" fn(*mut demux_t) -> c_int>,
    pub pf_control: Option<unsafe extern "C" fn(*mut demux_t, c_int, *mut c_void) -> c_int>,
    
    pub p_sys: *mut c_void,
    
    // ... other fields
    _padding: [u8; 256],
}

// Module descriptor structures (simplified)
#[repr(C)]
pub struct vlc_module_t {
    _opaque: [u8; 0],
}

// Format categories
pub const VIDEO_ES: c_int = 1;
pub const AUDIO_ES: c_int = 2;

extern "C" {
    // Stream operations
    pub fn stream_Read(s: *mut stream_t, buf: *mut c_void, len: usize) -> isize;
    pub fn stream_Tell(s: *mut stream_t) -> u64;
    pub fn stream_Seek(s: *mut stream_t, offset: u64) -> c_int;
    pub fn stream_GetSize(s: *mut stream_t) -> u64;
    
    // ES operations
    pub fn es_out_Add(out: *mut es_out_t, fmt: *const es_format_t) -> *mut es_out_id_t;
    pub fn es_out_Send(out: *mut es_out_t, id: *mut es_out_id_t, block: *mut block_t) -> c_int;
    pub fn es_out_Control(out: *mut es_out_t, query: c_int, ...) -> c_int;
    
    // ES format operations
    pub fn es_format_Init(fmt: *mut es_format_t, cat: c_int, codec: u32);
    pub fn es_format_Clean(fmt: *mut es_format_t);
    
    // Block operations
    pub fn block_Alloc(size: usize) -> *mut block_t;
    pub fn block_Release(block: *mut block_t);
}
