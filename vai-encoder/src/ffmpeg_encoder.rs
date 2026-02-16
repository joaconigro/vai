//! FFmpeg-based AV1/AVIF encoding
//!
//! Uses FFmpeg's AV1 encoders (libsvtav1, libaom-av1, etc.) to encode RGBA
//! images into AVIF.  This is significantly faster than the pure-Rust `ravif`
//! encoder, especially with `libsvtav1`.
//!
//! The flow:
//!   1. RGBA pixels → ffmpeg `frame::Video` (RGBA)
//!   2. swscale RGBA → YUV420P (required by most AV1 encoders)
//!   3. AV1 encoder → raw OBU bitstream
//!   4. Wrap the OBU in a minimal AVIF (ISOBMFF) container

use crate::{Error, Result};
use image::RgbaImage;

// ── Encoder preference list ──
// Tried in order; first one that FFmpeg can find wins.
const ENCODER_NAMES: &[&str] = &["libsvtav1", "libaom-av1", "librav1e"];

/// Encode an RGBA image to AVIF bytes using FFmpeg's AV1 encoders.
///
/// `quality` is 0–100 (like ravif).  Internally mapped to CRF for the chosen
/// encoder: CRF 0 = lossless, CRF 63 = worst quality.
///
/// Returns `Err` if no AV1 encoder is found, if the image is too small for the
/// encoder (SVT-AV1 requires ≥ 64×64), or on encoding failure.
pub fn encode_avif_ffmpeg(image: &RgbaImage, quality: u8) -> Result<Vec<u8>> {
    let width = image.width();
    let height = image.height();

    // SVT-AV1 (and some other encoders) require minimum 64×64.
    // Return an error so the caller can fall back to ravif.
    if width < 64 || height < 64 {
        return Err(Error::AvifEncode(format!(
            "Image too small for FFmpeg AV1 encoder ({width}×{height}, min 64×64)"
        )));
    }

    // Map quality 0..100 → CRF 63..0  (higher quality = lower CRF)
    let crf = ((100u16.saturating_sub(quality as u16)) as f64 * 63.0 / 100.0).round() as i32;

    // ── 1. Find an AV1 encoder ──
    let codec = ENCODER_NAMES
        .iter()
        .find_map(|name| ffmpeg_next::encoder::find_by_name(name))
        .ok_or_else(|| {
            Error::AvifEncode(
                "No AV1 encoder found (tried libsvtav1, libaom-av1, librav1e)".into(),
            )
        })?;

    let encoder_name = unsafe {
        std::ffi::CStr::from_ptr((*codec.as_ptr()).name)
            .to_str()
            .unwrap_or("unknown")
            .to_string()
    };

    // ── 2. Configure the encoder ──
    // YUV420P requires even dimensions; round up if needed.
    let enc_width = (width + 1) & !1;
    let enc_height = (height + 1) & !1;

    let context = ffmpeg_next::codec::context::Context::from_parameters(
        ffmpeg_next::codec::Parameters::new(),
    )?;
    let mut video = context.encoder().video()?;

    video.set_width(enc_width);
    video.set_height(enc_height);
    video.set_format(ffmpeg_next::format::Pixel::YUV420P);
    video.set_time_base(ffmpeg_next::Rational(1, 25));
    // Single still image — one intra frame, no B-frames
    video.set_gop(0);
    video.set_max_b_frames(0);

    // Encoder-specific options via the private options dict
    let mut opts = ffmpeg_next::Dictionary::new();
    opts.set("crf", &crf.to_string());

    // SVT-AV1 specific: use a fast preset for stills
    if encoder_name == "libsvtav1" {
        // preset 6 is a good speed/quality trade-off for stills
        opts.set("preset", "6");
        // Limit SVT-AV1 to 1 thread per instance — our outer parallel loop
        // already saturates all cores, so each SVT-AV1 instance only needs 1.
        opts.set("svtav1-params", "lp=1");
    } else if encoder_name == "libaom-av1" {
        // cpu-used 6 is much faster than default (1)
        opts.set("cpu-used", "6");
        opts.set("usage", "allintra");
        opts.set("row-mt", "1");
    }

    let mut encoder = video.open_as_with(codec, opts).map_err(|e| {
        Error::AvifEncode(format!(
            "FFmpeg encoder open failed for {encoder_name} ({enc_width}×{enc_height}, crf={crf}): {e}"
        ))
    })?;

    // ── 3. Create RGBA frame and convert to YUV420P ──
    // If we rounded up, create frames at the padded size and copy the source
    // pixels into the top-left corner (the extra column/row is black/transparent).
    let mut rgba_frame =
        ffmpeg_next::util::frame::video::Video::new(ffmpeg_next::format::Pixel::RGBA, enc_width, enc_height);
    rgba_frame.set_pts(Some(0));

    // Copy RGBA pixels into the frame (respecting stride + possible padding)
    {
        let stride = rgba_frame.stride(0);
        let dst = rgba_frame.data_mut(0);
        let src = image.as_raw();
        let row_bytes = (width as usize) * 4;
        for y in 0..height as usize {
            let src_off = y * row_bytes;
            let dst_off = y * stride;
            dst[dst_off..dst_off + row_bytes].copy_from_slice(&src[src_off..src_off + row_bytes]);
        }
    }

    // swscale: RGBA → YUV420P
    let mut scaler = ffmpeg_next::software::scaling::Context::get(
        ffmpeg_next::format::Pixel::RGBA,
        enc_width,
        enc_height,
        ffmpeg_next::format::Pixel::YUV420P,
        enc_width,
        enc_height,
        ffmpeg_next::software::scaling::Flags::BILINEAR,
    )?;

    let mut yuv_frame = ffmpeg_next::util::frame::video::Video::empty();
    scaler.run(&rgba_frame, &mut yuv_frame)?;
    yuv_frame.set_pts(Some(0));

    // ── 4. Encode ──
    let mut av1_data = Vec::new();

    encoder.send_frame(&yuv_frame)?;
    encoder.send_eof()?;

    let mut packet = ffmpeg_next::Packet::empty();
    while encoder.receive_packet(&mut packet).is_ok() {
        av1_data.extend_from_slice(packet.data().unwrap_or(&[]));
    }

    if av1_data.is_empty() {
        return Err(Error::AvifEncode("AV1 encoder produced no output".into()));
    }

    // ── 5. Wrap raw AV1 OBUs in a minimal AVIF (ISOBMFF) container ──
    let avif = wrap_av1_in_avif(&av1_data, width, height);

    Ok(avif)
}

