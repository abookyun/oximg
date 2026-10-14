//! `matrix`: walk fixture × box × format cells (plus 400/404 negatives)
//! against one server, and prove each 200 is an image of the labeled
//! type and the expected geometry.

use crate::args::{MatrixArgs, Opts};
use crate::error::{CtlError, emit};
use crate::http::{Http, HttpResult, content_type_matches_token, get_report, media_type_essence};
use crate::spawn::{child_auto_rotate, images_dir, remote_source_base, server_base};
use crate::urlpath::percent_encode_path;
use oximg::pipeline::{self, ImageFormat};
use serde_json::{Value, json};
use std::path::Path;
use std::time::{Duration, Instant};

pub fn run(opts: &Opts, a: &MatrixArgs) -> Result<(), CtlError> {
    let MatrixArgs {
        sources,
        boxes,
        formats,
        negatives,
        base,
        timeout_secs,
    } = a;
    let (negatives, timeout_secs) = (*negatives, *timeout_secs);
    let sources = if sources.is_empty() {
        default_sources()
    } else {
        sources.clone()
    };
    let boxes = if boxes.is_empty() {
        vec![(100, 100)]
    } else {
        boxes.clone()
    };
    let formats = if formats.is_empty() {
        default_formats()
    } else {
        formats.clone()
    };
    // Local fixtures are only an oracle when we spawned the server.
    // --base talks to someone else's tree; sniffing our images_dir
    // would fail a correct remote PNG served as photo.jpg.
    let sniff_local = base.is_none() && !remote_source_base(opts);
    let rotate = child_auto_rotate(opts);

    struct Cell {
        path: String,
        expect: u16,
        kind: &'static str,
        format: Option<String>,
        box_w: u32,
        box_h: u32,
        expected_wh: Option<(u32, u32)>,
    }
    let images = images_dir(opts);
    let mut plan: Vec<Cell> = Vec::new();
    for src in &sources {
        let src_enc = percent_encode_path(src);
        let src_wh = sniff_local
            .then(|| source_display_size(src, &images, rotate))
            .flatten();
        for (w, h) in &boxes {
            let expected_wh = src_wh.map(|(sw, sh)| fit_box(sw, sh, *w, *h));
            for fmt in &formats {
                let (path, format) = match fmt.as_str() {
                    "" | "source" => (
                        format!("/resize/{w}/{h}/{src_enc}"),
                        if sniff_local {
                            source_output_token(src, &images)
                        } else {
                            None
                        },
                    ),
                    token => (
                        format!("/resize/{w}/{h}/{src_enc}@{token}"),
                        Some(token.to_string()),
                    ),
                };
                plan.push(Cell {
                    path,
                    expect: 200,
                    kind: "cell",
                    format,
                    box_w: *w,
                    box_h: *h,
                    expected_wh,
                });
            }
        }
    }
    if negatives {
        plan.push(Cell {
            path: "/resize/0/0/photo.jpg".into(),
            expect: 400,
            kind: "negative",
            format: None,
            box_w: 0,
            box_h: 0,
            expected_wh: None,
        });
        // Only when that key is actually absent from this images dir.
        // --images-dir may contain a real missing.jpg; --base is a
        // foreign tree.
        if sniff_local && matches!(images.join("missing.jpg").try_exists(), Ok(false)) {
            plan.push(Cell {
                path: "/resize/100/100/missing.jpg".into(),
                expect: 404,
                kind: "negative",
                format: None,
                box_w: 100,
                box_h: 100,
                expected_wh: None,
            });
        }
        plan.push(Cell {
            path: "/resize/100/100/photo.jpg@gif".into(),
            expect: 400,
            kind: "negative",
            format: None,
            box_w: 100,
            box_h: 100,
            expected_wh: None,
        });
    }

    if opts.dry_run {
        emit(
            &json!({
                "ok": true,
                "dry_run": true,
                "cells": plan.iter().map(|c| json!({
                    "path": c.path,
                    "expect": c.expect,
                    "kind": c.kind,
                })).collect::<Vec<_>>(),
            }),
            opts.pretty,
        );
        return Ok(());
    }

    let http = Http::new().map_err(|e| CtlError::fail(e, None))?;
    let (_spawned, base) = server_base(opts, base.as_deref(), &http)?;

    let t0 = Instant::now();
    let timeout = Duration::from_secs(timeout_secs);
    let mut cells = Vec::new();
    let mut failed = 0u32;
    for c in &plan {
        let url = format!("{base}{}", c.path);
        match http.get(&url, None, timeout) {
            Ok(res) => {
                let mut row = get_report(&res, None)?;
                let mut pass = res.status == c.expect;
                if pass && c.expect == 200 {
                    pass = matrix_200_proved(&res, &row)
                        && matrix_geometry_ok(&row, c.box_w, c.box_h, c.expected_wh);
                    if pass && let Some(token) = c.format.as_deref() {
                        let ct = res
                            .headers
                            .get("content-type")
                            .map(String::as_str)
                            .unwrap_or("");
                        pass = content_type_matches_token(ct, token);
                    }
                }
                if !pass {
                    failed += 1;
                }
                row["path"] = json!(c.path);
                row["expect"] = json!(c.expect);
                row["kind"] = json!(c.kind);
                row["pass"] = json!(pass);
                // Nested ok:true on a failing cell is confusing; the
                // top-level ok is the proof.
                row.as_object_mut().unwrap().remove("ok");
                cells.push(row);
            }
            Err(e) => {
                failed += 1;
                cells.push(json!({
                    "path": c.path,
                    "expect": c.expect,
                    "kind": c.kind,
                    "pass": false,
                    "error": e,
                }));
            }
        }
    }
    let ms = t0.elapsed().as_secs_f64() * 1e3;
    let report = json!({
        "ok": failed == 0,
        "failed": failed,
        "total": cells.len(),
        "ms": (ms * 10.0).round() / 10.0,
        "base": base,
        "cells": cells,
    });
    if failed > 0 {
        return Err(CtlError::from_value(1, report));
    }
    emit(&report, opts.pretty);
    Ok(())
}

