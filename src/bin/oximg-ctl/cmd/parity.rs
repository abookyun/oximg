//! `parity`: the same requests against two builds, compared byte for
//! byte. The instrument behind "the agent profile matches release".
//!
//! Each build gets its own spawned server in turn (never both at once:
//! one signal-forwarding slot, one port each), over the same images dir
//! and env. Only a 200 on both sides compares encoded output; the same
//! error on both is skipped, not counted as agreement.

use crate::args::{MatrixArgs, Opts, ParityArgs};
use crate::cmd::matrix;
use crate::error::{CtlError, emit};
use crate::http::Http;
use crate::report::sha256_hex;
use crate::spawn::{images_dir, interrupted, resolve_bin, server_base};
use oximg::pipeline;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;

const DEFAULT_BOXES: [(u32, u32); 3] = [(100, 100), (333, 0), (64, 64)];
/// Enough examples to act on without burying the counts.
const LIST_CAP: usize = 20;

pub fn run(opts: &Opts, a: &ParityArgs) -> Result<(), CtlError> {
    let images = images_dir(opts);
    let sources = if a.sources.is_empty() {
        image_fixtures(&images)?
    } else {
        a.sources.clone()
    };
    let boxes = if a.boxes.is_empty() {
        DEFAULT_BOXES.to_vec()
    } else {
        a.boxes.clone()
    };
    let plan = MatrixArgs {
        sources,
        boxes,
        formats: a.formats.clone(),
        negatives: false,
        base: None,
        timeout_secs: a.timeout_secs,
    };
    let paths: Vec<String> = matrix::plan(opts, &plan)
        .into_iter()
        .map(|c| c.path)
        .collect();
    let bin_a = resolve_bin(opts.bin.as_deref())?;
    let bin_b = a.against.clone();
    if opts.dry_run {
        emit(
            &json!({
                "ok": true,
                "dry_run": true,
                "a": bin_a.display().to_string(),
                "b": bin_b.display().to_string(),
                "cells": paths,
            }),
            opts.pretty,
        );
        return Ok(());
    }
    let http = Http::new().map_err(|e| CtlError::fail(e, None))?;
    let timeout = Duration::from_secs(a.timeout_secs);
    let side_a = responses(opts, &bin_a, &paths, &http, timeout)?;
    let side_b = responses(opts, &bin_b, &paths, &http, timeout)?;
    let mut report = compare(&paths, &side_a, &side_b);
    report["a"] = json!(bin_a.display().to_string());
    report["b"] = json!(bin_b.display().to_string());
    if report["ok"] == true {
        emit(&report, opts.pretty);
        Ok(())
    } else {
        Err(CtlError::from_value(1, report))
    }
}

/// What one build answered for one path.
#[derive(Debug, Clone, PartialEq)]
enum Outcome {
    Response {
        status: u16,
        sha256: String,
    },
    /// No HTTP response at all (refused, timed out). Its text names the
    /// build's own port, so it never equals the other side's.
    Transport(String),
}

impl Outcome {
    fn to_json(&self) -> Value {
        match self {
            Outcome::Response { status, sha256 } => json!({"status": status, "sha256": sha256}),
            Outcome::Transport(e) => json!({"error": e}),
        }
    }
}

/// Spawn `bin` over the same images and env, request every path, and
/// reap the server before returning.
fn responses(
    opts: &Opts,
    bin: &Path,
    paths: &[String],
    http: &Http,
    timeout: Duration,
) -> Result<Vec<Outcome>, CtlError> {
    let mut side = opts.clone();
    side.bin = Some(bin.to_path_buf());
    let (_spawned, base) = server_base(&side, None, http)?;
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        out.push(match http.get(&format!("{base}{p}"), None, timeout) {
            Ok(res) => Outcome::Response {
                status: res.status,
                sha256: sha256_hex(&res.body),
            },
            Err(e) => Outcome::Transport(e),
        });
        // Ctrl-C reaches the server, not us: every later request would
        // fail and read as a difference. Stop before the guard drops,
        // which clears the flag.
        if interrupted() {
            return Err(CtlError::fail(
                format!(
                    "interrupted while driving {}; nothing was compared",
                    bin.display()
                ),
                None,
            ));
        }
    }
    Ok(out)
}

