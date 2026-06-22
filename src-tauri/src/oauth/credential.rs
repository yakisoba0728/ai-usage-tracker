//! Turning a token response into a `StoredCredential`, and classifying one
//! inbound callback request purely from its query params (the X-3 state-first
//! security decision).

use serde_json::Value;

use crate::jwt::jwt_payload;
use crate::model::Provider;
use crate::store::StoredCredential;

/// What to do with one inbound callback request, decided purely from its query
/// params. `state` is validated FIRST (X-3): any request whose state does not
/// match ours is `Ignore`d — NOT treated as an error — so an attacker-supplied
/// `?error=...` to the fixed callback port cannot abort the user's in-flight
/// login. Only a state-matched request can finish the login (success or error).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CallbackAction {
    /// Wrong/missing state — not our callback. Reject and keep waiting.
    Ignore,
    /// State-matched genuine provider error → fail the login.
    Error(String),
    /// State-matched success → use this authorization code.
    Code(String),
    /// State-matched but no usable code → keep waiting.
    MissingCode,
}

pub(crate) fn classify_callback(
    params: &std::collections::HashMap<String, String>,
    expected_state: &str,
) -> CallbackAction {
    // X-3: validate `state` before acting on anything else. A non-matching
    // (or absent) state means the request is not the user's real callback.
    if params.get("state").map(|s| s.as_str()) != Some(expected_state) {
        return CallbackAction::Ignore;
    }
    if let Some(err) = params.get("error") {
        return CallbackAction::Error(err.clone());
    }
    match params.get("code") {
        Some(c) if !c.is_empty() => CallbackAction::Code(c.clone()),
        _ => CallbackAction::MissingCode,
    }
}

pub(crate) fn build_credential(
    provider: Provider,
    tokens: &Value,
) -> Result<StoredCredential, String> {
    let access_token = tokens
        .get("access_token")
        .or_else(|| tokens.get("accessToken"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .ok_or_else(|| "OAuth token response missing access_token".to_string())?;
    let refresh_token = tokens
        .get("refresh_token")
        .or_else(|| tokens.get("refreshToken"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let id_token = tokens
        .get("id_token")
        .or_else(|| tokens.get("idToken"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let expires_in = tokens.get("expires_in").and_then(|v| v.as_u64());
    let expires_at = expires_in
        .map(|s| chrono::Utc::now().timestamp_millis() + (s as i64) * 1000)
        .unwrap_or(0);

    let (label, account_id) = match (provider, &id_token) {
        (Provider::Codex, Some(jwt)) => {
            let (email, acct) = crate::jwt::codex_identity(jwt);
            (email.unwrap_or_else(|| "Codex account".into()), acct)
        }
        (Provider::Gemini, Some(jwt)) => {
            // Google id_token JWT carries the user email at the top level.
            let email = jwt_payload(jwt)
                .ok()
                .and_then(|c| c.get("email").and_then(|v| v.as_str()).map(String::from));
            (email.unwrap_or_else(|| "Gemini account".into()), None)
        }
        _ => (format!("{provider:?} account"), None),
    };

    Ok(StoredCredential {
        id: String::new(),
        provider,
        label,
        access_token,
        refresh_token,
        expires_at,
        id_token,
        account_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn callback_with_wrong_or_missing_state_is_ignored_not_errored() {
        // The security property (X-3): an attacker hitting the fixed callback
        // port with ?error=... and a wrong/absent state must NOT abort the login.
        assert_eq!(
            classify_callback(&params(&[("error", "access_denied")]), "S"),
            CallbackAction::Ignore,
            "error with NO state must be ignored, not treated as a provider error"
        );
        assert_eq!(
            classify_callback(&params(&[("error", "x"), ("state", "WRONG")]), "S"),
            CallbackAction::Ignore
        );
        assert_eq!(
            classify_callback(&params(&[("code", "c"), ("state", "WRONG")]), "S"),
            CallbackAction::Ignore,
            "even a code with the wrong state is ignored"
        );
    }

    #[test]
    fn state_matched_callback_resolves_error_or_code() {
        assert_eq!(
            classify_callback(&params(&[("error", "denied"), ("state", "S")]), "S"),
            CallbackAction::Error("denied".into())
        );
        assert_eq!(
            classify_callback(&params(&[("code", "abc"), ("state", "S")]), "S"),
            CallbackAction::Code("abc".into())
        );
        // State-matched but empty/absent code → keep waiting, don't fail.
        assert_eq!(
            classify_callback(&params(&[("code", ""), ("state", "S")]), "S"),
            CallbackAction::MissingCode
        );
        assert_eq!(
            classify_callback(&params(&[("state", "S")]), "S"),
            CallbackAction::MissingCode
        );
    }

    /// Encode a fake JWT (`hdr.<payload>.sig`) whose middle segment is the given
    /// claims, so `build_credential`'s id_token parsing can be exercised offline.
    fn fake_jwt(claims: serde_json::Value) -> String {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
        format!("hdr.{payload}.sig")
    }

    #[test]
    fn build_credential_extracts_codex_email_and_account_id() {
        let id_token = fake_jwt(serde_json::json!({
            "email": "codex-oauth@example.invalid",
            "https://api.openai.com/auth": { "chatgpt_account_id": "acct-123" },
        }));
        let tokens = serde_json::json!({
            "access_token": "at",
            "refresh_token": "rt",
            "id_token": id_token,
            "expires_in": 3600,
        });
        let cred = build_credential(Provider::Codex, &tokens).unwrap();
        assert_eq!(cred.access_token, "at");
        assert_eq!(cred.refresh_token.as_deref(), Some("rt"));
        assert_eq!(cred.label, "codex-oauth@example.invalid");
        assert_eq!(cred.account_id.as_deref(), Some("acct-123"));
        assert!(cred.expires_at > 0, "expires_in should set expires_at");
    }

    #[test]
    fn build_credential_rejects_missing_access_token() {
        let tokens = serde_json::json!({
            "refresh_token": "rt",
            "expires_in": 3600,
        });

        let err = build_credential(Provider::Codex, &tokens).unwrap_err();

        assert!(err.contains("access_token"));
    }

    #[test]
    fn build_credential_uses_gemini_email_and_no_account_id() {
        let id_token = fake_jwt(serde_json::json!({ "email": "gemini-oauth@example.invalid" }));
        let tokens = serde_json::json!({ "access_token": "g-at", "id_token": id_token });
        let cred = build_credential(Provider::Gemini, &tokens).unwrap();
        assert_eq!(cred.label, "gemini-oauth@example.invalid");
        assert_eq!(cred.account_id, None);
        assert_eq!(cred.expires_at, 0, "no expires_in → unknown expiry");
    }

    #[test]
    fn build_credential_falls_back_to_generic_label_without_id_token() {
        let tokens = serde_json::json!({ "access_token": "at" });
        let cred = build_credential(Provider::Codex, &tokens).unwrap();
        assert_eq!(cred.access_token, "at");
        assert_eq!(cred.label, "Codex account");
        assert_eq!(cred.account_id, None);
    }
}
