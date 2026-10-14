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

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!(
            "{}/tests/fixtures/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    #[test]
    fn probe_reports_type_and_stored_size() {
        let v = probe_value(&fixture("photo.jpg"));
        assert_eq!(v["content_type"], "image/jpeg");
        assert_eq!(
            (v["width"].as_u64(), v["height"].as_u64()),
            (Some(200), Some(150))
        );
        assert_eq!(v["stored_pixels"], 30_000);
        assert!(v.get("animation").is_none(), "{v}");
        assert!(v.get("error").is_none(), "{v}");
    }

    #[test]
    fn probe_reports_animation() {
        let v = probe_value(&fixture("anim.gif"));
        assert_eq!(v["animation"]["frames"], 3);
        assert_eq!(v["animation"]["duration_ms"], 1500);
    }

    #[test]
    fn probe_of_non_image_bytes_is_an_error_field() {
        let v = probe_value(b"not an image");
        assert!(v["error"].is_string(), "{v}");
        assert!(v.get("width").is_none(), "{v}");
    }

    #[test]
    fn sha256_is_lowercase_hex() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
