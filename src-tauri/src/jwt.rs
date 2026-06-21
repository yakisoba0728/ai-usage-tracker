//! Decode a JWT payload WITHOUT verifying the signature.
//! Intended for non-secret metadata only (plan, email, account id, exp).
//! Tolerates both padded and unpadded base64url.

use serde_json::Value;

pub fn jwt_payload(token: &str) -> Result<Value, String> {
    let segs: Vec<&str> = token.split('.').collect();
    if segs.len() < 2 {
        return Err("not a JWT".into());
    }
    let payload = segs[1];
    use base64::engine::general_purpose::{STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
    use base64::Engine;

    // OpenAI id_tokens are base64url, sometimes WITH padding; try the common
    // engines concretely (the Engine trait isn't dyn-compatible).
    let attempts: [Result<Vec<u8>, _>; 3] = [
        URL_SAFE_NO_PAD.decode(payload),
        URL_SAFE.decode(payload),
        STANDARD_NO_PAD.decode(payload),
    ];
    let mut last_err = String::from("no engine decoded");
    for bytes in attempts.into_iter().flatten() {
        match serde_json::from_slice::<Value>(&bytes) {
            Ok(v) => return Ok(v),
            Err(e) => last_err = format!("json: {e}"),
        }
    }
    // Last resort: strip trailing '=' and retry url-safe no-pad.
    let stripped = payload.trim_end_matches('=');
    if let Ok(bytes) = URL_SAFE_NO_PAD.decode(stripped) {
        if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
            return Ok(v);
        }
    }
    Err(last_err)
}

/// Expiry (exp) claim as epoch seconds, if present.
pub fn jwt_exp(token: &str) -> Option<i64> {
    jwt_payload(token).ok()?.get("exp")?.as_i64()
}

/// Extract `(email, chatgpt_account_id)` from a Codex/OpenAI id_token. Both the
/// device-code and loopback OAuth login paths need this identity, so it lives
/// here. Either field is `None` when absent or the token can't be decoded.
pub fn codex_identity(id_token: &str) -> (Option<String>, Option<String>) {
    let Ok(claims) = jwt_payload(id_token) else {
        return (None, None);
    };
    let email = claims
        .get("email")
        .and_then(|v| v.as_str())
        .map(String::from);
    let account_id = claims
        .get("https://api.openai.com/auth")
        .and_then(|a| a.get("chatgpt_account_id"))
        .and_then(|v| v.as_str())
        .map(String::from);
    (email, account_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
    use base64::Engine;

    #[test]
    fn decodes_unpadded_payload() {
        let payload = URL_SAFE_NO_PAD.encode(br#"{"plan":"pro","exp":100}"#);
        let tok = format!("hdr.{payload}.sig");
        let v = jwt_payload(&tok).unwrap();
        assert_eq!(v["plan"], "pro");
        assert_eq!(v["exp"], 100);
        assert_eq!(jwt_exp(&tok), Some(100));
    }

    #[test]
    fn decodes_padded_payload() {
        // OpenAI-style base64url WITH padding
        let payload = URL_SAFE.encode(br#"{"email":"a@b.c"}"#);
        let tok = format!("hdr.{payload}.sig");
        let v = jwt_payload(&tok).unwrap();
        assert_eq!(v["email"], "a@b.c");
    }

    #[test]
    fn rejects_non_jwt() {
        assert!(jwt_payload("not-a-jwt").is_err());
    }

    #[test]
    fn codex_identity_extracts_email_and_account_id() {
        let payload = URL_SAFE_NO_PAD.encode(
            br#"{"email":"u@x.com","https://api.openai.com/auth":{"chatgpt_account_id":"acct-9"}}"#,
        );
        let tok = format!("hdr.{payload}.sig");
        let (email, acct) = codex_identity(&tok);
        assert_eq!(email.as_deref(), Some("u@x.com"));
        assert_eq!(acct.as_deref(), Some("acct-9"));

        // Missing claims / undecodable tokens → (None, None).
        let no_claims = URL_SAFE_NO_PAD.encode(br#"{"sub":"123"}"#);
        let (e2, a2) = codex_identity(&format!("hdr.{no_claims}.sig"));
        assert!(e2.is_none() && a2.is_none());
        assert_eq!(codex_identity("garbage"), (None, None));
    }
}