fn default_sources() -> Vec<String> {
    let mut v = vec![
        "photo.jpg".into(),
        "rgb.png".into(),
        "photo.webp".into(),
        "still.gif".into(),
        "anim.gif".into(),
    ];
    if cfg!(feature = "avif") {
        v.push("photo.avif".into());
    }
    v
}

fn default_formats() -> Vec<String> {
    vec!["".into(), "webp".into()]
}

/// Bare (no `@{fmt}`) output codec. Prefer sniffing bytes — extensions
/// are never trusted. GIF sources become WebP. Fallback to the
/// extension only when the file cannot be read.
fn source_output_token(src: &str, images_dir: &Path) -> Option<String> {
    let path = Path::new(src);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        images_dir.join(src)
    };
    if let Ok(bytes) = std::fs::read(&path)
        && let Ok((fmt, _, _)) = pipeline::probe(&bytes)
    {
        return Some(
            match fmt {
                ImageFormat::Gif => "webp",
                ImageFormat::Jpeg => "jpeg",
                ImageFormat::Png => "png",
                ImageFormat::Webp => "webp",
                ImageFormat::Avif => "avif",
            }
            .into(),
        );
    }
    let name = src.rsplit('/').next().unwrap_or(src);
    let ext = name.rsplit('.').next()?.to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => Some("jpeg".into()),
        "png" => Some("png".into()),
        "webp" => Some("webp".into()),
        "gif" => Some("webp".into()),
        "avif" => Some("avif".into()),
        _ => None,
    }
}

fn source_display_size(src: &str, images_dir: &Path, rotate: bool) -> Option<(u32, u32)> {
    let path = Path::new(src);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        images_dir.join(src)
    };
    let bytes = std::fs::read(path).ok()?;
    let (_, w, h) = pipeline::probe_display_with(&bytes, rotate).ok()?;
    Some((w as u32, h as u32))
}

/// Matrix 200 cells must be images that probe as the labeled type.
/// Unlike `image_200_proved`, a text 200 (e.g. `/health`) is not a pass.
fn matrix_200_proved(res: &HttpResult, report: &Value) -> bool {
    if res.status != 200 {
        return false;
    }
    let ct = res
        .headers
        .get("content-type")
        .map(String::as_str)
        .unwrap_or("");
    let header = media_type_essence(ct);
    if !header.starts_with("image/") {
        return false;
    }
    match report.get("probe") {
        Some(p) => {
            p.get("error").is_none()
                && p.get("width").is_some()
                && p.get("content_type")
                    .and_then(|v| v.as_str())
                    .is_some_and(|pct| media_type_essence(pct).eq_ignore_ascii_case(header))
        }
        None => false,
    }
}

/// Keep in lockstep with `pipeline::fit_dims`: proportional shrink,
/// never enlarge, round, at least 1px. `0` on an axis is unconstrained
/// (the HTTP grammar).
fn fit_box(src_w: u32, src_h: u32, box_w: u32, box_h: u32) -> (u32, u32) {
    let max_w = if box_w == 0 { u32::MAX } else { box_w };
    let max_h = if box_h == 0 { u32::MAX } else { box_h };
    let scale = f64::min(
        max_w as f64 / src_w as f64,
        f64::min(max_h as f64 / src_h as f64, 1.0),
    );
    (
        ((src_w as f64 * scale).round() as u32).max(1),
        ((src_h as f64 * scale).round() as u32).max(1),
    )
}

fn matrix_geometry_ok(
    report: &Value,
    box_w: u32,
    box_h: u32,
    expected: Option<(u32, u32)>,
) -> bool {
    let Some(w) = report
        .pointer("/probe/width")
        .and_then(Value::as_u64)
        .map(|n| n as u32)
    else {
        return false;
    };
    let Some(h) = report
        .pointer("/probe/height")
        .and_then(Value::as_u64)
        .map(|n| n as u32)
    else {
        return false;
    };
    if box_w > 0 && w > box_w {
        return false;
    }
    if box_h > 0 && h > box_h {
        return false;
    }
    match expected {
        Some((ew, eh)) => (w, h) == (ew, eh),
        None => true,
    }
}
