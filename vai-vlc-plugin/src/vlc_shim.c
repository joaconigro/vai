/**
 * VLC plugin shim for VAI demuxer.
 *
 * This C file handles all VLC ABI interactions:
 *   - Module descriptor (vlc_entry__*)
 *   - Open / Close / Demux / Control callbacks
 *   - VLC stream, es_out, block, and es_format API calls
 *
 * The actual VAI logic lives in Rust. This shim communicates with
 * Rust through a small C‑ABI interface (vai_plugin_*).
 */

#include <stdint.h>
#include <string.h>
#include <stdlib.h>

/* ── VLC headers ── */
#define __PLUGIN__
#define MODULE_NAME    vai
#define MODULE_STRING  "vai"

#include <vlc/plugins/vlc_common.h>
#include <vlc/plugins/vlc_plugin.h>
#include <vlc/plugins/vlc_demux.h>
#include <vlc/plugins/vlc_es.h>
#include <vlc/plugins/vlc_es_out.h>
#include <vlc/plugins/vlc_block.h>
#include <vlc/plugins/vlc_stream.h>

/* ── Shared info struct (matches Rust repr(C)) ── */
typedef struct {
    uint32_t width;
    uint32_t height;
    uint32_t fps_num;
    uint32_t fps_den;
    uint64_t duration_ms;
    uint64_t total_frames;
    double   fps;
} vai_plugin_info_t;

/* ── Rust extern "C" functions implemented in lib.rs ── */
extern void *vai_plugin_open(const uint8_t *data, size_t len,
                             vai_plugin_info_t *out_info);
extern int   vai_plugin_render(void *handle, uint64_t timestamp_ms,
                               uint8_t *out_buf, size_t buf_size);
extern void  vai_plugin_seek_frame(void *handle, uint64_t frame);
extern uint64_t vai_plugin_current_frame(void *handle);
extern void  vai_plugin_advance(void *handle);
extern void  vai_plugin_close(void *handle);

/* ── Private demux data stored in demux_t.p_sys ── */
struct demux_sys_t {
    void           *rust_handle;   /* opaque ptr returned by vai_plugin_open */
    es_out_id_t    *es_id;
    vai_plugin_info_t info;
};

/* ── Forward declarations for callbacks ── */
static int Open(vlc_object_t *);
static void Close(vlc_object_t *);
static int Demux(demux_t *);
static int Control(demux_t *, int, va_list);

/* ═════════════════════════════════════════════════════════════════════
 *  Module descriptor — this is what VLC looks for when loading the .so
 * ═════════════════════════════════════════════════════════════════════ */
vlc_module_begin()
    set_shortname("VAI")
    set_description("VAI sprite-sheet video demuxer")
    set_category(CAT_INPUT)
    set_subcategory(SUBCAT_INPUT_DEMUX)
    set_capability("demux", 320)
    set_callbacks(Open, Close)
    add_shortcut("vai")
vlc_module_end()

/* ═════════════════════════════════════════════════════════════════════
 *  Open – probe the stream and initialise the demuxer
 * ═════════════════════════════════════════════════════════════════════ */
static int Open(vlc_object_t *obj)
{
    demux_t *demux = (demux_t *)obj;

    /* Probe: first 4 bytes must be "VAI\0" */
    const uint8_t *peek;
    if (vlc_stream_Peek(demux->s, &peek, 4) < 4)
        return VLC_EGENERIC;
    if (memcmp(peek, "VAI\0", 4) != 0)
        return VLC_EGENERIC;

    /* Read the entire file into a temporary buffer */
    uint64_t file_size = 0;
    if (vlc_stream_GetSize(demux->s, &file_size) || file_size == 0
        || file_size > (uint64_t)1024 * 1024 * 1024)
        return VLC_EGENERIC;

    uint8_t *buf = malloc((size_t)file_size);
    if (!buf)
        return VLC_ENOMEM;

    ssize_t n = vlc_stream_Read(demux->s, buf, (size_t)file_size);
    if (n < 0 || (size_t)n != (size_t)file_size) {
        free(buf);
        return VLC_EGENERIC;
    }

    /* Hand the bytes to Rust for parsing */
    vai_plugin_info_t info;
    memset(&info, 0, sizeof(info));

    void *rust_handle = vai_plugin_open(buf, (size_t)file_size, &info);
    free(buf);   /* Rust made its own copy */

    if (!rust_handle)
        return VLC_EGENERIC;

    /* Set up an RGBA video elementary stream */
    es_format_t fmt;
    es_format_Init(&fmt, VIDEO_ES, VLC_CODEC_RGBA);
    fmt.video.i_width          = info.width;
    fmt.video.i_height         = info.height;
    fmt.video.i_visible_width  = info.width;
    fmt.video.i_visible_height = info.height;
    fmt.video.i_sar_num        = 1;
    fmt.video.i_sar_den        = 1;
    fmt.video.i_frame_rate      = info.fps_num;
    fmt.video.i_frame_rate_base = info.fps_den;

    es_out_id_t *es_id = es_out_Add(demux->out, &fmt);
    es_format_Clean(&fmt);

    if (!es_id) {
        vai_plugin_close(rust_handle);
        return VLC_EGENERIC;
    }

    /* Allocate and populate p_sys */
    demux_sys_t *sys = calloc(1, sizeof(*sys));
    if (!sys) {
        vai_plugin_close(rust_handle);
        return VLC_ENOMEM;
    }
    sys->rust_handle = rust_handle;
    sys->es_id       = es_id;
    sys->info        = info;

    demux->p_sys     = sys;
    demux->pf_demux  = Demux;
    demux->pf_control = Control;

    msg_Info(demux, "VAI: opened %ux%u @ %u/%u fps, %"PRIu64" ms, %"PRIu64" frames",
             info.width, info.height, info.fps_num, info.fps_den,
             info.duration_ms, info.total_frames);

    return VLC_SUCCESS;
}

