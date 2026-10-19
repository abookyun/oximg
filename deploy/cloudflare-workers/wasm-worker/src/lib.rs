//! Path B: oximg's pipeline as a plain wasm32-wasip1 reactor with a C
//! ABI. src/index.js (a JS Worker) owns HTTP and the WASI shim; this
//! module only turns bytes into bytes.
//!
//! JS: p = oximg_alloc(n); copy the source in; r = oximg_process(p, n,
//! w, h, fmt, q); read the Out at r; oximg_out_free(r).

use oximg::pipeline::{self, ImageFormat, Params};

/// Read by JS at fixed offsets: five little-endian u32 words.
#[repr(C)]
pub struct Out {
    /// 0 = ok, 1 = pipeline error (the message is in ptr/len).
    status: u32,
    /// Output format code (same table as `fmt`), or 0 on error.
    format: u32,
    /// Image bytes on success, a UTF-8 error message on failure.
    ptr: *mut u8,
    len: usize,
    cap: usize,
}

fn format_from_code(c: u32) -> Option<ImageFormat> {
    match c {
        1 => Some(ImageFormat::Jpeg),
        2 => Some(ImageFormat::Png),
        3 => Some(ImageFormat::Webp),
        _ => None,
    }
}

fn code_from_format(f: ImageFormat) -> u32 {
    match f {
        ImageFormat::Jpeg => 1,
        ImageFormat::Png => 2,
        ImageFormat::Webp => 3,
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn oximg_alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len);
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// # Safety
/// `p`/`cap` must come from `oximg_alloc(cap)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oximg_free(p: *mut u8, cap: usize) {
    drop(unsafe { Vec::from_raw_parts(p, 0, cap) });
}

/// # Safety
/// `src` must point to `len` initialized bytes owned by the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oximg_process(
    src: *const u8,
    len: usize,
    max_w: u32,
    max_h: u32,
    fmt: u32,
    quality: f32,
) -> *mut Out {
    let src = unsafe { std::slice::from_raw_parts(src, len) };
    let p = Params {
        max_width: if max_w == 0 { u32::MAX } else { max_w },
        max_height: if max_h == 0 { u32::MAX } else { max_h },
        quality: if quality > 0.0 { quality } else { 80.0 },
        output: format_from_code(fmt),
        ..Params::default()
    };
    let (status, format, mut bytes) = match pipeline::process(src, &p) {
        Ok((out, f)) => (0, code_from_format(f), out),
        Err(e) => (1, 0, format!("{:?}: {e}", e.kind()).into_bytes()),
    };
    let out = Box::new(Out {
        status,
        format,
        ptr: bytes.as_mut_ptr(),
        len: bytes.len(),
        cap: bytes.capacity(),
    });
    std::mem::forget(bytes);
    Box::into_raw(out)
}

/// # Safety
/// `out` must come from `oximg_process` and not be freed twice.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oximg_out_free(out: *mut Out) {
    let out = unsafe { Box::from_raw(out) };
    drop(unsafe { Vec::from_raw_parts(out.ptr, out.len, out.cap) });
}
