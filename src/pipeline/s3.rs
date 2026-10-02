//! `s3://` source fetching (issue #11): read objects from a private
//! S3 or S3-compatible bucket.
//!
//! This first part is the request signer: AWS Signature Version 4. It
//! uses `hmac` and `sha2`, which the `server` feature already has for
//! URL signing. It does not use an AWS SDK: we only sign one kind of
//! request, a GET with an empty body.
//!
//! S3 differs from the generic SigV4 rules in one place. The canonical
//! URI is the path encoded once, with no normalization. In S3, `.` and
//! `..` segments and repeated slashes are part of the key, so we sign
//! the path exactly as we send it.

use hmac::Mac;
use hmac::digest::KeyInit;
use sha2::{Digest, Sha256};

type HmacSha256 = hmac::Hmac<Sha256>;

/// SHA-256 of an empty body. Every request this module signs is a GET,
/// so this is always the payload hash.
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

struct Credentials {
    access_key_id: String,
    secret_access_key: String,
    /// Present for temporary keys. Signed as `x-amz-security-token`.
    session_token: Option<String>,
}

/// Percent-encode raw path bytes for a SigV4 canonical URI. Only RFC 3986
/// unreserved bytes stay as they are. Every other byte becomes an
/// upper-case `%XX`. This includes the sub-delims that
/// `encode_upstream_path` keeps for the HTTP mode. `/` stays as the
/// separator. We use the result as the request path and as the
/// canonical URI, so the two always match.
fn uri_encode_path(raw: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(raw.len());
    for &b in raw {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0xf) as usize] as char);
        }
    }
    out
}

/// The `x-amz-date` value (`YYYYMMDDTHHMMSSZ`) for a Unix time in
/// seconds. The first eight characters are the credential-scope date.
fn amz_datetime(unix_secs: u64) -> String {
    let days = (unix_secs / 86_400) as i64;
    let rem = unix_secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Convert days since 1970-01-01 to a Gregorian (year, month, day).
/// This is Howard Hinnant's `civil_from_days` algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

fn hmac(key: &[u8], data: &[u8]) -> [u8; 32] {
    // HMAC accepts a key of any length, so this cannot fail.
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC takes any key length");
    mac.update(data);
    mac.finalize().into_bytes().into()
}

fn signing_key(secret: &str, date: &str, region: &str, service: &str) -> [u8; 32] {
    let k = hmac(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let k = hmac(&k, region.as_bytes());
    let k = hmac(&k, service.as_bytes());
    hmac(&k, b"aws4_request")
}

/// One request to sign. `headers` lists the headers to sign. Their
/// names must be lower-case, and their order does not matter.
/// `canonical_uri` must be the same encoded path that we send.
struct Request<'a> {
    method: &'a str,
    canonical_uri: &'a str,
    canonical_query: &'a str,
    headers: &'a [(&'a str, &'a str)],
    payload_hash: &'a str,
}

/// Return the `Authorization` header value for `req`. `datetime` must
/// be the request's `x-amz-date` value, and `x-amz-date` must be one of
/// the signed headers.
fn authorization(
    req: &Request<'_>,
    creds: &Credentials,
    region: &str,
    service: &str,
    datetime: &str,
) -> String {
    let mut headers: Vec<(&str, String)> = req
        .headers
        .iter()
        .map(|(name, value)| (*name, canonical_header_value(value)))
        .collect();
    headers.sort_by(|a, b| a.0.cmp(b.0));
    let signed_headers = headers
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(";");
    let canonical_headers: String = headers
        .iter()
        .map(|(name, value)| format!("{name}:{value}\n"))
        .collect();
    let canonical_request = format!(
        "{}\n{}\n{}\n{canonical_headers}\n{signed_headers}\n{}",
        req.method, req.canonical_uri, req.canonical_query, req.payload_hash
    );
    let date = &datetime[..8];
    let scope = format!("{date}/{region}/{service}/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{datetime}\n{scope}\n{}",
        hex(&Sha256::digest(canonical_request.as_bytes()))
    );
    let key = signing_key(&creds.secret_access_key, date, region, service);
    let signature = hex(&hmac(&key, string_to_sign.as_bytes()));
    format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
        creds.access_key_id
    )
}