/* ═════════════════════════════════════════════════════════════════════
 *  Close – release resources
 * ═════════════════════════════════════════════════════════════════════ */
static void Close(vlc_object_t *obj)
{
    demux_t    *demux = (demux_t *)obj;
    demux_sys_t *sys  = demux->p_sys;

    if (sys) {
        if (sys->rust_handle)
            vai_plugin_close(sys->rust_handle);
        free(sys);
    }
    demux->p_sys = NULL;
}

/* ═════════════════════════════════════════════════════════════════════
 *  Demux – deliver the next frame to VLC
 * ═════════════════════════════════════════════════════════════════════ */
static int Demux(demux_t *demux)
{
    demux_sys_t *sys = demux->p_sys;

    uint64_t cur = vai_plugin_current_frame(sys->rust_handle);
    uint64_t timestamp_ms = (uint64_t)((double)cur * 1000.0 / sys->info.fps);

    if (timestamp_ms >= sys->info.duration_ms)
        return VLC_DEMUXER_EOF;

    size_t frame_size = (size_t)sys->info.width * sys->info.height * 4;
    block_t *blk = block_Alloc(frame_size);
    if (!blk)
        return VLC_DEMUXER_EGENERIC;

    int ok = vai_plugin_render(sys->rust_handle, timestamp_ms,
                               blk->p_buffer, frame_size);
    if (ok != 0) {
        block_Release(blk);
        return VLC_DEMUXER_EOF;
    }

    mtime_t pts = (mtime_t)timestamp_ms * 1000;   /* ms → µs */
    blk->i_pts    = pts;
    blk->i_dts    = pts;
    blk->i_length = (mtime_t)(1000000.0 / sys->info.fps);

    es_out_Send(demux->out, sys->es_id, blk);
    es_out_Control(demux->out, ES_OUT_SET_PCR, pts);

    vai_plugin_advance(sys->rust_handle);

    return VLC_DEMUXER_SUCCESS;
}

/* ═════════════════════════════════════════════════════════════════════
 *  Control – handle VLC queries (seek, position, duration …)
 * ═════════════════════════════════════════════════════════════════════ */
static int Control(demux_t *demux, int query, va_list args)
{
    demux_sys_t *sys = demux->p_sys;

    switch (query) {
    case DEMUX_CAN_SEEK: {
        bool *pb = va_arg(args, bool *);
        *pb = true;
        return VLC_SUCCESS;
    }
    case DEMUX_GET_POSITION: {
        double *pd = va_arg(args, double *);
        if (sys->info.duration_ms > 0) {
            uint64_t cur = vai_plugin_current_frame(sys->rust_handle);
            uint64_t ts  = (uint64_t)((double)cur * 1000.0 / sys->info.fps);
            *pd = (double)ts / (double)sys->info.duration_ms;
        } else {
            *pd = 0.0;
        }
        return VLC_SUCCESS;
    }
    case DEMUX_SET_POSITION: {
        double pos = va_arg(args, double);
        uint64_t frame = (uint64_t)(pos * (double)sys->info.total_frames);
        if (frame >= sys->info.total_frames && sys->info.total_frames > 0)
            frame = sys->info.total_frames - 1;
        vai_plugin_seek_frame(sys->rust_handle, frame);
        return VLC_SUCCESS;
    }
    case DEMUX_GET_LENGTH: {
        int64_t *pi = va_arg(args, int64_t *);
        *pi = (int64_t)sys->info.duration_ms * 1000;  /* µs */
        return VLC_SUCCESS;
    }
    case DEMUX_GET_TIME: {
        int64_t *pi = va_arg(args, int64_t *);
        uint64_t cur = vai_plugin_current_frame(sys->rust_handle);
        *pi = (int64_t)((double)cur * 1000000.0 / sys->info.fps);
        return VLC_SUCCESS;
    }
    case DEMUX_SET_TIME: {
        int64_t us = va_arg(args, int64_t);
        uint64_t ms = (uint64_t)(us / 1000);
        uint64_t frame = (uint64_t)((double)ms * sys->info.fps / 1000.0);
        if (frame >= sys->info.total_frames && sys->info.total_frames > 0)
            frame = sys->info.total_frames - 1;
        vai_plugin_seek_frame(sys->rust_handle, frame);
        return VLC_SUCCESS;
    }
    default:
        return VLC_EGENERIC;
    }
}