// ─────────────────────────────────────────────────────────────────────────────
//  Minimal AVIF (ISOBMFF) container writer
//
//  AVIF is a profile of HEIF (ISO 23008-12) using AV1 (ISO/IEC 23091).
//  For a single still image the structure is:
//
//    ftyp  (file type)
//    meta  (metadata)
//      hdlr  (handler — "pict")
//      pitm  (primary item)
//      iloc  (item location — points into the mdat)
//      iinf  (item info)
//        infe  (item info entry — "av01")
//      iprp  (item properties)
//        ipco  (item property container)
//          ispe  (image spatial extents — width × height)
//          av1C  (AV1 codec configuration)
//        ipma  (item property association)
//    mdat  (media data — the raw AV1 OBUs)
// ─────────────────────────────────────────────────────────────────────────────

fn wrap_av1_in_avif(av1_data: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(av1_data.len() + 256);

    // ── ftyp ──
    let ftyp = build_ftyp();
    out.extend_from_slice(&ftyp);

    // ── meta (fullbox, version 0, flags 0) ──
    let meta_inner = build_meta_inner(av1_data, width, height);
    write_fullbox(&mut out, b"meta", 0, 0, &meta_inner);

    // ── mdat ──
    write_box(&mut out, b"mdat", av1_data);

    out
}