/// SigV4 trims a header value and folds each run of spaces into one.
fn canonical_header_value(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    // These vectors come from the AWS SigV4 test suite in aws-c-auth
    // (Apache-2.0), in tests/aws-signing-test-suite/v4/<name>/. They all
    // use the suite's example credentials, us-east-1, the service name
    // "service", and the time 2015-08-30T12:36:00Z.
    const KEY_ID: &str = "AKIDEXAMPLE";
    const SECRET: &str = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";
    const DATETIME: &str = "20150830T123600Z";
    const HOST: &str = "example.amazonaws.com";

    fn creds(token: Option<&str>) -> Credentials {
        Credentials {
            access_key_id: KEY_ID.into(),
            secret_access_key: SECRET.into(),
            session_token: token.map(Into::into),
        }
    }

    /// Sign a suite request. It is a GET of `raw_path` with no body. It
    /// signs `host` and `x-amz-date`, and the token when there is one.
    fn sign_suite(raw_path: &str, token: Option<&str>) -> String {
        let uri = uri_encode_path(raw_path.as_bytes());
        let mut headers = vec![("host", HOST), ("x-amz-date", DATETIME)];
        if let Some(t) = token {
            headers.push(("x-amz-security-token", t));
        }
        let req = Request {
            method: "GET",
            canonical_uri: &uri,
            canonical_query: "",
            headers: &headers,
            payload_hash: EMPTY_SHA256,
        };
        authorization(&req, &creds(token), "us-east-1", "service", DATETIME)
    }

    fn signature_of(auth: &str) -> &str {
        auth.rsplit_once("Signature=").unwrap().1
    }

    #[test]
    fn get_vanilla() {
        let auth = sign_suite("/", None);
        assert_eq!(
            auth,
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, \
             SignedHeaders=host;x-amz-date, \
             Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
        );
    }

    #[test]
    fn get_vanilla_with_session_token() {
        let token = "6e86291e8372ff2a2260956d9b8aae1d763fbf315fa00fa31553b73ebf194267";
        let auth = sign_suite("/", Some(token));
        assert!(auth.contains("SignedHeaders=host;x-amz-date;x-amz-security-token,"));
        assert_eq!(
            signature_of(&auth),
            "07ec1639c89043aa0e3e2de82b96708f198cceab042d4a97044c66dd9f74e7f8"
        );
    }

    #[test]
    fn get_unreserved() {
        let path = "/-._~0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
        assert_eq!(uri_encode_path(path.as_bytes()), path);
        assert_eq!(
            signature_of(&sign_suite(path, None)),
            "07ef7494c76fa4850883e2b006601f940f8a34d404d0cfa977f52a65bbf5f24f"
        );
    }

    #[test]
    fn get_utf8() {
        assert_eq!(uri_encode_path("/\u{1234}".as_bytes()), "/%E1%88%B4");
        assert_eq!(
            signature_of(&sign_suite("/\u{1234}", None)),
            "8318018e0b0f223aa2bbf98705b62bb787dc9c0e678f255a891fd03141be5d85"
        );
    }

    // The "unnormalized" vectors follow the S3 rule. The signer keeps
    // `//` and `..` in the path and does not collapse them.

    #[test]
    fn get_space_unnormalized() {
        assert_eq!(uri_encode_path(b"/example space/"), "/example%20space/");
        assert_eq!(
            signature_of(&sign_suite("/example space/", None)),
            "652487583200325589f1fba4c7e578f72c47cb61beeca81406b39ddec1366741"
        );
    }

    #[test]
    fn get_slashes_unnormalized() {
        assert_eq!(
            signature_of(&sign_suite("//example//", None)),
            "87cca117541a147f6df867677d98a7d80dff226d2bfca9e4ffa899665623c7e5"
        );
    }

    #[test]
    fn get_relative_relative_unnormalized() {
        assert_eq!(
            signature_of(&sign_suite("/example1/example2/../..", None)),
            "dc33e0856fd4baca4d7aa2146c38958283844764f38c74252a333df5e613003b"
        );
    }

    /// The GET Object example from the S3 API reference ("Signature
    /// Version 4: authenticating requests using the Authorization
    /// header"). Unlike the suite vectors, it uses the service name
    /// `s3` and signs `x-amz-content-sha256`, as every request of ours
    /// does. It also signs a `range` header, which we never send.
    #[test]
    fn s3_get_object_example() {
        let creds = Credentials {
            access_key_id: "AKIAIOSFODNN7EXAMPLE".into(),
            secret_access_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into(),
            session_token: None,
        };
        let datetime = "20130524T000000Z";
        let headers = [
            ("host", "examplebucket.s3.amazonaws.com"),
            ("range", "bytes=0-9"),
            ("x-amz-content-sha256", EMPTY_SHA256),
            ("x-amz-date", datetime),
        ];
        let req = Request {
            method: "GET",
            canonical_uri: "/test.txt",
            canonical_query: "",
            headers: &headers,
            payload_hash: EMPTY_SHA256,
        };
        assert_eq!(
            authorization(&req, &creds, "us-east-1", "s3", datetime),
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, \
             SignedHeaders=host;range;x-amz-content-sha256;x-amz-date, \
             Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        );
    }

    /// Header order does not change the signature, because the signer
    /// sorts the headers.
    #[test]
    fn header_order_does_not_matter() {
        let req = |headers: &[(&str, &str)]| {
            let r = Request {
                method: "GET",
                canonical_uri: "/",
                canonical_query: "",
                headers,
                payload_hash: EMPTY_SHA256,
            };
            authorization(&r, &creds(None), "us-east-1", "service", DATETIME)
        };
        assert_eq!(
            req(&[("x-amz-date", DATETIME), ("host", HOST)]),
            req(&[("host", HOST), ("x-amz-date", DATETIME)])
        );
    }

    /// The HTTP mode keeps the sub-delims as they are. Here we must
    /// encode them. If we do not, S3 builds a different canonical URI
    /// and answers 403.
    #[test]
    fn sub_delims_are_encoded() {
        assert_eq!(
            uri_encode_path(b"/a+b!$&'()*,;=:@.jpg"),
            "/a%2Bb%21%24%26%27%28%29%2A%2C%3B%3D%3A%40.jpg"
        );
        assert_eq!(uri_encode_path(b"/100%.jpg"), "/100%25.jpg");
        // A key cannot add a query or a fragment to the URL we send.
        assert_eq!(uri_encode_path(b"/a?b#c.jpg"), "/a%3Fb%23c.jpg");
    }

    #[test]
    fn header_values_are_trimmed_and_folded() {
        assert_eq!(canonical_header_value("  a   b  c "), "a b c");
    }

    #[test]
    fn amz_datetime_formats_utc() {
        assert_eq!(amz_datetime(0), "19700101T000000Z");
        // The suite's timestamp, 2015-08-30T12:36:00Z.
        assert_eq!(amz_datetime(1_440_938_160), DATETIME);
        // Leap day, and the last second of a leap year.
        assert_eq!(amz_datetime(951_782_400), "20000229T000000Z");
        assert_eq!(amz_datetime(1_735_689_599), "20241231T235959Z");
    }
}
