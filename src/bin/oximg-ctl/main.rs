//! Control plane for the `oximg` binary: spawn the real server, drive
//! HTTP/CLI/library surfaces, and print one JSON object per command.
//!
//! This is a lever for agents (and operators), not a second user-facing
//! product. It never reimplements the pipeline — every HTTP cell hits a
//! spawned `oximg` process, `resize` shells out to `oximg resize`, and
//! `probe`/`sign` use the same library/HMAC scheme the server does.
//!
//!   oximg-ctl get /resize/100/100/photo.jpg
//!   oximg-ctl probe tests/fixtures/photo.jpg
//!   oximg-ctl matrix
//!
//! Exit 0 on success, 1 when a command ran and failed its proof, 2 for
//! usage errors. stdout is always one JSON value (except `--help` and
//! `--version`).

use oximg::pipeline::{self, ImageFormat};
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

mod args;
mod cmd;
mod error;
mod http;
mod report;
mod spawn;
mod urlpath;

use args::{Cmd, GetArgs, MatrixArgs, Opts, ProbeArgs, ResizeArgs, ServeArgs, parse_args};
use error::{CtlError, emit};
use http::{
    Http, HttpResult, content_type_matches_token, get_report, image_200_proved, media_type_essence,
    normalize_path,
};
use report::{probe_value, sha256_hex};
use spawn::{
    child_auto_rotate, images_dir, remote_source_base, resolve_bin, server_base, spawn_server,
    wait_served_child,
};
use urlpath::percent_encode_path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        return;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("oximg-ctl {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let pretty = args.iter().any(|a| a == "--pretty");
    let result = parse_args(&args).and_then(|(opts, cmd)| match &cmd {
        Cmd::Serve(a) => cmd_serve(&opts, a),
        Cmd::Get(a) => cmd_get(&opts, a),
        Cmd::Probe(a) => cmd_probe(&opts, a),
        Cmd::Resize(a) => cmd_resize(&opts, a),
        Cmd::Sign(a) => cmd::sign::run(&opts, a),
        Cmd::Matrix(a) => cmd_matrix(&opts, a),
    });
    if let Err(e) = result {
        e.with_pretty(pretty).exit();
    }
}

