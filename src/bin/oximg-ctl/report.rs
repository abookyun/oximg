//! The fields every command reports about bytes it saw: a header probe
//! (format, stored size, animation) and a sha256.

use oximg::pipeline::{self, Animation};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub fn probe_value(bytes: &[u8]) -> Value {
    match pipeline::probe(bytes) {
        Ok((fmt, w, h)) => {
            let mut v = json!({
                "content_type": fmt.content_type(),
                "width": w,
                "height": h,
                "stored_pixels": w as u64 * h as u64,
            });
            match pipeline::probe_animation(bytes) {
                Ok(Some(Animation {
                    frames,
                    duration_ms,
                    loop_count,
                })) => {
                    v["animation"] = json!({
                        "frames": frames,
                        "duration_ms": duration_ms,
                        "loop_count": loop_count,
                    });
                }
                Ok(None) => {}
                Err(e) => {
                    v["error"] = json!(e.to_string());
                    v["animation_error"] = json!(e.to_string());
                }
            }
            v
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(H[(b >> 4) as usize] as char);
        s.push(H[(b & 0xf) as usize] as char);
    }
    s
}