fn build_ftyp() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(b"avif"); // major brand
    body.extend_from_slice(&0u32.to_be_bytes()); // minor version
    body.extend_from_slice(b"avif"); // compatible brand
    body.extend_from_slice(b"mif1"); // compatible brand

    let mut out = Vec::new();
    write_box(&mut out, b"ftyp", &body);
    out
}

fn build_meta_inner(av1_data: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut inner = Vec::new();

    // hdlr
    {
        let mut body = Vec::new();
        body.extend_from_slice(&0u32.to_be_bytes()); // pre_defined
        body.extend_from_slice(b"pict"); // handler_type
        body.extend_from_slice(&[0u8; 12]); // reserved
        body.push(0); // name (null-terminated empty string)
        write_fullbox(&mut inner, b"hdlr", 0, 0, &body);
    }

    // pitm (primary item ID = 1)
    {
        let body = 1u16.to_be_bytes();
        write_fullbox(&mut inner, b"pitm", 0, 0, &body);
    }

    // iinf (item info box, 1 entry)
    {
        let mut infe_body = Vec::new();
        infe_body.extend_from_slice(&1u16.to_be_bytes()); // item_ID
        infe_body.extend_from_slice(&0u16.to_be_bytes()); // item_protection_index
        infe_body.extend_from_slice(b"av01"); // item_type
        infe_body.push(0); // item_name (null-terminated empty)

        let mut infe = Vec::new();
        write_fullbox(&mut infe, b"infe", 2, 0, &infe_body);

        let mut iinf_body = Vec::new();
        iinf_body.extend_from_slice(&1u16.to_be_bytes()); // entry_count
        iinf_body.extend_from_slice(&infe);

        write_fullbox(&mut inner, b"iinf", 0, 0, &iinf_body);
    }

    // iprp
    {
        let mut iprp_inner = Vec::new();

        // ipco (property container)
        {
            let mut ipco_inner = Vec::new();

            // ispe (image spatial extents) — property index 1
            {
                let mut ispe_body = Vec::new();
                ispe_body.extend_from_slice(&width.to_be_bytes());
                ispe_body.extend_from_slice(&height.to_be_bytes());
                write_fullbox(&mut ipco_inner, b"ispe", 0, 0, &ispe_body);
            }

            // av1C (AV1 codec configuration) — property index 2
            {
                let av1c_body = build_av1c(av1_data);
                write_box(&mut ipco_inner, b"av1C", &av1c_body);
            }

            // pixi (pixel information) — property index 3
            // Required by libavif for av01 items.  YUV420P 8-bit → 3 planes, 8 bpp each.
            {
                let mut pixi_body = Vec::new();
                pixi_body.push(3); // num_channels
                pixi_body.push(8); // bits_per_channel[0]
                pixi_body.push(8); // bits_per_channel[1]
                pixi_body.push(8); // bits_per_channel[2]
                write_fullbox(&mut ipco_inner, b"pixi", 0, 0, &pixi_body);
            }

            write_box(&mut iprp_inner, b"ipco", &ipco_inner);
        }

        // ipma (item property association)
        {
            let mut ipma_body = Vec::new();
            ipma_body.extend_from_slice(&1u32.to_be_bytes()); // entry_count
            ipma_body.extend_from_slice(&1u16.to_be_bytes()); // item_ID
            ipma_body.push(3); // association_count
            // association 1: essential=true, property_index=1 (ispe)
            ipma_body.push(0x80 | 1);
            // association 2: essential=true, property_index=2 (av1C)
            ipma_body.push(0x80 | 2);
            // association 3: essential=false, property_index=3 (pixi)
            ipma_body.push(3);

            write_fullbox(&mut iprp_inner, b"ipma", 0, 0, &ipma_body);
        }

        write_box(&mut inner, b"iprp", &iprp_inner);
    }

    // iloc (item location)
    // We need to know the offset of the mdat payload.
    // The mdat box = 8 (box header) + av1_data.len().
    // The mdat payload starts at offset = ftyp.len() + meta_box.len() + 8.
    // We can't know meta_box.len() yet (it includes iloc which we're building
    // now), so we use construction_method=1 (idat_offset) — but simpler: use
    // iloc with offset_size=4, base_offset relative to file start with a
    // placeholder, then patch after.
    //
    // Actually, the standard approach is to use construction_method=0 and
    // just point to the mdat data.  We'll compute the total meta size after
    // building iloc, then patch the base_offset.
    //
    // For simplicity, use offset_size=4, length_size=4, base_offset_size=0,
    // and set extent_offset = 0 (we'll patch it later).
    {
        // Version 0, offset_size=4, length_size=4, base_offset_size=0
        let mut iloc_body = Vec::new();
        iloc_body.push(0x44); // (offset_size << 4) | (length_size)
        iloc_body.push(0x00); // (base_offset_size << 4) | 0  (index_size n/a for v0)
        iloc_body.extend_from_slice(&1u16.to_be_bytes()); // item_count
        // item_ID=1
        iloc_body.extend_from_slice(&1u16.to_be_bytes());
        iloc_body.extend_from_slice(&0u16.to_be_bytes()); // data_reference_index
        iloc_body.extend_from_slice(&1u16.to_be_bytes()); // extent_count
        // extent_offset: placeholder (4 bytes), will be patched
        let extent_offset_pos = iloc_body.len();
        iloc_body.extend_from_slice(&0u32.to_be_bytes()); // extent_offset (placeholder)
        iloc_body.extend_from_slice(&(av1_data.len() as u32).to_be_bytes()); // extent_length

        write_fullbox(&mut inner, b"iloc", 0, 0, &iloc_body);

        // Now patch the extent_offset.
        // The iloc fullbox in `inner` was just written.  We need to find the
        // placeholder and patch it once we know the total file offset.
        //
        // Total file layout:
        //   ftyp_len + meta_box_len + 8 (mdat header)  →  mdat payload offset
        //
        // ftyp_len is fixed (24 bytes):  8 (header) + 4 (brand) + 4 (version) + 4 + 4 (compat)
        let ftyp_len = 24u32;
        // meta_box_len = 12 (fullbox header) + inner.len()
        let meta_box_len = (12 + inner.len()) as u32;
        // mdat header = 8
        let mdat_payload_offset = ftyp_len + meta_box_len + 8;

        // Find the placeholder in `inner`.  The iloc fullbox we just appended
        // has a known structure.  The placeholder is at:
        //   inner.len() - iloc_box_total_size + 12 (fullbox header of iloc)
        //   + extent_offset_pos
        let iloc_box_body_len = iloc_body.len();
        let iloc_fullbox_total = 12 + iloc_box_body_len;
        let iloc_start_in_inner = inner.len() - iloc_fullbox_total;
        let patch_pos = iloc_start_in_inner + 12 + extent_offset_pos;
        inner[patch_pos..patch_pos + 4].copy_from_slice(&mdat_payload_offset.to_be_bytes());
    }

    inner
}

