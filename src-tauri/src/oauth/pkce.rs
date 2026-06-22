//! PKCE (RFC 7636) verifier/challenge generation and the shared random base64url
//! helper used for both the verifier and the OAuth `state` parameter.

use base64::Engine;
use rand::Rng;
use sha2::{Digest, Sha256};

pub(crate) fn pkce_verifier() -> String {
    // RFC 7636 §4.1: 43–128 chars; 32 random bytes → 43 base64url chars.
    random_b64(32)
}

pub(crate) fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

pub(crate) fn random_b64(n: usize) -> String {
    let mut bytes = vec![0u8; n];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_roundtrip() {
        let v = pkce_verifier();
        let c = pkce_challenge(&v);
        assert!(c.len() > 20);
        assert_ne!(v, c);
    }
}