fn print_help() {
    print!(
        "\
oximg-ctl {}
JSON control plane for the oximg binary (server, CLI, and pipeline).

Usage:
  oximg-ctl [global] <command> [args]

Commands:
  serve                     Spawn oximg, print {{port,pid,base}}, wait
  get <path>                HTTP GET; auto-spawns unless --base is set
  probe <file>              Header probe (format, stored size, animation)
  resize <in> <w> <h>       Shell out to `oximg resize`; probe the output
  sign <path>               imgproxy-style HMAC over the decoded path
  matrix                    Walk fixture × box × format cells + negatives

Global:
  --bin PATH                oximg binary (else sibling, OXIMG_BIN, or PATH)
  --images-dir DIR          IMAGES_DIR for a spawned server
                            (default: this crate's tests/fixtures)
  --env KEY=VAL             Extra env for a spawned server (repeatable;
                            PORT and IMAGES_DIR are reserved)
  --pretty                  Indent JSON
  --dry-run                 Print the plan; do not spawn or write
  --help, --version

serve:
  --port N                  Listen port (default: OS-assigned 0)

get:
  --base URL                Use an already-running server (no spawn)
  --accept VALUE            Accept request header
  --write PATH              Save the response body
  --expect N                Fail unless the status is N
  --timeout-secs N          Request timeout (default 60)

resize:
  --out PATH                Output file (default: a temp file)
  -f, --format FMT          {}
  -q, --quality N           JPEG quality 1-100
  --preset P                jpegli | fast | small

sign:
  --key HEX                 OXIMG_KEY (else the env)
  --salt HEX                OXIMG_SALT (else the env)

matrix:
  --source FILE             Fixture filename (repeatable; default: a
                            small committed set under tests/fixtures)
  --box WxH                 e.g. 100x100 or 750x0 (repeatable)
  --format TOKEN            source | {}
                            (repeatable; default: source and webp)
  --no-negatives            Skip the 400/404 cells
  --base URL                Drive an existing server

Spawned servers use PORT=0 and, unless already set, OXIMG_WORKERS=1
so a control-plane loop does not size itself to the host. stderr from
oximg is forwarded. stdout is one JSON object (`--help` and `--version`
are plain text). `serve` prints the ready record then waits; a later
child failure is a stderr line and exit 1, not a second JSON object.

Examples:
  oximg-ctl get /resize/100/100/photo.jpg
  oximg-ctl get /resize/100/100/photo.jpg@webp --write /tmp/out.webp
  oximg-ctl --env OXIMG_OPTIONS_PREFIX=/image get /image/width=100/photo.jpg
  oximg-ctl sign /resize/100/100/photo.jpg --key deadbeef... --salt cafebabe...
  oximg-ctl matrix --box 100x100 --source photo.jpg
",
        env!("CARGO_PKG_VERSION"),
        ImageFormat::output_token_hint().replace('|', " | "),
        ImageFormat::output_token_hint().replace('|', " | "),
    );
}

// ---------------------------------------------------------------------------
// serve / spawn
// ---------------------------------------------------------------------------

fn cmd_serve(opts: &Opts, a: &ServeArgs) -> Result<(), CtlError> {
    let port = a.port;
    if opts.dry_run {
        emit(
            &json!({
                "ok": true,
                "dry_run": true,
                "bin": resolve_bin(opts.bin.as_deref())?.display().to_string(),
                "images_dir": images_dir(opts).display().to_string(),
                "port": port.unwrap_or(0),
                "env": opts
                    .env
                    .iter()
                    .map(|(k, v)| json!({ (k): redact_env_value(k, v) }))
                    .collect::<Vec<_>>(),
            }),
            opts.pretty,
        );
        return Ok(());
    }
    let http = Http::new().map_err(|e| CtlError::fail(e, None))?;
    let mut spawned = spawn_server(opts, port, false, &http)?;
    emit(
        &json!({
            "ok": true,
            "pid": spawned.child.id(),
            "port": spawned.port,
            "base": spawned.base(),
            "bin": spawned.bin.display().to_string(),
            "images_dir": spawned.images_dir.display().to_string(),
        }),
        opts.pretty,
    );
    let _ = std::io::stdout().flush();
    // Ready record is already on stdout. A later child failure must not
    // emit a second JSON object. Keep kill_on_drop until the child is
    // reaped so SIGINT/SIGTERM cannot leave an orphan listener.
    match wait_served_child(&mut spawned) {
        Ok(status) if status.success() => {
            spawned.kill_on_drop = false;
            Ok(())
        }
        Ok(status) => {
            spawned.kill_on_drop = false;
            eprintln!("oximg-ctl: server exited {status}");
            std::process::exit(1);
        }
        Err(e) => {
            let _ = spawned.child.kill();
            let _ = spawned.child.wait();
            spawned.kill_on_drop = false;
            eprintln!("oximg-ctl: wait on server: {e}");
            std::process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

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

fn redact_env_value<'a>(key: &str, value: &'a str) -> &'a str {
    let u = key.to_ascii_uppercase();
    if ["KEY", "SALT", "SECRET", "TOKEN", "PASSWORD"]
        .iter()
        .any(|needle| u.contains(needle))
    {
        "<redacted>"
    } else {
        value
    }
}

fn cmd_get(opts: &Opts, a: &GetArgs) -> Result<(), CtlError> {
    let base = a.base.as_deref();
    let write = a.write.as_deref();
    let accept = a.accept.as_deref();
    let expect = a.expect;
    let timeout_secs = a.timeout_secs;
    let path = normalize_path(&a.path);
    if opts.dry_run {
        emit(
            &json!({
                "ok": true,
                "dry_run": true,
                "path": path,
                "base": base,
                "accept": accept,
                "expect": expect,
            }),
            opts.pretty,
        );
        return Ok(());
    }
    let http = Http::new().map_err(|e| CtlError::fail(e, None))?;
    let (_spawned, base) = server_base(opts, base, &http)?;
    let url = format!("{base}{path}");
    let res = http
        .get(&url, accept, Duration::from_secs(timeout_secs))
        .map_err(|e| {
            CtlError::fail(
                e,
                Some("is the server up? pass --base or let get auto-spawn"),
            )
        })?;
    let mut report = get_report(&res, write)?;
    if let Some(want) = expect
        && res.status != want
    {
        report["ok"] = json!(false);
        report["error"] = json!(format!("expected status {want}, got {}", res.status));
        return Err(CtlError::from_value(1, report));
    }
    if !image_200_proved(&res, &report) {
        report["ok"] = json!(false);
        report["error"] = json!("response body did not probe as an image");
        return Err(CtlError::from_value(1, report));
    }
    emit(&report, opts.pretty);
    Ok(())
}

// ---------------------------------------------------------------------------
// probe / resize / sign
// ---------------------------------------------------------------------------

fn cmd_probe(opts: &Opts, a: &ProbeArgs) -> Result<(), CtlError> {
    let file = a.file.as_path();
    if opts.dry_run {
        emit(
            &json!({"ok": true, "dry_run": true, "file": file.display().to_string()}),
            opts.pretty,
        );
        return Ok(());
    }
    let bytes = std::fs::read(file).map_err(|e| {
        CtlError::fail(
            format!("read {}: {e}", file.display()),
            Some("path is relative to the current working directory"),
        )
    })?;
    let mut probe = probe_value(&bytes);
    if probe.get("error").is_some() {
        let mut v = json!({
            "ok": false,
            "file": file.display().to_string(),
            "bytes": bytes.len(),
            "sha256": sha256_hex(&bytes),
            "probe": probe,
        });
        v["error"] = probe["error"].take();
        return Err(CtlError::from_value(1, v));
    }
    emit(
        &json!({
            "ok": true,
            "file": file.display().to_string(),
            "bytes": bytes.len(),
            "sha256": sha256_hex(&bytes),
            "probe": probe,
        }),
        opts.pretty,
    );
    Ok(())
}

fn cmd_resize(opts: &Opts, a: &ResizeArgs) -> Result<(), CtlError> {
    let ResizeArgs {
        input,
        w,
        h,
        out,
        format,
        quality,
        preset,
    } = a;
    let format = format.as_deref();
    let quality = quality.as_deref();
    let preset = preset.as_deref();
    let bin = resolve_bin(opts.bin.as_deref())?;
    let ephemeral = out.is_none();
    let tmp;
    let out_path: &Path = match out {
        Some(p) => p,
        None => {
            tmp = std::env::temp_dir().join(format!(
                "oximg-ctl-{}-{}-{}x{}.out",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0),
                w,
                h
            ));
            &tmp
        }
    };
    struct RemoveOnDrop(Option<PathBuf>);
    impl Drop for RemoveOnDrop {
        fn drop(&mut self) {
            if let Some(p) = self.0.take() {
                let _ = std::fs::remove_file(p);
            }
        }
    }
    if opts.dry_run {
        emit(
            &json!({
                "ok": true,
                "dry_run": true,
                "bin": bin.display().to_string(),
                "argv": resize_argv(input, w, h, out_path, format, quality, preset),
            }),
            opts.pretty,
        );
        return Ok(());
    }
    let _ephemeral = RemoveOnDrop(ephemeral.then(|| out_path.to_path_buf()));
    let argv = resize_argv(input, w, h, out_path, format, quality, preset);
    let t0 = Instant::now();
    let mut cmd = Command::new(&bin);
    cmd.args(&argv[1..]);
    for (k, v) in &opts.env {
        cmd.env(k, v);
    }
    let output = cmd.output().map_err(|e| {
        CtlError::fail(
            format!("run {}: {e}", bin.display()),
            Some("pass --bin at the oximg executable"),
        )
    })?;
    let ms = t0.elapsed().as_secs_f64() * 1e3;
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
        let code = output.status.code().unwrap_or(1);
        let msg = format!("oximg resize exited {code}: {stderr}");
        return Err(if code == 2 {
            CtlError::usage(msg)
        } else {
            CtlError::fail(msg, None)
        });
    }
    let bytes = std::fs::read(out_path)
        .map_err(|e| CtlError::fail(format!("read {}: {e}", out_path.display()), None))?;
    let probe = probe_value(&bytes);
    let mut report = json!({
        "ok": true,
        "bytes": bytes.len(),
        "sha256": sha256_hex(&bytes),
        "ms": (ms * 10.0).round() / 10.0,
        "probe": probe,
        "stderr": stderr,
    });
    if !ephemeral {
        report["out"] = json!(out_path.display().to_string());
    }
    if report["probe"].get("error").is_some() {
        report["ok"] = json!(false);
        report["error"] = json!("output did not probe as an image");
        return Err(CtlError::from_value(1, report));
    }
    emit(&report, opts.pretty);
    Ok(())
}

fn resize_argv(
    input: &Path,
    w: &str,
    h: &str,
    out: &Path,
    format: Option<&str>,
    quality: Option<&str>,
    preset: Option<&str>,
) -> Vec<String> {
    let mut v = vec![
        "oximg".into(),
        "resize".into(),
        input.display().to_string(),
        w.into(),
        h.into(),
        out.display().to_string(),
    ];
    if let Some(f) = format {
        v.push("-f".into());
        v.push(f.into());
    }
    if let Some(q) = quality {
        v.push("-q".into());
        v.push(q.into());
    }
    if let Some(p) = preset {
        v.push("--preset".into());
        v.push(p.into());
    }
    v
}

// ---------------------------------------------------------------------------
// matrix
// ---------------------------------------------------------------------------

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

fn cmd_matrix(opts: &Opts, a: &MatrixArgs) -> Result<(), CtlError> {
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
