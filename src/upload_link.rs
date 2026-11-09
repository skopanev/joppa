//! The upload link of joppa_attach with upload: true. The token carries the whole attach
//! call and its expiry, signed with HMAC-SHA256 under a random key of this process, so a
//! restart makes the older links invalid. The link itself is the credential of the PUT.
use crate::spaces_sign::hmac_sha256;
use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// The test hook: the lifetime of a link in seconds. Production never sets it.
const LIFETIME: &str = "JOPPA_UPLOAD_LINK_SECONDS";

/// One attach call that waits for its bytes.
#[derive(Serialize, Deserialize, Clone)]
pub struct Upload {
    pub actor: String,
    pub alias: Option<String>,
    pub workspace: String,
    pub target: String,
    pub filename: String,
    pub content_type: String,
    pub check_result: Option<String>,
    pub valid_until: Option<String>,
    pub request_id: String,
    /// The end of the link, in Unix seconds.
    pub expires: i64,
}

/// Why the server refuses a link.
#[derive(Debug, PartialEq)]
pub enum Refusal {
    Invalid,
    Expired,
}

fn key() -> &'static [u8; 32] {
    static KEY: OnceLock<[u8; 32]> = OnceLock::new();
    KEY.get_or_init(|| {
        let mut key = [0; 32];
        key[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        key[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        key
    })
}

/// The lifetime of a new link: 900 seconds unless the test hook sets another.
pub fn lifetime() -> Result<i64> {
    match std::env::var(LIFETIME) {
        Ok(text) => text
            .parse()
            .with_context(|| format!("{LIFETIME} is not a number")),
        Err(_) => Ok(900),
    }
}

/// The token of `upload`: the base64url JSON, a dot and the base64url signature.
pub fn sign(upload: &Upload) -> Result<String> {
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(upload)?);
    let signature = URL_SAFE_NO_PAD.encode(hmac_sha256(key(), payload.as_bytes()));
    Ok(format!("{payload}.{signature}"))
}

/// The attach call of `token`. The signature comes first, then the lifetime.
pub fn open(token: &str) -> std::result::Result<Upload, Refusal> {
    let (payload, signature) = token.split_once('.').ok_or(Refusal::Invalid)?;
    let signature = URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| Refusal::Invalid)?;
    let expected = hmac_sha256(key(), payload.as_bytes());
    // The comparison reads every byte, so its time tells nothing about the signature.
    let same = signature.len() == expected.len()
        && signature
            .iter()
            .zip(expected)
            .fold(0, |d, (a, b)| d | (a ^ b))
            == 0;
    if !same {
        return Err(Refusal::Invalid);
    }
    let json = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| Refusal::Invalid)?;
    let upload: Upload = serde_json::from_slice(&json).map_err(|_| Refusal::Invalid)?;
    if chrono::Utc::now().timestamp() >= upload.expires {
        return Err(Refusal::Expired);
    }
    Ok(upload)
}