/// Build an AV1CodecConfigurationRecord from the raw OBU bitstream.
/// See AV1-ISOBMFF §2.3.  Minimal version for still images.
fn build_av1c(av1_data: &[u8]) -> Vec<u8> {
    // Parse the first Sequence Header OBU to extract profile, level, etc.
    // The OBU header is:  forbidden(1) | type(4) | extension_flag(1) | has_size(1) | reserved(1)
    //
    // For a minimal valid av1C we need:
    //   marker(1)=1 | version(7)=1
    //   seq_profile(3) | seq_level_idx_0(5)
    //   seq_tier_0(1) | high_bitdepth(1) | twelve_bit(1) | monochrome(1) |
    //     chroma_subsampling_x(1) | chroma_subsampling_y(1) | chroma_sample_position(2)
    //   reserved(3)=0 | initial_presentation_delay_present(1)=0 | reserved(4)=0

    // Try to parse a Sequence Header OBU for accurate values
    let (profile, level) = parse_seq_header_simple(av1_data).unwrap_or((0, 0));

    let mut av1c = Vec::with_capacity(4);
    av1c.push(0x81); // marker=1, version=1
    av1c.push((profile << 5) | (level & 0x1F)); // seq_profile(3) | seq_level_idx_0(5)
    // tier=0, high_bitdepth=0, twelve_bit=0, monochrome=0,
    // chroma_subsampling_x=1, chroma_subsampling_y=1, chroma_sample_position=0 (unknown)
    av1c.push(0b0000_0110);
    av1c.push(0x00); // no initial_presentation_delay

    av1c
}

