//! Full-frame source paths: PNG, WebP, and AVIF decode + the
//! shared orientation-aware resize they feed. (JPEG streams; it
//! lives in `jpeg`.)

use super::*;

/// A forward-only `Seek` over a `BufRead`, so the png crate can decode
/// straight from the caller's reader instead of a whole-source copy.
///
/// png 0.18 bounds `Decoder<R>` on `BufRead + Seek`, yet its decoder
/// never seeks (only its own tests do): the bound is what forced the
/// old `read_to_end` + `Cursor`, which kept the whole compressed PNG
/// resident on top of the decoded frame — 54 MB for a 7000^2 photo,
/// 246 MB for an unfiltered one (issue #129). This adapter answers
/// position queries from a byte counter and refuses any seek that
/// would move, as an `io::Error` the decoder surfaces as a decode
/// failure. `png_never_seeks_through_forward_only` pins the
/// assumption across every PNG shape, so a png upgrade that starts
/// seeking fails a test instead of silently mis-decoding.
pub(super) struct ForwardOnly<R> {
    inner: R,
    pos: u64,
}

impl<R> ForwardOnly<R> {
    pub(super) fn new(inner: R) -> Self {
        ForwardOnly { inner, pos: 0 }
    }
}

impl<R: std::io::Read> std::io::Read for ForwardOnly<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl<R: std::io::BufRead> std::io::BufRead for ForwardOnly<R> {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        self.inner.fill_buf()
    }

    fn consume(&mut self, amt: usize) {
        self.inner.consume(amt);
        self.pos += amt as u64;
    }
}

impl<R> std::io::Seek for ForwardOnly<R> {
    fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
        // Only a "seek" that lands where the stream already is can be
        // honored; the end is unknown and the past is gone.
        let stays = match to {
            std::io::SeekFrom::Start(n) => n == self.pos,
            std::io::SeekFrom::Current(d) => d == 0,
            std::io::SeekFrom::End(_) => false,
        };
        if stays {
            Ok(self.pos)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                format!(
                    "PNG source is forward-only: refused {to:?} at byte {}",
                    self.pos
                ),
            ))
        }
    }

    fn stream_position(&mut self) -> std::io::Result<u64> {
        Ok(self.pos)
    }
}

