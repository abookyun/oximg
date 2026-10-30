//! Issue #130: the decoded-bytes estimate against the peak RSS of the
//! compiled binary, one cell per decode arm and output encoder, driven
//! through `oximg-ctl resize` (which reports both). The estimate must
//! cover what the process held, and stay within a bound of it, so the
//! model and the code cannot drift apart again.
//!
//! Inputs are generated into a temp dir (hermetic). Noise is the worst
//! case for every content-dependent buffer (compressed sizes, encoder
//! working sets), which is what the estimate is priced at; a smooth
//! "photo" shows the other end. 2048² keeps each frame-sized buffer
//! well above the few MB of process state the estimate does not count.

use oximg::pipeline::{self, ImageFormat, Params};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

const N: u32 = 2048;

/// State that does not scale with the image and that the estimate does
/// not count: decoder state, coefficient tables, code pages touched by
/// the arm (README, `OXIMG_MAX_DECODED_BYTES`). Measured at ~1.5 MB
/// above the tiny-input baseline on the fused PNG arm, the one whose
/// dominant buffer the estimate prices exactly.
const UNCOUNTED: u64 = 2 << 20;

fn ctl() -> Command {
    let oximg = Path::new(env!("CARGO_BIN_EXE_oximg"));
    let ctl = oximg.with_file_name(if cfg!(windows) {
        "oximg-ctl.exe"
    } else {
        "oximg-ctl"
    });
    let mut c = Command::new(&ctl);
    c.arg("--bin").arg(oximg);
    c
}

/// One `oximg-ctl resize` with the estimate reported: (estimate, peak).
fn measure(input: &Path, w: &str, h: &str, out: &Path, env: &[&str]) -> (u64, u64) {
    let mut c = ctl();
    c.args(["--env", "OXIMG_LOG_DECODED_BYTES_ABOVE=1"]);
    for e in env {
        c.args(["--env", e]);
    }
    c.arg("resize").arg(input).args([w, h, "--out"]).arg(out);
    let o = c.output().expect("run oximg-ctl");
    let v: Value = serde_json::from_slice(&o.stdout).unwrap_or_else(|e| {
        panic!(
            "ctl stdout was not JSON ({e}): {}",
            String::from_utf8_lossy(&o.stderr)
        )
    });
    assert_eq!(v["ok"], true, "{v}");
    let est = v["decoded_bytes"]["total"]
        .as_u64()
        .unwrap_or_else(|| panic!("no estimate reported: {v}"));
    (est, v["peak_rss_bytes"].as_u64().expect("peak RSS"))
}

/// xorshift: deterministic noise without a dependency.
fn noise(len: usize, mut seed: u32) -> Vec<u8> {
    (0..len)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed >> 24) as u8
        })
        .collect()
}

fn write_png(path: &Path, px: &[u8], channels: usize) {
    write_png_sized(path, N, px, channels);
}

fn write_png_sized(path: &Path, side: u32, px: &[u8], channels: usize) {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, side, side);
    enc.set_color(if channels == 4 {
        png::ColorType::Rgba
    } else {
        png::ColorType::Rgb
    });
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(px).unwrap();
    std::fs::write(path, out).unwrap();
}

/// Re-encode `src` through the library into `format` at its own size.
fn transcode(src: &Path, dst: &Path, format: ImageFormat) {
    let (bytes, _) = pipeline::process(
        &std::fs::read(src).unwrap(),
        &Params {
            max_width: N,
            max_height: N,
            output: Some(format),
            ..Params::default()
        },
    )
    .unwrap();
    std::fs::write(dst, bytes).unwrap();
}

