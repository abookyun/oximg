//! The HTTP side of a proof: a blocking GET against the spawned server
//! (or `--base`), the JSON report of one response, and the rules for
//! when a 200 counts as an image.

use crate::error::CtlError;
use crate::report::{probe_value, sha256_hex};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

pub struct HttpResult {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub ms: f64,
    pub url: String,
}

pub struct Http {
    rt: tokio::runtime::Runtime,
    client: reqwest::Client,
}

impl Http {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            rt: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| format!("tokio runtime: {e}"))?,
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|e| format!("http client: {e}"))?,
        })
    }

    pub fn get(
        &self,
        url: &str,
        accept: Option<&str>,
        timeout: Duration,
    ) -> Result<HttpResult, String> {
        let t0 = Instant::now();
        self.rt.block_on(async {
            let mut req = self.client.get(url).timeout(timeout);
            if let Some(a) = accept {
                req = req.header("accept", a);
            }
            let resp = req.send().await.map_err(|e| format!("GET {url}: {e}"))?;
            let status = resp.status().as_u16();
            let mut headers = HashMap::new();
            for (k, v) in resp.headers() {
                if let Ok(val) = v.to_str() {
                    headers.insert(k.as_str().to_ascii_lowercase(), val.to_string());
                }
            }
            let body = resp
                .bytes()
                .await
                .map_err(|e| format!("read body: {e}"))?
                .to_vec();
            Ok(HttpResult {
                status,
                headers,
                body,
                ms: t0.elapsed().as_secs_f64() * 1e3,
                url: url.to_string(),
            })
        })
    }
}

pub fn normalize_path(path: &str) -> String {
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

pub fn get_report(res: &HttpResult, write: Option<&Path>) -> Result<Value, CtlError> {
    if let Some(p) = write {
        std::fs::write(p, &res.body)
            .map_err(|e| CtlError::fail(format!("write {}: {e}", p.display()), None))?;
    }
    let ct = res.headers.get("content-type").cloned().unwrap_or_default();
    let mut v = json!({
        "ok": true,
        "status": res.status,
        "url": res.url,
        "bytes": res.body.len(),
        "sha256": sha256_hex(&res.body),
        "ms": (res.ms * 10.0).round() / 10.0,
        "content_type": ct,
        "headers": {
            "content-type": res.headers.get("content-type"),
            "vary": res.headers.get("vary"),
            "cache-control": res.headers.get("cache-control"),
            "allow": res.headers.get("allow"),
        },
    });
    if let Some(p) = write {
        v["body_path"] = json!(p.display().to_string());
    }
    if res.status == 200 && ct.starts_with("image/") {
        v["probe"] = probe_value(&res.body);
    } else if !res.body.is_empty() && !ct.starts_with("image/") {
        let text = String::from_utf8_lossy(&res.body);
        let clipped: String = text.chars().take(512).collect();
        v["body_text"] = json!(clipped);
    }
    Ok(v)
}

/// A 200 image response is proved only when the body probes. Non-image
/// 200s (e.g. `/health`) and non-200s are observations, not image proofs.
pub fn image_200_proved(res: &HttpResult, report: &Value) -> bool {
    if res.status != 200 {
        return true;
    }
    let ct = res
        .headers
        .get("content-type")
        .map(String::as_str)
        .unwrap_or("");
    if !ct.starts_with("image/") {
        return true;
    }
    match report.get("probe") {
        Some(p) => p.get("error").is_none() && p.get("width").is_some(),
        None => false,
    }
}

pub fn media_type_essence(ct: &str) -> &str {
    ct.split(';').next().unwrap_or(ct).trim()
}

pub fn content_type_matches_token(ct: &str, token: &str) -> bool {
    let want = match token {
        "webp" => "image/webp",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "avif" => "image/avif",
        _ => return false,
    };
    media_type_essence(ct).eq_ignore_ascii_case(want)
}
