//! `get`: one HTTP GET against a spawned server (or `--base`), reported
//! with its status, headers, hash, and probe.

use crate::args::{GetArgs, Opts};
use crate::error::{CtlError, emit};
use crate::http::{Http, get_report, image_200_proved, normalize_path};
use crate::spawn::server_base;
use serde_json::json;
use std::time::Duration;

pub fn run(opts: &Opts, a: &GetArgs) -> Result<(), CtlError> {
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
