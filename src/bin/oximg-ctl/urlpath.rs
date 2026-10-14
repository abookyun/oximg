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

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_err(path: &str) -> String {
        percent_decode_path(path).expect_err("should fail").json["error"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn decode_turns_escapes_back_into_bytes() {
        assert_eq!(percent_decode_path("/a%2Fb%2fc").unwrap(), "/a/b/c");
        assert_eq!(percent_decode_path("/%C3%A9").unwrap(), "/é");
        // A path, not a query: `+` is a plus sign.
        assert_eq!(percent_decode_path("/a+b").unwrap(), "/a+b");
    }

    #[test]
    fn decode_rejects_truncated_and_non_hex_escapes() {
        assert!(decode_err("/a%2").contains("malformed percent-escape"));
        assert!(decode_err("/a%").contains("malformed percent-escape"));
        assert!(decode_err("/a%zz").contains("malformed percent-escape"));
    }

    #[test]
    fn decode_rejects_bytes_that_are_not_utf8() {
        assert!(decode_err("/%FF").contains("not valid UTF-8"));
    }

    #[test]
    fn encode_keeps_separators_and_pchars() {
        assert_eq!(
            percent_encode_path("/resize/1/1/a-b_c.~!$&'()*+,;=:@.jpg"),
            "/resize/1/1/a-b_c.~!$&'()*+,;=:@.jpg"
        );
    }

    #[test]
    fn encode_escapes_percent_space_and_non_ascii() {
        assert_eq!(percent_encode_path("/a b%c/é"), "/a%20b%25c/%C3%A9");
    }

    #[test]
    fn decode_undoes_encode() {
        for p in ["/a b/%25/é?#", "/albums/2026/photo.jpg", "/x%y"] {
            assert_eq!(percent_decode_path(&percent_encode_path(p)).unwrap(), p);
        }
    }
}
