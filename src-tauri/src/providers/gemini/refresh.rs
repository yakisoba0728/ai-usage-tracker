//! Gemini OAuth refresh: same Google OAuth endpoint + client metadata
//! (env override > Gemini CLI's public constants) as the CLI-path self-refresh.

use crate::http;
use crate::providers::ProviderError;

const GEMINI_CLIENT_ID: &str =
    "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com";
const GEMINI_CLIENT_SECRET: &str = "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";
const GEMINI_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

#[derive(serde::Deserialize)]
struct Refreshed {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

/// Resolve client metadata: env override > Gemini CLI's public constants.
fn gemini_client_metadata() -> (String, String) {
    if let (Ok(id), Ok(sec)) = (
        std::env::var("GEMINI_OAUTH_CLIENT_ID"),
        std::env::var("GEMINI_OAUTH_CLIENT_SECRET"),
    ) {
        if !id.is_empty() && !sec.is_empty() {
            return (id, sec);
        }
    }
    (
        GEMINI_CLIENT_ID.to_string(),
        GEMINI_CLIENT_SECRET.to_string(),
    )
}

/// Resolve the OAuth token endpoint: env override > Google's public URL.
/// Mirrors the `GEMINI_OAUTH_CLIENT_ID/_SECRET` override one function above —
/// a plain runtime env hook (same category as `AIT_CONFIG_PATH`/
/// `AIT_ACCOUNTS_PATH`) so a test (or a future self-hosted proxy) can point the
/// refresh POST at a local server without changing the production signature.
/// Defaults to the const, so production behavior is unchanged.
fn gemini_token_url() -> String {
    match std::env::var("GEMINI_OAUTH_TOKEN_URL") {
        Ok(url) if !url.is_empty() => url,
        _ => GEMINI_TOKEN_URL.to_string(),
    }
}

async fn refresh_gemini_token(
    http: &reqwest::Client,
    rt: &str,
) -> Result<Refreshed, ProviderError> {
    let (id, sec) = gemini_client_metadata();
    let resp = http
        .post(gemini_token_url())
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", rt),
            ("client_id", id.as_str()),
            ("client_secret", sec.as_str()),
        ])
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    let v = http::send_for_json(resp, "gemini refresh").await?;
    serde_json::from_value::<Refreshed>(v)
        .map_err(|e| ProviderError::Parse(format!("gemini refresh: {e}")))
}

/// Build a refreshed `StoredCredential` from a successful token refresh.
/// Pure: preserves `id`/`provider`/`label`/`id_token`/`account_id`, rotates
/// `access_token`/`refresh_token`/`expires_at`. Keeps the old refresh_token
/// when Google's response omits a fresh one (Google rotates them rarely).
fn build_refreshed_cred(
    cred: &crate::store::StoredCredential,
    fresh: &Refreshed,
    now_ms: i64,
) -> crate::store::StoredCredential {
    let expires_at = now_ms + (fresh.expires_in.unwrap_or(3600) as i64) * 1000;
    crate::store::rotate_credential(
        cred,
        fresh.access_token.clone(),
        fresh.refresh_token.clone(),
        None,
        expires_at,
    )
}

/// Refresh a stored credential's access_token using its refresh_token via the
/// same Google OAuth endpoint + client metadata (env override > Gemini CLI's
/// public constants) as the CLI-path self-refresh. Returns `Some(updated_cred)`
/// when a refresh happened (caller persists). Stored OAuth credentials that need
/// refresh must not fall back to a stale token, so missing refresh grants and
/// refresh failures are surfaced as provider errors.
pub(crate) async fn refresh_stored(
    http: &reqwest::Client,
    cred: &crate::store::StoredCredential,
) -> Result<Option<crate::store::StoredCredential>, ProviderError> {
    let rt = cred
        .refresh_token
        .as_ref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ProviderError::Expired(
                "stored Gemini token expired or is near expiry and has no refresh_token".into(),
            )
        })?;
    let fresh = refresh_gemini_token(http, rt).await?;
    let now_ms = chrono::Utc::now().timestamp_millis();
    Ok(Some(build_refreshed_cred(cred, &fresh, now_ms)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Provider;

    fn sample_stored(rt: Option<&str>) -> crate::store::StoredCredential {
        crate::store::StoredCredential {
            id: "abc".into(),
            provider: Provider::Gemini,
            label: "gemini-stored@example.invalid".into(),
            access_token: "old-at".into(),
            refresh_token: rt.map(str::to_string),
            expires_at: 1,
            id_token: Some("jwt-payload".into()),
            account_id: None,
        }
    }

    #[test]
    fn build_refreshed_cred_rotates_tokens_and_preserves_fields() {
        let cred = sample_stored(Some("old-rt"));
        let fresh = Refreshed {
            access_token: "new-at".into(),
            refresh_token: Some("new-rt".into()),
            expires_in: Some(3600),
        };
        let out = build_refreshed_cred(&cred, &fresh, 1_000_000);
        // Preserved verbatim.
        assert_eq!(out.id, "abc");
        assert_eq!(out.provider, Provider::Gemini);
        assert_eq!(out.label, "gemini-stored@example.invalid");
        assert_eq!(out.id_token.as_deref(), Some("jwt-payload"));
        assert!(out.account_id.is_none());
        // Rotated.
        assert_eq!(out.access_token, "new-at");
        assert_eq!(out.refresh_token.as_deref(), Some("new-rt"));
        assert_eq!(out.expires_at, 1_000_000 + 3600 * 1000);
    }

    #[test]
    fn build_refreshed_cred_keeps_old_rt_when_response_omits_one() {
        // Google rarely rotates refresh_tokens; the response may omit it.
        let cred = sample_stored(Some("old-rt"));
        let fresh = Refreshed {
            access_token: "new-at".into(),
            refresh_token: None,
            expires_in: None, // default 3600s fallback applies
        };
        let out = build_refreshed_cred(&cred, &fresh, 0);
        assert_eq!(out.access_token, "new-at");
        assert_eq!(out.refresh_token.as_deref(), Some("old-rt"));
        assert_eq!(out.expires_at, 3600 * 1000);
    }
}
