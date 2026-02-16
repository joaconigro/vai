//! VAI VLC Plugin — Rust core logic
//!
//! Exposes a C-ABI interface (`vai_plugin_*`) that the C shim in
//! `vlc_shim.c` calls.  Rust never touches VLC structs directly.

use image::RgbaImage;
use std::io::Cursor;
use std::os::raw::c_int;
use std::panic;
use std::ptr;
use vai_core::VaiContainer;
use vai_decoder::FrameCompositor;

/// Info about the opened VAI file, shared with C via repr(C).
#[repr(C)]
pub struct VaiPluginInfo {
    pub width: u32,
    pub height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    pub duration_ms: u64,
    pub total_frames: u64,
    pub fps: f64,
}

/// Internal playback state.
struct PluginState {
    compositor: FrameCompositor,
    info: VaiPluginInfo,
    current_frame: u64,
}

// ──────────────────── C-ABI functions ────────────────────

/// Parse a VAI container from raw bytes.
/// On success returns an opaque handle and fills `out_info`.
/// On failure returns NULL.
#[no_mangle]
pub unsafe extern "C" fn vai_plugin_open(
    data: *const u8,
    len: usize,
    out_info: *mut VaiPluginInfo,
) -> *mut std::ffi::c_void {
    let result = panic::catch_unwind(|| {
        if data.is_null() || len == 0 || out_info.is_null() {
            return ptr::null_mut();
        }

        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        let container = match VaiContainer::read(Cursor::new(slice.to_vec())) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("VAI plugin: parse error: {e}");
                return ptr::null_mut();
            }
        };

        let fps = container.fps();
        let duration_ms = container.header.duration_ms;
        let total_frames = ((duration_ms as f64 * fps) / 1000.0).ceil() as u64;

        let info = VaiPluginInfo {
            width: container.header.width,
            height: container.header.height,
            fps_num: container.header.fps_num,
            fps_den: container.header.fps_den,
            duration_ms,
            total_frames,
            fps,
        };

        unsafe {
            ptr::write(out_info, VaiPluginInfo {
                width: info.width,
                height: info.height,
                fps_num: info.fps_num,
                fps_den: info.fps_den,
                duration_ms: info.duration_ms,
                total_frames: info.total_frames,
                fps: info.fps,
            });
        }

        let state = Box::new(PluginState {
            compositor: FrameCompositor::new(container),
            info,
            current_frame: 0,
        });

        Box::into_raw(state) as *mut std::ffi::c_void
    });

    result.unwrap_or(ptr::null_mut())
}

/// Render the frame at `timestamp_ms` into `out_buf` (RGBA, row-major).
/// Returns 0 on success, -1 on failure.
#[no_mangle]
pub unsafe extern "C" fn vai_plugin_render(
    handle: *mut std::ffi::c_void,
    timestamp_ms: u64,
    out_buf: *mut u8,
    buf_size: usize,
) -> c_int {
    let result = panic::catch_unwind(|| {
        if handle.is_null() || out_buf.is_null() {
            return -1;
        }
        let state = unsafe { &mut *(handle as *mut PluginState) };

        let frame: RgbaImage = match state.compositor.render_frame(timestamp_ms) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("VAI plugin: render error: {e}");
                return -1;
            }
        };

        let raw = frame.as_raw();
        let copy_len = raw.len().min(buf_size);
        unsafe {
            ptr::copy_nonoverlapping(raw.as_ptr(), out_buf, copy_len);
        }
        0
    });

    result.unwrap_or(-1)
}

/// Return the current frame number.
#[no_mangle]
pub unsafe extern "C" fn vai_plugin_current_frame(
    handle: *mut std::ffi::c_void,
) -> u64 {
    if handle.is_null() {
        return 0;
    }
    let state = unsafe { &*(handle as *const PluginState) };
    state.current_frame
}

/// Advance to the next frame.
#[no_mangle]
pub unsafe extern "C" fn vai_plugin_advance(handle: *mut std::ffi::c_void) {
    if handle.is_null() {
        return;
    }
    let state = unsafe { &mut *(handle as *mut PluginState) };
    state.current_frame += 1;
}

/// Seek to a specific frame number.
#[no_mangle]
pub unsafe extern "C" fn vai_plugin_seek_frame(
    handle: *mut std::ffi::c_void,
    frame: u64,
) {
    if handle.is_null() {
        return;
    }
    let state = unsafe { &mut *(handle as *mut PluginState) };
    state.current_frame = frame;
}

/// Free the plugin state.
#[no_mangle]
pub unsafe extern "C" fn vai_plugin_close(handle: *mut std::ffi::c_void) {
    if !handle.is_null() {
        let _ = unsafe { Box::from_raw(handle as *mut PluginState) };
    }
}