pub(super) fn process_png<R: std::io::BufRead>(
    s: &mut Scratch,
    reader: R,
    target: ImageFormat,
    p: &Resolved,
) -> Result<Vec<u8>> {
    let timing = crate::config::config().timing;
    let t0 = crate::clock::Instant::now();
    // Decode straight from the reader: the compressed source is never
    // copied, so only the caller's own buffer (if any) stays resident.
    let mut decoder = png::Decoder::new(ForwardOnly::new(reader));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut png_reader = decoder.read_info().context("parse PNG")?;
    let (src_w, src_h) = {
        let hdr = png_reader.info();
        (hdr.width as usize, hdr.height as usize)
    };
    check_src_pixels(src_w, src_h)?;
    // eXIf orientation (raw TIFF per the PNG spec; a stray JPEG-style
    // prefix is tolerated like browsers do). Only chunks ahead of the
    // image data are seen — the orientation must steer the resize box,
    // so it has to be known before decoding. The registration allows
    // post-IDAT placement, but Chrome and Firefox also honor only
    // pre-IDAT eXIf, so serving those unrotated is browser parity, and
    // fail-safe besides.
    let orientation = if p.auto_rotate {
        png_reader
            .info()
            .exif_metadata
            .as_ref()
            .and_then(|d| crate::meta::Orientation::from_exif_payload(d))
            .unwrap_or(crate::meta::Orientation::UPRIGHT)
    } else {
        crate::meta::Orientation::UPRIGHT
    };
    // iCCP profile (the png crate has already inflated it), bytes
    // passed through untouched.
    let icc: Option<Vec<u8>> = if p.icc_passthrough && target_supports_icc(target) {
        png_reader
            .info()
            .icc_profile
            .as_ref()
            .filter(|c| !c.is_empty() && c.len() <= ICC_CAP)
            .map(|c| c.to_vec())
    } else {
        None
    };

    // Hot path: plain RGB8, non-interlaced, linear-light mode. Rows are
    // sRGB->linear LUT-mapped as they are decoded, so the pixels never
    // take a second full-image pass (and never exist as a full u8 copy).
    // Everything that picks it is in the header, so it is decided
    // before the estimate, which then prices the arm that will run.
    let (ct, bits) = png_reader.output_color_type();
    let fused = {
        let (dst_w, dst_h) = fit_dims(src_w, src_h, p.max_width, p.max_height);
        ct == png::ColorType::Rgb
            && bits == png::BitDepth::Eight
            && !png_reader.info().interlaced
            && (src_w, src_h) != (dst_w, dst_h)
            && p.linear_light
            // Oriented PNGs are rare; they take the general arm below
            // rather than teaching the row-streaming path to rotate.
            && orientation.is_upright()
    };
    {
        // EXPAND|STRIP_16 normalize to 8 bits, and resolve palettes to
        // RGB or RGBA by tRNS; grayscale is widened to RGB(A) in place.
        let channels = match ct {
            png::ColorType::Rgba | png::ColorType::GrayscaleAlpha => 4,
            _ => 3,
        };
        let (dst_w, dst_h) = oriented_fit(src_w, src_h, orientation, p);
        // The encoder sees the frame after rotation.
        let (enc_w, enc_h) = orientation.display_dims(dst_w, dst_h);
        // PNG has no shrink-on-load: the general arm stages the whole
        // frame in `chunk8`; the fused arm never does.
        let cost = if fused {
            DecodeCost::streaming()
        } else {
            DecodeCost::staged(src_w, src_h, channels)
        };
        check_decoded_bytes(
            cost.with_full_frame_resize((src_w, src_h), (dst_w, dst_h), channels, p)
                .with_exact_size_rotation((src_w, src_h), (dst_w, dst_h), channels, orientation)
                .with_encode(Some(target), enc_w, enc_h, channels, p)
                // Decoded straight from the reader: no `srcbuf` copy, and
                // its length may be a previous request's.
                .with_compressed(s.held_source_bytes),
            "PNG",
        )?;
    }
    if fused {
        let (dst_w, dst_h) = fit_dims(src_w, src_h, p.max_width, p.max_height);
        let fwd = fwd_lut();
        // Fully filled row by row; the row-count check below rejects
        // truncated streams before the buffer is consumed.
        scratch_u16(&mut s.src16, src_w * src_h * 3);
        let mut y = 0usize;
        while let Some(row) = png_reader.next_row().context("decode PNG")? {
            let dst = &mut s.src16[y * src_w * 3..(y + 1) * src_w * 3];
            for (d, &b) in dst.iter_mut().zip(row.data()) {
                *d = fwd[b as usize];
            }
            y += 1;
        }
        anyhow::ensure!(y == src_h, "PNG row count mismatch");
        let t_decode = t0.elapsed();
        let t1 = crate::clock::Instant::now();
        scratch_u16(&mut s.dst16, dst_w * dst_h * 3);
        resize_bands(
            u16_as_bytes(&s.src16[..src_w * src_h * 3]),
            src_w,
            src_h,
            u16_as_bytes_mut(&mut s.dst16[..dst_w * dst_h * 3]),
            dst_w,
            dst_h,
            PixelType::U16x3,
            p.parallel,
            &mut s.resizer,
        )?;
        let back = back_lut();
        let out = scratch_u8(&mut s.out8, dst_w * dst_h * 3);
        for (d, &v) in out.iter_mut().zip(&s.dst16[..dst_w * dst_h * 3]) {
            *d = back[v as usize];
        }
        let t_resize = t1.elapsed();
        let t2 = crate::clock::Instant::now();
        let out = encode_output(s, dst_w, dst_h, 3, target, p, icc.as_deref());
        if timing {
            eprintln!(
                "timing png(fused) decode+fwd({src_w}x{src_h})={:.1}ms resize+back={:.1}ms encode={:.1}ms",
                t_decode.as_secs_f64() * 1e3,
                t_resize.as_secs_f64() * 1e3,
                t2.elapsed().as_secs_f64() * 1e3
            );
        }
        return out;
    }

    let buf_len = png_reader.output_buffer_size().context("PNG too large")?;
    scratch_u8(&mut s.chunk8, buf_len);
    let info = png_reader
        .next_frame(&mut s.chunk8[..buf_len])
        .context("decode PNG")?;
    let (src_w, src_h) = (info.width as usize, info.height as usize);
    let len = info.buffer_size();

    // Normalize to RGB8/RGBA8 (EXPAND leaves grayscale as 1-2 channels).
    let channels = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Grayscale => {
            gray_to_rgb(s, len, 1);
            3
        }
        png::ColorType::GrayscaleAlpha => {
            gray_to_rgb(s, len, 2);
            4
        }
        png::ColorType::Indexed => anyhow::bail!("unexpanded indexed PNG"),
    };

    let t_decode = t0.elapsed();
    let t1 = crate::clock::Instant::now();
    let (dst_w, dst_h) = resize_pixels_oriented(s, channels, src_w, src_h, orientation, p)?;
    let t_resize = t1.elapsed();
    let t2 = crate::clock::Instant::now();
    let out = encode_output(s, dst_w, dst_h, channels, target, p, icc.as_deref());
    if timing {
        eprintln!(
            "timing png decode({src_w}x{src_h})={:.1}ms resize={:.1}ms encode={:.1}ms",
            t_decode.as_secs_f64() * 1e3,
            t_resize.as_secs_f64() * 1e3,
            t2.elapsed().as_secs_f64() * 1e3
        );
    }
    out
}

