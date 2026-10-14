//! `probe`: the in-process header probe of a file on disk.

use crate::args::{Opts, ProbeArgs};
use crate::error::{CtlError, emit};
use crate::report::{probe_value, sha256_hex};
use serde_json::json;

pub fn run(opts: &Opts, a: &ProbeArgs) -> Result<(), CtlError> {
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
