//! The lowercase hex text of a SHA-256 digest. Stored hashes, session tokens and derived
//! identifiers keep this exact form, so a new version of sha2 changes no stored value.
use sha2::{Digest, Sha256};
use std::fmt::Write;

/// The SHA-256 digest of `data` as 64 lowercase hex characters.
pub fn sha256_hex(data: impl AsRef<[u8]>) -> String {
    Sha256::digest(data)
        .iter()
        .fold(String::with_capacity(64), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_digest_text_keeps_the_stored_lowercase_hex_form() {
        assert_eq!(
            super::sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