/// `N x N` RGBA as lossless WebP (oximg's own encoder is lossy only).
fn lossless_webp(rgba: &[u8]) -> Vec<u8> {
    let mut out: *mut u8 = std::ptr::null_mut();
    // SAFETY: `rgba` holds N*N*4 bytes at stride N*4; libwebp allocates
    // `out`, which is copied and released with WebPFree exactly once.
    unsafe {
        let len = libwebp_sys::WebPEncodeLosslessRGBA(
            rgba.as_ptr(),
            N as i32,
            N as i32,
            (N * 4) as i32,
            &mut out,
        );
        assert!(len > 0 && !out.is_null(), "lossless WebP encode failed");
        let bytes = std::slice::from_raw_parts(out, len).to_vec();
        libwebp_sys::WebPFree(out.cast());
        bytes
    }
}

/// `frames` frames of `side x side` palette noise with a transparent
/// index: the animated path, at its one-bit-alpha worst case.
fn noise_gif(side: u16, frames: u32) -> Vec<u8> {
    let palette: Vec<u8> = noise(256 * 3, 0x1656_67B1);
    let mut out = Vec::new();
    {
        let mut enc = gif::Encoder::new(&mut out, side, side, &palette).unwrap();
        enc.set_repeat(gif::Repeat::Infinite).unwrap();
        for i in 0..frames {
            let px = noise(side as usize * side as usize, 0x2545_F491 ^ i);
            let mut frame = gif::Frame::from_indexed_pixels(side, side, px, Some(0));
            frame.delay = 10;
            enc.write_frame(&frame).unwrap();
        }
    }
    out
}

struct Inputs {
    dir: PathBuf,
}

