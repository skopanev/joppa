//! AWS Signature Version 4 for presigned S3 URLs: query string, `host` as the only
//! signed header and an unsigned payload. DigitalOcean Spaces accepts the same form.
use sha2::{Digest, Sha256};

/// HMAC-SHA256 (RFC 2104) on sha2, the hash that the server already uses.
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut block = [0u8; 64];
    if key.len() > 64 {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let pad = |byte: u8| block.iter().map(|b| b ^ byte).collect::<Vec<u8>>();
    let inner = Sha256::new()
        .chain_update(pad(0x36))
        .chain_update(message)
        .finalize();
    let outer = Sha256::new()
        .chain_update(pad(0x5c))
        .chain_update(inner)
        .finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&outer);
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// RFC 3986 encoding of S3: every byte except the unreserved characters, and `/` too
/// unless `slash` keeps it, as in an object path.
pub fn encode(text: &str, slash: bool) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            b'/' if slash => "/".into(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// The credentials and the place of one bucket region.
pub struct Signer<'a> {
    pub key_id: &'a str,
    pub secret: &'a str,
    pub region: &'a str,
}

impl Signer<'_> {
    /// The canonical query string with the signature for `method host path` at `time`
    /// (`YYYYMMDDTHHMMSSZ`), valid `ttl` seconds. `extra` adds query keys, for example
    /// `response-content-type`.
    pub fn query(
        &self,
        method: &str,
        host: &str,
        path: &str,
        time: &str,
        ttl: u32,
        extra: &[(&str, &str)],
    ) -> String {
        let date = &time[..8];
        let scope = format!("{date}/{}/s3/aws4_request", self.region);
        let credential = format!("{}/{scope}", self.key_id);
        let ttl = ttl.to_string();
        let mut pairs: Vec<(String, String)> = [
            ("X-Amz-Algorithm", "AWS4-HMAC-SHA256"),
            ("X-Amz-Credential", credential.as_str()),
            ("X-Amz-Date", time),
            ("X-Amz-Expires", ttl.as_str()),
            ("X-Amz-SignedHeaders", "host"),
        ]
        .iter()
        .chain(extra)
        .map(|(k, v)| (encode(k, false), encode(v, false)))
        .collect();
        pairs.sort();
        let query = pairs
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");
        let canonical = format!(
            "{method}\n{}\n{query}\nhost:{host}\n\nhost\nUNSIGNED-PAYLOAD",
            encode(path, true)
        );
        let text = format!(
            "AWS4-HMAC-SHA256\n{time}\n{scope}\n{}",
            hex(&Sha256::digest(canonical.as_bytes()))
        );
        let mut key = hmac_sha256(format!("AWS4{}", self.secret).as_bytes(), date.as_bytes());
        for part in [self.region, "s3", "aws4_request"] {
            key = hmac_sha256(&key, part.as_bytes());
        }
        format!(
            "{query}&X-Amz-Signature={}",
            hex(&hmac_sha256(&key, text.as_bytes()))
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_matches_rfc_4231_case_2() {
        assert_eq!(
            hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    /// The presigned GET example of the Amazon S3 documentation.
    #[test]
    fn a_presigned_get_matches_the_aws_example() {
        let signer = Signer {
            key_id: "AKIAIOSFODNN7EXAMPLE",
            secret: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
            region: "us-east-1",
        };
        let query = signer.query(
            "GET",
            "examplebucket.s3.amazonaws.com",
            "/test.txt",
            "20130524T000000Z",
            86400,
            &[],
        );
        assert!(
            query.ends_with(
                "X-Amz-Signature=aeeed9bbccd4d02ee5c0109b86d86835f995330da4c265957d157751f604d404"
            ),
            "{query}"
        );
    }
}