/// The stored-orientation resize target: fit the *displayed* frame,
/// then swap the axes back for axis-swapping orientations (the rotation
/// runs on the resized frame).
pub(super) fn oriented_fit(
    src_w: usize,
    src_h: usize,
    orientation: crate::meta::Orientation,
    p: &Resolved,
) -> (usize, usize) {
    let (disp_w, disp_h) = orientation.display_dims(src_w, src_h);
    let (fit_w, fit_h) = fit_dims(disp_w, disp_h, p.max_width, p.max_height);
    if orientation.swaps_axes() {
        (fit_h, fit_w)
    } else {
        (fit_w, fit_h)
    }
}

/// Expand grayscale(+alpha) pixels in `chunk8[..len]` to RGB(A) in place.
pub(super) fn gray_to_rgb(s: &mut Scratch, len: usize, in_ch: usize) {
    let out_ch = in_ch + 2;
    let pixels = len / in_ch;
    // The reverse loop below writes every output position.
    scratch_u8(&mut s.chunk8, pixels * out_ch);
    for i in (0..pixels).rev() {
        let g = s.chunk8[i * in_ch];
        let a = if in_ch == 2 {
            s.chunk8[i * in_ch + 1]
        } else {
            0
        };
        let o = i * out_ch;
        s.chunk8[o] = g;
        s.chunk8[o + 1] = g;
        s.chunk8[o + 2] = g;
        if in_ch == 2 {
            s.chunk8[o + 3] = a;
        }
    }
}

pub(super) fn process_webp<R: std::io::Read>(
    s: &mut Scratch,
    mut reader: R,
    target: ImageFormat,
    p: &Resolved,
) -> Result<Vec<u8>> {
    s.srcbuf.clear();
    reader
        .read_to_end(&mut s.srcbuf)
        .context("read WebP source")?;

    // Decode with libwebp's built-in scaler when we are shrinking well past
    // the target, keeping the same quality headroom as the JPEG DCT path:
    // decode at >= margin x target, then hand the remainder to linear-light
    // Lanczos. libwebp's scaler alone (scale straight to target) is what
    // costs other servers their score.
    let timing = crate::config::config().timing;
    let t0 = crate::clock::Instant::now();
    // One mux parse serves both metadata chunks: the ICC profile and
    // the EXIF orientation (raw TIFF or JPEG-style prefixed; writers
    // disagree, browsers accept both).
    let (icc, exif) = webp_metadata(
        &s.srcbuf,
        p.icc_passthrough && target_supports_icc(target),
        p.auto_rotate,
    );
    let orientation = exif
        .and_then(|d| crate::meta::Orientation::from_exif_payload(&d))
        .unwrap_or(crate::meta::Orientation::UPRIGHT);
    // Animated sources render their first frame, like other image
    // proxies: swap the container for the frame's bitstream (metadata
    // was already read from the full container above). A first frame
    // that does not cover the canvas keeps the original bytes and
    // fails with the animation error below, as before.
    if let Some(frame) = webp_first_frame(&s.srcbuf) {
        s.srcbuf.clear();
        s.srcbuf.extend_from_slice(&frame);
    }
    let (src_w, src_h, channels, dec_w, dec_h) =
        webp_decode_into_chunk8(s, orientation, target, p)?;
    let _ = (src_w, src_h);
    let t_dec = t0.elapsed();

    let t1 = crate::clock::Instant::now();
    let (dst_w, dst_h) = resize_pixels_oriented(s, channels, dec_w, dec_h, orientation, p)?;
    let t_resize = t1.elapsed();

    let t2 = crate::clock::Instant::now();
    let out = encode_output(s, dst_w, dst_h, channels, target, p, icc.as_deref())?;
    if timing {
        eprintln!(
            "timing webp decode({dec_w}x{dec_h})={:.1}ms resize={:.1}ms encode={:.1}ms",
            t_dec.as_secs_f64() * 1e3,
            t_resize.as_secs_f64() * 1e3,
            t2.elapsed().as_secs_f64() * 1e3
        );
    }
    Ok(out)
}

