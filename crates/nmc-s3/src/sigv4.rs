//! AWS Signature Version 4 for S3 requests.
//!
//! The payload is sent as `UNSIGNED-PAYLOAD`: TLS already protects it, and
//! it lets uploads stream without hashing everything first. Reference:
//! <https://docs.aws.amazon.com/AmazonS3/latest/API/sig-v4-header-based-auth.html>

use hmac::{Hmac, KeyInit, Mac};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use sha2::{Digest, Sha256};

/// The payload hash sent with every request.
pub const UNSIGNED_PAYLOAD: &str = "UNSIGNED-PAYLOAD";

/// Everything except the unreserved characters `A-Z a-z 0-9 - _ . ~`.
const ENCODE: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');

/// Encodes a query parameter, or one segment of a key.
pub fn encode(text: &str) -> String {
    utf8_percent_encode(text, ENCODE).to_string()
}

/// Encodes an object key for a URL path: `/` stays as is.
pub fn encode_key(key: &str) -> String {
    key.split('/').map(encode).collect::<Vec<_>>().join("/")
}

/// An access key.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    /// Access key id.
    pub access_key: String,
    /// Secret access key.
    pub secret_key: String,
    /// Session token of temporary credentials.
    pub session_token: Option<String>,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the secret.
        f.debug_struct("Credentials").field("access_key", &self.access_key).finish_non_exhaustive()
    }
}

/// A request to sign. `path` is already encoded; `query` is not.
pub struct Request<'a> {
    /// `GET`, `PUT`, ...
    pub method: &'a str,
    /// The `Host` header.
    pub host: &'a str,
    /// The encoded URL path.
    pub path: &'a str,
    /// Query parameters, not encoded.
    pub query: &'a [(String, String)],
    /// Headers to sign besides `host`, `x-amz-date` and
    /// `x-amz-content-sha256`, with lowercase names.
    pub headers: &'a [(String, String)],
}

/// The headers to add to `request`: `x-amz-date`, `x-amz-content-sha256`,
/// `x-amz-security-token` (temporary credentials) and `authorization`.
/// `time` is `YYYYMMDDTHHMMSSZ`.
pub fn sign(
    request: &Request<'_>,
    credentials: &Credentials,
    region: &str,
    time: &str,
) -> Vec<(String, String)> {
    sign_with_payload(request, credentials, region, time, UNSIGNED_PAYLOAD)
}

/// [`sign`] with another payload hash.
fn sign_with_payload(
    request: &Request<'_>,
    credentials: &Credentials,
    region: &str,
    time: &str,
    payload: &str,
) -> Vec<(String, String)> {
    let date = &time[..8];
    let mut added = vec![
        ("x-amz-content-sha256".to_owned(), payload.to_owned()),
        ("x-amz-date".to_owned(), time.to_owned()),
    ];
    if let Some(token) = &credentials.session_token {
        added.push(("x-amz-security-token".to_owned(), token.clone()));
    }

    let mut headers: Vec<(String, String)> = vec![("host".into(), request.host.into())];
    headers.extend(request.headers.iter().cloned());
    headers.extend(added.iter().cloned());
    headers.sort();
    let signed_headers: Vec<&str> = headers.iter().map(|(name, _)| name.as_str()).collect();
    let signed_headers = signed_headers.join(";");
    let canonical_headers: String =
        headers.iter().map(|(name, value)| format!("{name}:{}\n", value.trim())).collect();

    let mut query: Vec<(String, String)> =
        request.query.iter().map(|(k, v)| (encode(k), encode(v))).collect();
    query.sort();
    let canonical_query: Vec<String> = query.iter().map(|(k, v)| format!("{k}={v}")).collect();

    let canonical_request = [
        request.method,
        request.path,
        &canonical_query.join("&"),
        &canonical_headers,
        &signed_headers,
        payload,
    ]
    .join("\n");

    let scope = format!("{date}/{region}/s3/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{time}\n{scope}\n{}",
        hex::encode(Sha256::digest(canonical_request.as_bytes()))
    );
    let mut key = hmac(format!("AWS4{}", credentials.secret_key).as_bytes(), date.as_bytes());
    for part in [region, "s3", "aws4_request"] {
        key = hmac(&key, part.as_bytes());
    }
    let signature = hex::encode(hmac(&key, string_to_sign.as_bytes()));
    added.push((
        "authorization".to_owned(),
        format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, \
             Signature={signature}",
            credentials.access_key
        ),
    ));
    added
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The "GET Object" example of the AWS documentation, which signs the
    /// hash of an empty payload instead of `UNSIGNED-PAYLOAD`.
    #[test]
    fn matches_the_aws_documentation_example() {
        let credentials = Credentials {
            access_key: "AKIAIOSFODNN7EXAMPLE".into(),
            secret_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into(),
            session_token: None,
        };
        let empty = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let headers = [("range".to_owned(), "bytes=0-9".to_owned())];
        let request = Request {
            method: "GET",
            host: "examplebucket.s3.amazonaws.com",
            path: "/test.txt",
            query: &[],
            headers: &headers,
        };
        let signed =
            sign_with_payload(&request, &credentials, "us-east-1", "20130524T000000Z", empty);
        let authorization = signed.iter().find(|(n, _)| n == "authorization").unwrap();
        assert!(
            authorization.1.ends_with(
                "Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
            ),
            "{}",
            authorization.1
        );
    }

    #[test]
    fn encoding_keeps_only_unreserved_characters() {
        assert_eq!(encode("a b/c~d"), "a%20b%2Fc~d");
        assert_eq!(encode_key("dir/My File+.txt"), "dir/My%20File%2B.txt");
        assert_eq!(encode_key("caffè"), "caff%C3%A8");
    }
}
