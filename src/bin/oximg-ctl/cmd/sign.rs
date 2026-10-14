//! `sign`: the imgproxy-style URL signature the server verifies.

use crate::args::{Opts, SignArgs};
use crate::error::{CtlError, emit};
use crate::http::normalize_path;
use crate::urlpath::{percent_decode_path, percent_encode_path};
use hmac::Mac;
use hmac::digest::KeyInit;
use serde_json::json;

pub fn run(opts: &Opts, a: &SignArgs) -> Result<(), CtlError> {
    let key = a.key.as_deref();
    let salt = a.salt.as_deref();
    let path = percent_decode_path(&normalize_path(&a.path))?;
    let key_hex = key
        .map(str::to_string)
        .or_else(|| std::env::var("OXIMG_KEY").ok())
        .ok_or_else(|| CtlError::usage("sign needs --key HEX or OXIMG_KEY"))?;
    let salt_hex = salt
        .map(str::to_string)
        .or_else(|| std::env::var("OXIMG_SALT").ok())
        .ok_or_else(|| CtlError::usage("sign needs --salt HEX or OXIMG_SALT"))?;
    if opts.dry_run {
        emit(
            &json!({
                "ok": true,
                "dry_run": true,
                "path": path,
            }),
            opts.pretty,
        );
        return Ok(());
    }
    let key = decode_hex("key", &key_hex)?;
    let salt = decode_hex("salt", &salt_hex)?;
    let signature = sign_path(&key, &salt, &path)?;
    emit(
        &json!({
            "ok": true,
            "path": path,
            "signature": signature,
            "url": format!("/{signature}{}", percent_encode_path(&path)),
        }),
        opts.pretty,
    );
    Ok(())
}

fn decode_hex(name: &str, v: &str) -> Result<Vec<u8>, CtlError> {
    let v = v.trim();
    if v.is_empty() {
        return Err(CtlError::usage(format!(
            "{name} must not be empty (unset OXIMG_KEY/OXIMG_SALT means signing off)"
        )));
    }
    let raw = v.as_bytes();
    if !raw.len().is_multiple_of(2) {
        return Err(CtlError::usage(format!(
            "{name} is not valid hex (odd length)"
        )));
    }
    if !raw.iter().all(u8::is_ascii_hexdigit) {
        return Err(CtlError::usage(format!("{name} is not valid hex")));
    }
    raw.chunks(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).expect("ascii hex"), 16)
                .map_err(|_| CtlError::usage(format!("{name} is not valid hex")))
        })
        .collect()
}

/// The scheme `Signing::verify` in src/main.rs accepts: unpadded
/// base64url(HMAC-SHA256(key, salt || path)) over the percent-decoded
/// path. Vectors live in tests/server.rs and the Rails gem.
fn sign_path(key: &[u8], salt: &[u8], path: &str) -> Result<String, CtlError> {
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(key)
        .map_err(|_| CtlError::fail("HMAC key was empty", None))?;
    mac.update(salt);
    mac.update(path.as_bytes());
    Ok(base64url_nopad(&mac.finalize().into_bytes()))
}

fn base64url_nopad(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= data.len() {
        let n = u32::from_be_bytes([0, data[i], data[i + 1], data[i + 2]]);
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(T[((n >> 6) & 63) as usize] as char);
        out.push(T[(n & 63) as usize] as char);
        i += 3;
    }
    match data.len() - i {
        1 => {
            let n = (data[i] as u32) << 16;
            out.push(T[((n >> 18) & 63) as usize] as char);
            out.push(T[((n >> 12) & 63) as usize] as char);
        }
        2 => {
            let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8);
            out.push(T[((n >> 18) & 63) as usize] as char);
            out.push(T[((n >> 12) & 63) as usize] as char);
            out.push(T[((n >> 6) & 63) as usize] as char);
        }
        _ => {}
    }
    out
}