/// AVIF: decode via dav1d, resize, re-encode via SVT-AV1. AV1 has no
/// reduced-resolution decode mode, so unlike JPEG/WebP the decode always
/// runs at full source resolution.
#[cfg(feature = "avif")]
pub(super) fn process_avif<R: std::io::Read>(
    s: &mut Scratch,
    mut reader: R,
    target: ImageFormat,
    p: &Resolved,
) -> Result<Vec<u8>> {
    s.srcbuf.clear();
    reader
        .read_to_end(&mut s.srcbuf)
        .context("read AVIF source")?;

    let timing = crate::config::config().timing;
    let t0 = crate::clock::Instant::now();
    // avif-parse exposes neither colr nor irot/imir; both come from
    // our own bounded container walk.
    let icc = if p.icc_passthrough && target_supports_icc(target) {
        crate::avif::extract_icc(&s.srcbuf)
    } else {
        None
    };
    let orientation = if p.auto_rotate {
        crate::avif::extract_orientation(&s.srcbuf)
    } else {
        crate::meta::Orientation::UPRIGHT
    };
    {
        // Before the decode allocates: dav1d has no shrink-on-load, so
        // the whole frame materializes. Dimensions come from the
        // container probe; four channels covers the alpha auxiliary
        // item.
        let (pw, ph) = crate::avif::probe_avif(&s.srcbuf)?;
        check_src_pixels(pw, ph)?;
        let (ow, oh) = oriented_fit(pw, ph, orientation, p);
        let (enc_w, enc_h) = orientation.display_dims(ow, oh);
        let mut cost = DecodeCost::staged(pw, ph, 4);
        // dav1d's pictures and frame state next to `chunk8`, still
        // resident when the resize allocates (freed memory the
        // allocator keeps): 2.1 B/px for 8-bit 4:2:0, 6.8 for 12-bit
        // 4:4:4, 10.5 for 10-bit 4:4:4 with noise alpha (decode alone,
        // 4000², M2 Max).
        cost.staged_bytes = cost
            .staged_bytes
            .saturating_add((pw as u64).saturating_mul(ph as u64).saturating_mul(12));
        check_decoded_bytes(
            cost.with_full_frame_resize((pw, ph), (ow, oh), 4, p)
                .with_exact_size_rotation((pw, ph), (ow, oh), 4, orientation)
                .with_encode(Some(target), enc_w, enc_h, 4, p)
                .with_compressed(s.srcbuf.len() + s.held_source_bytes),
            "AVIF",
        )?;
    }
    let (src_w, src_h, channels) = crate::avif::decode_avif_into(&s.srcbuf, &mut s.chunk8)?;
    let t_dec = t0.elapsed();

    let t1 = crate::clock::Instant::now();
    let (dst_w, dst_h) = resize_pixels_oriented(s, channels, src_w, src_h, orientation, p)?;
    let t_resize = t1.elapsed();

    let t2 = crate::clock::Instant::now();
    let out = encode_output(s, dst_w, dst_h, channels, target, p, icc.as_deref())?;
    if timing {
        eprintln!(
            "timing avif decode({src_w}x{src_h})={:.1}ms resize={:.1}ms encode={:.1}ms",
            t_dec.as_secs_f64() * 1e3,
            t_resize.as_secs_f64() * 1e3,
            t2.elapsed().as_secs_f64() * 1e3
        );
    }
    Ok(out)
}