impl Inputs {
    fn new() -> Inputs {
        let dir = std::env::temp_dir().join(format!("oximg-decbytes-rss-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let n = (N * N) as usize;
        write_png(&dir.join("noise.png"), &noise(n * 3, 0x9E37_79B9), 3);
        write_png(&dir.join("noise_rgba.png"), &noise(n * 4, 0x85EB_CA6B), 4);
        let photo: Vec<u8> = (0..n)
            .flat_map(|i| {
                let (x, y) = ((i % N as usize) as u32, (i / N as usize) as u32);
                [
                    (x * 255 / N) as u8,
                    (y * 255 / N) as u8,
                    ((x + y) * 127 / N) as u8,
                ]
            })
            .collect();
        write_png(&dir.join("photo.png"), &photo, 3);
        write_png_sized(&dir.join("tiny.png"), 8, &[128; 8 * 8 * 3], 3);
        transcode(
            &dir.join("noise.png"),
            &dir.join("noise.jpg"),
            ImageFormat::Jpeg,
        );
        transcode(
            &dir.join("photo.png"),
            &dir.join("photo.jpg"),
            ImageFormat::Jpeg,
        );
        transcode(
            &dir.join("noise_rgba.png"),
            &dir.join("alpha.webp"),
            ImageFormat::Webp,
        );
        #[cfg(feature = "avif")]
        transcode(
            &dir.join("noise_rgba.png"),
            &dir.join("alpha.avif"),
            ImageFormat::Avif,
        );
        std::fs::write(
            dir.join("lossless.webp"),
            lossless_webp(&noise(n * 4, 0x27D4_EB2F)),
        )
        .unwrap();
        std::fs::write(dir.join("anim.gif"), noise_gif(1024, 6)).unwrap();
        Inputs { dir }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

impl Drop for Inputs {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// (label, input, w, h, output extension, env, max estimate/held).
///
/// The bounds are the higher of two measured ratios plus ~25%: M2 Max
/// (macOS) and Ryzen 7 8745HS (Linux, glibc, SVT-AV1 built as CI builds
/// it), 2026-10-30. A distribution SVT-AV1 built with its memory
/// tracking on holds ~200 MB more per encode; CI does not use one.
type Cell = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static [&'static str],
    f64,
);

const CELLS: &[Cell] = &[
    // JPEG source: exact size, the row kernel, and both full-frame arms.
    (
        "jpeg exact, progressive",
        "noise.jpg",
        "0",
        "0",
        "jpg",
        &[],
        2.0,
    ),
    (
        "jpeg exact, sequential",
        "noise.jpg",
        "0",
        "0",
        "jpg",
        &["OXIMG_JPEG_PROGRESSIVE=0"],
        2.4,
    ),
    (
        "jpeg kernel resize",
        "noise.jpg",
        "1024",
        "1024",
        "jpg",
        &[],
        1.6,
    ),
    (
        "jpeg sRGB resize",
        "noise.jpg",
        "1024",
        "1024",
        "jpg",
        &["OXIMG_RESIZE=srgb"],
        2.2,
    ),
    (
        "jpeg fir resize",
        "noise.jpg",
        "1024",
        "1024",
        "jpg",
        &["OXIMG_RESIZE_BACKEND=fir"],
        2.0,
    ),
    // PNG source: the fused arm, exact size, the staged RGBA arm.
    (
        "png fused thumbnail",
        "noise.png",
        "10",
        "200",
        "png",
        &[],
        1.3,
    ),
    ("png exact to png", "noise.png", "0", "0", "png", &[], 1.7),
    ("png exact to jpeg", "noise.png", "0", "0", "jpg", &[], 1.5),
    (
        "png rgba resize",
        "noise_rgba.png",
        "1024",
        "1024",
        "png",
        &[],
        1.3,
    ),
    // Encoders priced at their worst case.
    (
        "png rgba to webp",
        "noise_rgba.png",
        "0",
        "0",
        "webp",
        &[],
        1.6,
    ),
    (
        "png quantized",
        "noise.png",
        "0",
        "0",
        "png",
        &["OXIMG_PNG_QUANTIZE=1"],
        1.5,
    ),
    // WebP source: VP8L-coded alpha.
    ("webp alpha exact", "alpha.webp", "0", "0", "png", &[], 1.8),
    // VP8L holds the whole image as ARGB while it decodes.
    (
        "webp lossless exact",
        "lossless.webp",
        "0",
        "0",
        "png",
        &[],
        2.5,
    ),
    // Every frame composited, resized and encoded; libwebp retains them.
    ("gif animation", "anim.gif", "0", "0", "webp", &[], 3.0),
    // Photo content: the low end of the content-dependent encoders,
    // which the header cannot tell from noise.
    ("photo png exact", "photo.png", "0", "0", "png", &[], 3.6),
    ("photo jpeg exact", "photo.jpg", "0", "0", "jpg", &[], 3.5),
];

#[cfg(feature = "avif")]
const AVIF_CELLS: &[Cell] = &[
    (
        "png rgba to avif",
        "noise_rgba.png",
        "0",
        "0",
        "avif",
        &[],
        1.8,
    ),
    ("avif alpha exact", "alpha.avif", "0", "0", "png", &[], 2.1),
    (
        "avif alpha resize",
        "alpha.avif",
        "1024",
        "1024",
        "png",
        &[],
        2.0,
    ),
];
#[cfg(not(feature = "avif"))]
const AVIF_CELLS: &[Cell] = &[];

/// One test, cells in sequence: concurrent cells would be measured
/// under each other's memory pressure.
#[cfg(unix)]
#[test]
fn estimate_covers_peak_rss_on_every_arm() {
    let inputs = Inputs::new();
    let (_, baseline) = measure(
        &inputs.path("tiny.png"),
        "0",
        "0",
        &inputs.path("tiny.out.png"),
        &[],
    );
    let mut failures = Vec::new();
    for &(label, input, w, h, ext, env, max_ratio) in CELLS.iter().chain(AVIF_CELLS) {
        let out = inputs.path(&format!("out.{ext}"));
        let (est, peak) = measure(&inputs.path(input), w, h, &out, env);
        let held = peak.saturating_sub(baseline);
        let ratio = est as f64 / held as f64;
        eprintln!("{label}: estimate {est}, peak {peak} (held {held}), {ratio:.2}x");
        if est + UNCOUNTED < held {
            failures.push(format!(
                "{label}: estimate {est} under the {held} bytes held"
            ));
        }
        if ratio > max_ratio {
            failures.push(format!(
                "{label}: estimate {est} is {ratio:.2}x the {held} bytes held (max {max_ratio})"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