/// Very simple parser to extract seq_profile and seq_level_idx_0 from the first
/// Sequence Header OBU in a raw AV1 bitstream.
fn parse_seq_header_simple(data: &[u8]) -> Option<(u8, u8)> {
    if data.is_empty() {
        return None;
    }

    let mut i = 0;
    while i < data.len() {
        let header_byte = data[i];
        let obu_type = (header_byte >> 3) & 0x0F;
        let has_extension = (header_byte >> 2) & 1;
        let has_size = (header_byte >> 1) & 1;
        i += 1;

        if has_extension == 1 && i < data.len() {
            i += 1; // skip extension byte
        }

        let obu_size = if has_size == 1 {
            // leb128
            let (size, consumed) = read_leb128(&data[i..])?;
            i += consumed;
            size
        } else {
            // rest of data
            data.len() - i
        };

        if obu_type == 1 {
            // OBU_SEQUENCE_HEADER
            if i + 1 < data.len() {
                let first_byte = data[i];
                let seq_profile = (first_byte >> 5) & 0x07;
                // still_picture flag is bit 4
                // reduced_still_picture_header is bit 3
                let reduced = (first_byte >> 3) & 1;
                if reduced == 1 {
                    // seq_level_idx_0 is the next 5 bits
                    let second_part = ((first_byte & 0x07) << 2) | (data.get(i + 1)? >> 6);
                    return Some((seq_profile, second_part));
                } else {
                    // More complex header; just use defaults
                    return Some((seq_profile, 0));
                }
            }
            return None;
        }

        i += obu_size;
    }

    None
}

fn read_leb128(data: &[u8]) -> Option<(usize, usize)> {
    let mut value: u64 = 0;
    let mut consumed = 0;
    for &byte in data.iter().take(8) {
        value |= ((byte & 0x7F) as u64) << (consumed * 7);
        consumed += 1;
        if byte & 0x80 == 0 {
            return Some((value as usize, consumed));
        }
    }
    None
}

// ── ISOBMFF box helpers ──

fn write_box(out: &mut Vec<u8>, box_type: &[u8; 4], body: &[u8]) {
    let size = (8 + body.len()) as u32;
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(box_type);
    out.extend_from_slice(body);
}

fn write_fullbox(out: &mut Vec<u8>, box_type: &[u8; 4], version: u8, flags: u32, body: &[u8]) {
    let size = (12 + body.len()) as u32;
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(box_type);
    // version (1 byte) + flags (3 bytes)
    let vf = ((version as u32) << 24) | (flags & 0x00FFFFFF);
    out.extend_from_slice(&vf.to_be_bytes());
    out.extend_from_slice(body);
}

/// Probe whether any FFmpeg AV1 encoder is available at runtime.
pub fn is_available() -> bool {
    ffmpeg_next::init().is_ok()
        && ENCODER_NAMES
            .iter()
            .any(|name| ffmpeg_next::encoder::find_by_name(name).is_some())
}

/// Return the name of the best available AV1 encoder, or None.
pub fn best_encoder_name() -> Option<&'static str> {
    let _ = ffmpeg_next::init();
    ENCODER_NAMES
        .iter()
        .copied()
        .find(|name| ffmpeg_next::encoder::find_by_name(name).is_some())
}