/// Decode `srcbuf` (WebP) into `chunk8`, scaling during decode down to
/// margin x target when the source is much larger. Returns
/// (src_w, src_h, channels, decoded_w, decoded_h).
pub(super) fn webp_decode_into_chunk8(
    s: &mut Scratch,
    orientation: crate::meta::Orientation,
    target: ImageFormat,
    p: &Resolved,
) -> Result<(usize, usize, usize, usize, usize)> {
    use libwebp_sys as w;
    // SAFETY: FFI cluster reading only `s.srcbuf`, which is live for the whole
    // block. A zeroed WebPDecoderConfig is libwebp's documented pre-init state and
    // the WebPInitDecoderConfig ABI check is enforced; `output.u.RGBA` is the arm
    // libwebp fills for MODE_RGB/MODE_RGBA. WebPFreeDecBuffer runs exactly once on
    // both the error and success paths.
    unsafe {
        let mut config: w::WebPDecoderConfig = std::mem::zeroed();
        anyhow::ensure!(
            w::WebPInitDecoderConfig(&mut config),
            "libwebp ABI mismatch"
        );
        let status = w::WebPGetFeatures(s.srcbuf.as_ptr(), s.srcbuf.len(), &mut config.input);
        anyhow::ensure!(
            status == w::VP8StatusCode::VP8_STATUS_OK,
            "parse WebP header"
        );
        anyhow::ensure!(
            config.input.has_animation == 0,
            "animated WebP is unsupported"
        );
        let (src_w, src_h) = (config.input.width as usize, config.input.height as usize);
        check_src_pixels(src_w, src_h)?;
        let channels = if config.input.has_alpha != 0 { 4 } else { 3 };
        // Conservative for WebP: libwebp's decode scaler may shrink
        // this below the source (decided a few lines down), so the
        // full frame is an upper bound, not the exact figure.
        let (ow, oh) = oriented_fit(src_w, src_h, orientation, p);
        let (enc_w, enc_h) = orientation.display_dims(ow, oh);
        let mut cost = DecodeCost::staged(src_w, src_h, channels as u64);
        // VP8L keeps the whole image as ARGB words while it decodes,
        // even when the output is scaled: lossless sources, and the
        // alpha plane of lossy ones. Measured 4.0-4.1 B/px lossless and
        // 2.1 for lossy alpha (4000², M2 Max); lossy RGB streams rows.
        if config.input.format != 1 || config.input.has_alpha != 0 {
            cost.staged_bytes = cost.staged_bytes.saturating_add(
                (src_w as u64)
                    .saturating_mul(src_h as u64)
                    .saturating_mul(4),
            );
        }
        check_decoded_bytes(
            cost.with_full_frame_resize((src_w, src_h), (ow, oh), channels as u64, p)
                .with_exact_size_rotation((src_w, src_h), (ow, oh), channels as u64, orientation)
                .with_encode(Some(target), enc_w, enc_h, channels as u64, p)
                .with_compressed(s.srcbuf.len() + s.held_source_bytes),
            "WebP",
        )?;

        // The stored-space resize target: fit the *displayed* frame,
        // swap back for axis-swapping orientations. Deciding the decode
        // scale from the unoriented fit would under-decode sources
        // whose displayed aspect fits the box differently.
        let (dst_w, dst_h) = (ow, oh);
        // Unlike the streaming JPEG arm, this one keeps its shrink by
        // default: libwebp stages the whole frame, so the decode size
        // is the resident set. Full decode does score better — 69.27
        // against 64.15 on 4000x2667 sources into 750x500, monotonic
        // rather than the JPEG path's erratic dips, because libwebp
        // rescales properly instead of truncating DCT coefficients —
        // but it costs 133.9 MB against 21.7. Five points is not worth
        // six times the memory here. The knob still overrides.
        let margin = dct_margin().unwrap_or(BUFFERED_DCT_MARGIN);
        let need_w = ((dst_w as f64) * margin).ceil() as usize;
        let (dec_w, dec_h) = if need_w < src_w {
            let scale = need_w as f64 / src_w as f64;
            (
                need_w.max(dst_w),
                (((src_h as f64) * scale).round() as usize).max(dst_h),
            )
        } else {
            (src_w, src_h)
        };
        if (dec_w, dec_h) != (src_w, src_h) {
            config.options.use_scaling = 1;
            config.options.scaled_width = dec_w as i32;
            config.options.scaled_height = dec_h as i32;
        }
        // libwebp's threaded decode pipelines entropy decoding and
        // reconstruction across two threads (the same setting libvips
        // ships); like band-parallel resize this briefly exceeds the CPU
        // slot without oversubscribing on average.
        if crate::config::config().webp_decode_threads {
            config.options.use_threads = 1;
        }
        config.output.colorspace = if channels == 4 {
            w::WEBP_CSP_MODE::MODE_RGBA
        } else {
            w::WEBP_CSP_MODE::MODE_RGB
        };

        let status = w::WebPDecode(s.srcbuf.as_ptr(), s.srcbuf.len(), &mut config);
        if status != w::VP8StatusCode::VP8_STATUS_OK {
            w::WebPFreeDecBuffer(&mut config.output);
            anyhow::bail!("decode WebP: {status:?}");
        }
        let buf = &config.output.u.RGBA;
        let stride = buf.stride as usize;
        let row = dec_w * channels;
        // Every row is copied below before the buffer is read.
        scratch_u8(&mut s.chunk8, dec_h * row);
        for y in 0..dec_h {
            // SAFETY: after VP8_STATUS_OK the output buffer holds dec_h rows spaced
            // `stride` bytes apart, each with dec_w * channels (= row) valid bytes, per
            // the WebPRGBABuffer contract; y < dec_h keeps every read in bounds, and the
            // buffer is not freed until after this loop.
            let src_row = std::slice::from_raw_parts(buf.rgba.add(y * stride), row);
            s.chunk8[y * row..(y + 1) * row].copy_from_slice(src_row);
        }
        w::WebPFreeDecBuffer(&mut config.output);
        Ok((src_w, src_h, channels, dec_w, dec_h))
    }
}

