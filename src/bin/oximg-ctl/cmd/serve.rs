//! `serve`: spawn a held server, print its ready record, and wait.

use crate::args::{Opts, ServeArgs};
use crate::error::{CtlError, emit};
use crate::http::Http;
use crate::spawn::{images_dir, resolve_bin, spawn_server, wait_served_child};
use serde_json::json;
use std::io::Write;

pub fn run(opts: &Opts, a: &ServeArgs) -> Result<(), CtlError> {
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
