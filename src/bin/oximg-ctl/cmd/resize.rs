//! `resize`: shell out to `oximg resize` with the argv a user types,
//! then probe what it wrote.

use crate::args::{Opts, ResizeArgs};
use crate::error::{CtlError, emit};
use crate::report::{probe_value, sha256_hex};
use crate::spawn::resolve_bin;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

pub fn run(opts: &Opts, a: &ResizeArgs) -> Result<(), CtlError> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv_is_what_a_user_types() {
        let argv = resize_argv(
            Path::new("in.jpg"),
            "100",
            "0",
            Path::new("o.jpg"),
            None,
            None,
            None,
        );
        assert_eq!(argv, ["oximg", "resize", "in.jpg", "100", "0", "o.jpg"]);
    }

    #[test]
    fn options_follow_in_the_clis_spelling() {
        let argv = resize_argv(
            Path::new("a b.gif"),
            "64",
            "64",
            Path::new("/tmp/o.webp"),
            Some("webp"),
            Some("70"),
            Some("fast"),
        );
        assert_eq!(
            argv,
            [
                "oximg",
                "resize",
                "a b.gif",
                "64",
                "64",
                "/tmp/o.webp",
                "-f",
                "webp",
                "-q",
                "70",
                "--preset",
                "fast"
            ]
        );
    }
}