/// Resize the fully decoded pixels in `chunk8` honoring an EXIF-style
/// orientation: the box fits the *displayed* frame, the resize runs in
/// the stored orientation, and the pixels rotate afterwards on the
/// small output frame — the same strategy as the JPEG arm, shared by
/// the PNG/WebP/AVIF full-frame paths.
pub(super) fn resize_pixels_oriented(
    s: &mut Scratch,
    channels: usize,
    src_w: usize,
    src_h: usize,
    orientation: crate::meta::Orientation,
    p: &Resolved,
) -> Result<(usize, usize)> {
    let (dst_w, dst_h) = oriented_fit(src_w, src_h, orientation, p);
    resize_pixels_to(s, channels, src_w, src_h, dst_w, dst_h, p)?;
    if orientation.is_upright() {
        return Ok((dst_w, dst_h));
    }
    // chunk8 held the decoded source; it is free once the resize is
    // done, so rotate into it and swap it in as out8.
    let dims = crate::meta::apply_orientation(
        &s.out8[..dst_w * dst_h * channels],
        dst_w,
        dst_h,
        channels,
        orientation,
        &mut s.chunk8,
    );
    std::mem::swap(&mut s.out8, &mut s.chunk8);
    Ok(dims)
}

/// Resize the fully decoded pixels in `chunk8` (3 or 4 channels) into
/// `out8` at exactly `dst_w`x`dst_h`. RGB follows the same
/// linear-light path as JPEG; alpha images are premultiplied before
/// resampling and unpremultiplied after.
///
/// `chunk8` does not survive: at exact size its buffer *becomes*
/// `out8` (and `chunk8` is left holding the old `out8`), and in sRGB
/// mode alpha is premultiplied in place. The animated-GIF canvas,
/// which persists across frames, therefore skips this at exact size;
/// its resized frames still come here, where the in-place premultiply
/// is an exact no-op: every canvas pixel is either opaque (drawn) or
/// all zero (never drawn, or cleared).
pub(super) fn resize_pixels_to(
    s: &mut Scratch,
    channels: usize,
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
    p: &Resolved,
) -> Result<(usize, usize)> {
    let src_len = src_w * src_h * channels;
    if (src_w, src_h) == (dst_w, dst_h) {
        // No resize: hand the decoded frame over instead of copying it.
        // A copy kept two full frames resident (147 MB each at 7000^2
        // RGB, issue #129). Every decoder sizes chunk8 to at least the
        // frame, and every later chunk8 user re-sizes it through
        // scratch_u8 or `resize`, so the shorter buffer swapped in is safe.
        debug_assert!(s.chunk8.len() >= src_len);
        std::mem::swap(&mut s.out8, &mut s.chunk8);
        return Ok((dst_w, dst_h));
    }

    if p.linear_light {
        let (fwd, back) = (fwd_lut(), back_lut());
        // Fully overwritten by the LUT/premultiply loops just below.
        scratch_u16(&mut s.src16, src_len);
        if channels == 4 {
            for (d, src) in s.src16[..src_len]
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(s.chunk8[..src_len].as_chunks::<4>().0)
            {
                let a = src[3] as u32 * 257;
                for c in 0..3 {
                    // Premultiply in linear light so resampling never bleeds
                    // color from fully transparent pixels.
                    d[c] = ((fwd[src[c] as usize] as u32 * a) / 65535) as u16;
                }
                d[3] = a as u16;
            }
        } else {
            for (d, src) in s.src16[..src_len].iter_mut().zip(&s.chunk8[..src_len]) {
                *d = fwd[*src as usize];
            }
        }
        let dst_len = dst_w * dst_h * channels;
        scratch_u16(&mut s.dst16, dst_len);
        resize_bands(
            u16_as_bytes(&s.src16[..src_len]),
            src_w,
            src_h,
            u16_as_bytes_mut(&mut s.dst16[..dst_len]),
            dst_w,
            dst_h,
            if channels == 4 {
                PixelType::U16x4
            } else {
                PixelType::U16x3
            },
            p.parallel,
            &mut s.resizer,
        )?;
        scratch_u8(&mut s.out8, dst_len);
        if channels == 4 {
            for (d, src) in s.out8[..dst_len]
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(s.dst16[..dst_len].as_chunks::<4>().0)
            {
                let a = src[3] as u32;
                for (out, &pre) in d[..3].iter_mut().zip(&src[..3]) {
                    let un = (pre as u32 * 65535)
                        .checked_div(a)
                        .map_or(0, |v| v.min(65535)) as u16;
                    *out = back[un as usize];
                }
                d[3] = (a / 257) as u8;
            }
        } else {
            for (d, src) in s.out8[..dst_len].iter_mut().zip(&s.dst16[..dst_len]) {
                *d = back[*src as usize];
            }
        }
    } else {
        if channels == 4 {
            // Premultiply in place (u8 approximation for speed mode).
            for px in s.chunk8[..src_len].as_chunks_mut::<4>().0 {
                let a = px[3] as u32;
                for c in px[..3].iter_mut() {
                    *c = ((*c as u32 * a + 127) / 255) as u8;
                }
            }
        }
        let dst_len = dst_w * dst_h * channels;
        scratch_u8(&mut s.out8, dst_len);
        let (chunk8, out8) = (&s.chunk8, &mut s.out8);
        resize_bands(
            &chunk8[..src_len],
            src_w,
            src_h,
            &mut out8[..dst_len],
            dst_w,
            dst_h,
            if channels == 4 {
                PixelType::U8x4
            } else {
                PixelType::U8x3
            },
            p.parallel,
            &mut s.resizer,
        )?;
        if channels == 4 {
            for px in s.out8[..dst_len].as_chunks_mut::<4>().0 {
                let a = px[3] as u32;
                for c in px[..3].iter_mut() {
                    *c = (*c as u32 * 255).checked_div(a).map_or(0, |v| v.min(255)) as u8;
                }
            }
        }
    }
    Ok((dst_w, dst_h))
}
