//! Percent-encoding for URL paths, matching the server: `sign` HMACs the
//! decoded path its `Path` extractor sees, and `matrix` builds request
//! paths from fixture names that may hold `%` or spaces.

use crate::error::CtlError;

/// Percent-decode a URL path the way the server's `Path` extractor does
/// before HMAC verify. `+` is a literal (this is a path, not a query).
pub fn percent_decode_path(path: &str) -> Result<String, CtlError> {
    let b = path.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            if i + 2 >= b.len() {
                return Err(CtlError::usage(format!(
                    "malformed percent-escape in {path:?}"
                )));
            }
            let hi = hex_digit(b[i + 1])
                .ok_or_else(|| CtlError::usage(format!("malformed percent-escape in {path:?}")))?;
            let lo = hex_digit(b[i + 2])
                .ok_or_else(|| CtlError::usage(format!("malformed percent-escape in {path:?}")))?;
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out)
        .map_err(|_| CtlError::usage(format!("percent-decoded path is not valid UTF-8: {path:?}")))
}

fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Encode a decoded path so it can be put back in a URL. HMAC still
/// covers the decoded form; `%` in a filename must round-trip as `%25`.
/// `/` stays a separator. Other bytes follow RFC 3986 `pchar`.
pub fn percent_encode_path(path: &str) -> String {
    path.split('/')
        .map(percent_encode_segment)
        .collect::<Vec<_>>()
        .join("/")
}

fn percent_encode_segment(seg: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::new();
    for &b in seg.as_bytes() {
        if is_pchar(b) {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0xf) as usize] as char);
        }
    }
    out
}

fn is_pchar(b: u8) -> bool {
    matches!(
        b,
        b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'.'
            | b'_'
            | b'~'
            | b'!'
            | b'$'
            | b'&'
            | b'\''
            | b'('
            | b')'
            | b'*'
            | b'+'
            | b','
            | b';'
            | b'='
            | b':'
            | b'@'
    )
}