/// The verdict: ok only when no cell differs and at least one compared.
fn compare(paths: &[String], a: &[Outcome], b: &[Outcome]) -> Value {
    let (mut compared, mut skipped, mut differing) = (0usize, Vec::new(), Vec::new());
    for ((path, oa), ob) in paths.iter().zip(a).zip(b) {
        match (oa, ob) {
            (Outcome::Response { status: 200, .. }, _) if oa == ob => compared += 1,
            (Outcome::Response { .. }, _) if oa == ob => skipped.push(path.clone()),
            _ => differing.push(json!({"path": path, "a": oa.to_json(), "b": ob.to_json()})),
        }
    }
    let ok = differing.is_empty() && compared > 0;
    let mut report = json!({
        "ok": ok,
        "cells": paths.len(),
        "compared": compared,
        "skipped_same_error": skipped.len(),
        "differ": differing.len(),
        "differing": differing.iter().take(LIST_CAP).collect::<Vec<_>>(),
        "skipped": skipped.iter().take(LIST_CAP).collect::<Vec<_>>(),
    });
    if compared == 0 && differing.is_empty() {
        report["error"] = json!("no cell answered 200 on both builds; nothing was compared");
    } else if !differing.is_empty() {
        report["error"] = json!(format!(
            "{} of {} cells differ",
            differing.len(),
            paths.len()
        ));
    }
    report
}

/// Every file in the images dir whose header probes as an image, sorted.
fn image_fixtures(dir: &Path) -> Result<Vec<String>, CtlError> {
    let entries = std::fs::read_dir(dir).map_err(|e| {
        CtlError::fail(
            format!("read images dir {}: {e}", dir.display()),
            Some("pass --images-dir or run from the oximg checkout"),
        )
    })?;
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && probes(p))
        .filter_map(|p| p.file_name()?.to_str().map(str::to_string))
        .collect();
    names.sort();
    Ok(names)
}

fn probes(path: &Path) -> bool {
    std::fs::read(path).is_ok_and(|bytes| pipeline::probe(&bytes).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(status: u16, sha: &str) -> Outcome {
        Outcome::Response {
            status,
            sha256: sha.into(),
        }
    }

    fn paths(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("/p{i}")).collect()
    }

    #[test]
    fn equal_200s_compare_and_pass() {
        let r = compare(
            &paths(2),
            &[ok(200, "x"), ok(200, "y")],
            &[ok(200, "x"), ok(200, "y")],
        );
        assert_eq!(r["ok"], true);
        assert_eq!(
            (r["compared"].as_u64(), r["differ"].as_u64()),
            (Some(2), Some(0))
        );
        assert!(r.get("error").is_none(), "{r}");
    }

    #[test]
    fn the_same_error_on_both_sides_is_skipped_not_agreement() {
        let r = compare(
            &paths(2),
            &[ok(200, "x"), ok(422, "e")],
            &[ok(200, "x"), ok(422, "e")],
        );
        assert_eq!(r["ok"], true);
        assert_eq!(r["compared"], 1);
        assert_eq!(r["skipped_same_error"], 1);
        assert_eq!(r["skipped"][0], "/p1");
    }

    #[test]
    fn nothing_compared_is_a_failure() {
        let r = compare(&paths(1), &[ok(422, "e")], &[ok(422, "e")]);
        assert_eq!(r["ok"], false);
        assert!(
            r["error"]
                .as_str()
                .unwrap()
                .contains("nothing was compared")
        );
    }

    #[test]
    fn different_bytes_status_or_transport_all_differ() {
        let a = [
            ok(200, "x"),
            ok(200, "x"),
            Outcome::Transport("refused :1".into()),
        ];
        let b = [
            ok(200, "z"),
            ok(404, "x"),
            Outcome::Transport("refused :2".into()),
        ];
        let r = compare(&paths(3), &a, &b);
        assert_eq!(r["ok"], false);
        assert_eq!(r["differ"], 3);
        assert_eq!(r["differing"][0]["a"]["sha256"], "x");
        assert_eq!(r["differing"][0]["b"]["sha256"], "z");
        assert_eq!(r["differing"][2]["a"]["error"], "refused :1");
        assert!(r["error"].as_str().unwrap().contains("3 of 3 cells differ"));
    }

    #[test]
    fn listed_examples_are_capped_but_counts_are_not() {
        let n = LIST_CAP + 5;
        let a: Vec<_> = (0..n).map(|_| ok(200, "a")).collect();
        let b: Vec<_> = (0..n).map(|_| ok(200, "b")).collect();
        let r = compare(&paths(n), &a, &b);
        assert_eq!(r["differ"], n);
        assert_eq!(r["differing"].as_array().unwrap().len(), LIST_CAP);
    }

    #[test]
    fn default_sources_are_the_fixtures_that_probe() {
        let names = image_fixtures(&crate::spawn::images_dir(&Opts {
            bin: None,
            images_dir: None,
            env: vec![],
            pretty: false,
            dry_run: false,
        }))
        .unwrap();
        assert!(names.contains(&"photo.jpg".to_string()));
        assert!(names.contains(&"anim.gif".to_string()));
        for not_image in ["README.md", "list.txt", "regen.sh"] {
            assert!(!names.iter().any(|n| n == not_image), "{not_image} listed");
        }
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn a_missing_images_dir_is_a_failure_with_a_hint() {
        let e = image_fixtures(Path::new("/no/such/dir")).expect_err("should fail");
        assert!(e.json["hint"].as_str().unwrap().contains("--images-dir"));
    }
}
