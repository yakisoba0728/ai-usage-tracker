//! Codex OAuth refresh: both the auto/CLI-path self-refresh
//! (`refresh_auto_auth_if_needed`, which persists back to `auth.json`) and the
//! stored-account `build_refreshed_cred` path. POST `auth.openai.com/oauth/token`
//! with the public CLI client_id from codex-rs/login.

use serde_json::Value;

use crate::http;
use crate::providers::ProviderError;

use super::creds::{read_auth, write_auth, Tokens};

/// Public OAuth client_id shipped in codex-rs/login (`auth/manager.rs::CLIENT_ID`).
/// Used for both the device-code login and `refresh_token` grant.
const CODEX_OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
/// OAuth token endpoint (codex-rs/login `REFRESH_TOKEN_URL`).
const CODEX_REFRESH_URL: &str = "https://auth.openai.com/oauth/token";

static AUTO_REFRESH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn access_needs_refresh(access_token: &str, now_sec: i64) -> bool {
    crate::jwt::jwt_exp(access_token)
        .map(|exp| exp <= now_sec + 300)
        .unwrap_or(false)
}

fn apply_auth_refresh(mut auth: Value, fresh: &Refreshed, refreshed_at: &str) -> Option<Value> {
    let access = fresh.access_token.as_ref()?;
    let tokens = auth.get_mut("tokens")?.as_object_mut()?;
    let account_id = fresh
        .id_token
        .as_deref()
        .or_else(|| tokens.get("id_token").and_then(|v| v.as_str()))
        .and_then(|jwt| crate::jwt::codex_identity(jwt).1);
    tokens.insert("access_token".into(), serde_json::json!(access));
    if let Some(id_token) = &fresh.id_token {
        tokens.insert("id_token".into(), serde_json::json!(id_token));
        if !tokens.contains_key("account_id") {
            if let Some(account_id) = crate::jwt::codex_identity(id_token).1 {
                tokens.insert("account_id".into(), serde_json::json!(account_id));
            }
        }
    }
    if let Some(refresh_token) = &fresh.refresh_token {
        tokens.insert("refresh_token".into(), serde_json::json!(refresh_token));
    }
    if !tokens.contains_key("account_id") {
        if let Some(account_id) = account_id {
            tokens.insert("account_id".into(), serde_json::json!(account_id));
        }
    }
    if let Some(obj) = auth.as_object_mut() {
        obj.insert("last_refresh".into(), serde_json::json!(refreshed_at));
    }
    Some(auth)
}

pub(super) fn account_id_for_tokens(tokens: &Tokens) -> Option<String> {
    tokens.account_id.clone().or_else(|| {
        tokens
            .id_token
            .as_deref()
            .and_then(|jwt| crate::jwt::codex_identity(jwt).1)
    })
}

pub(super) async fn refresh_auto_auth_if_needed(
    http: &reqwest::Client,
    tokens: Tokens,
) -> Result<Tokens, ProviderError> {
    if !access_needs_refresh(&tokens.access_token, chrono::Utc::now().timestamp()) {
        return Ok(tokens);
    }

    let _guard = AUTO_REFRESH_LOCK.lock().await;
    let (auth, mut latest) = read_auth()?;
    if !access_needs_refresh(&latest.access_token, chrono::Utc::now().timestamp()) {
        return Ok(latest);
    }
    let rt = latest
        .refresh_token
        .as_ref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ProviderError::Expired(
                "Codex token expired and auth.json has no refresh_token — run `codex login`."
                    .into(),
            )
        })?;
    let fresh = refresh_oauth(http, rt).await?;
    if fresh.access_token.is_none() {
        return Err(ProviderError::Parse(
            "codex refresh: missing access_token".into(),
        ));
    }
    let updated =
        apply_auth_refresh(auth, &fresh, &chrono::Utc::now().to_rfc3339()).ok_or_else(|| {
            ProviderError::Parse("codex refresh: auth.json has no tokens object".into())
        })?;
    write_auth(&updated)?;
    if let Some(access_token) = fresh.access_token {
        latest.access_token = access_token;
    }
    if let Some(id_token) = fresh.id_token {
        latest.id_token = Some(id_token);
    }
    if let Some(refresh_token) = fresh.refresh_token {
        latest.refresh_token = Some(refresh_token);
    }
    latest.account_id = account_id_for_tokens(&latest);
    Ok(latest)
}

/// Successful OAuth refresh response from `auth.openai.com/oauth/token`
/// (matches codex-rs/login `RefreshResponse`). `id_token`/`refresh_token`
/// may be omitted; `access_token` is always present on a successful refresh.
#[derive(serde::Deserialize)]
pub(super) struct Refreshed {
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
}

/// Pure: build the OAuth `refresh_token` grant request body. Token endpoint
/// grant requests are form-encoded, matching Codex CLI's token exchange path.
fn build_refresh_body(refresh_token: &str) -> String {
    format!(
        "client_id={}&grant_type=refresh_token&refresh_token={}",
        urlencoding::encode(CODEX_OAUTH_CLIENT_ID),
        urlencoding::encode(refresh_token),
    )
}

/// Pure: build a refreshed `StoredCredential`. Derives `expires_at` (epoch ms)
/// from the new access_token's JWT `exp`, falling back to the new id_token's
/// `exp`, then to 0 (unknown). Preserves `id`/`provider`/`label`/`account_id`;
/// takes the new `id_token` when supplied, else keeps the old one; keeps the
/// old refresh_token when the response omits a fresh one (OpenAI rotates per
/// refresh, but tolerate omission).
fn build_refreshed_cred(
    cred: &crate::store::StoredCredential,
    fresh: &Refreshed,
) -> crate::store::StoredCredential {
    let new_access = fresh.access_token.clone().unwrap_or_default();
    // exp from the new access token, falling back to the new-or-existing id_token.
    let new_id_token = fresh.id_token.clone().or_else(|| cred.id_token.clone());
    let expires_at = crate::jwt::jwt_exp(&new_access)
        .or_else(|| new_id_token.as_deref().and_then(crate::jwt::jwt_exp))
        .map(|s| s * 1000)
        .unwrap_or(0);
    let mut out = crate::store::rotate_credential(
        cred,
        new_access,
        fresh.refresh_token.clone(),
        fresh.id_token.clone(),
        expires_at,
    );
    if out.account_id.is_none() {
        out.account_id = new_id_token
            .as_deref()
            .and_then(|jwt| crate::jwt::codex_identity(jwt).1);
    }
    out
}

async fn refresh_oauth(
    http: &reqwest::Client,
    refresh_token: &str,
) -> Result<Refreshed, ProviderError> {
    let resp = http
        .post(CODEX_REFRESH_URL)
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(build_refresh_body(refresh_token))
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    let v = http::send_for_json(resp, "codex refresh").await?;
    serde_json::from_value::<Refreshed>(v)
        .map_err(|e| ProviderError::Parse(format!("codex refresh: {e}")))
}

/// Refresh a stored Codex OAuth credential via `auth.openai.com/oauth/token`
/// using the public CLI client_id (`app_EMoamEEZ73f0CkXaXp7hrann`) and a
/// `refresh_token` grant. Returns `Some(updated_cred)` when a refresh happened
/// (caller persists); `None` if there is no refresh_token, the network call
/// fails, the server returns non-2xx, or the response lacks an access_token
/// (caller falls back to the existing token).
pub(super) async fn refresh_stored(
    http: &reqwest::Client,
    cred: &crate::store::StoredCredential,
) -> Option<crate::store::StoredCredential> {
    let rt = cred.refresh_token.as_ref().filter(|s| !s.is_empty())?;
    let fresh = refresh_oauth(http, rt).await.ok()?;
    let _ = fresh.access_token.as_ref()?;
    Some(build_refreshed_cred(cred, &fresh))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Provider;

    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;

    fn jwt_with_claims(claims: serde_json::Value) -> String {
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
        format!("hdr.{payload}.sig")
    }

    fn jwt_with_exp(exp: i64) -> String {
        jwt_with_claims(serde_json::json!({ "exp": exp }))
    }

    fn stored(id: &str) -> crate::store::StoredCredential {
        crate::store::StoredCredential {
            id: id.into(),
            provider: Provider::Codex,
            label: "codex-stored@example.invalid".into(),
            access_token: "old.at".into(),
            refresh_token: Some("old.rt".into()),
            expires_at: 0,
            id_token: Some("old.id".into()),
            account_id: Some("acct-1".into()),
        }
    }

    #[test]
    fn refresh_body_uses_public_cli_client_id() {
        assert_eq!(CODEX_OAUTH_CLIENT_ID, "app_EMoamEEZ73f0CkXaXp7hrann");
        let body = build_refresh_body("rt-abc");
        assert_eq!(
            body,
            format!(
                "client_id={CODEX_OAUTH_CLIENT_ID}&grant_type=refresh_token&refresh_token=rt-abc"
            )
        );
    }

    #[test]
    fn build_refreshed_cred_rotates_tokens_and_extracts_exp() {
        let access_token = jwt_with_exp(1_700_000_000);
        let fresh = Refreshed {
            id_token: Some("new.id".into()),
            access_token: Some(access_token.clone()),
            refresh_token: Some("new.rt".into()),
        };
        let out = build_refreshed_cred(&stored("acc-1"), &fresh);
        assert_eq!(out.id, "acc-1");
        assert_eq!(out.provider, Provider::Codex);
        assert_eq!(out.label, "codex-stored@example.invalid");
        assert_eq!(out.account_id.as_deref(), Some("acct-1"));
        assert_eq!(out.access_token, access_token);
        assert_eq!(out.refresh_token.as_deref(), Some("new.rt"));
        assert_eq!(out.id_token.as_deref(), Some("new.id"));
        assert_eq!(out.expires_at, 1_700_000_000_000); // exp → epoch ms
    }

    #[test]
    fn build_refreshed_cred_derives_missing_account_id_from_id_token() {
        let id_token = jwt_with_claims(serde_json::json!({
            "https://api.openai.com/auth": { "chatgpt_account_id": "acct-from-id-token" },
        }));
        let mut cred = stored("acc-1");
        cred.account_id = None;
        cred.id_token = Some(id_token);
        let fresh = Refreshed {
            id_token: None,
            access_token: Some(jwt_with_exp(1_700_000_000)),
            refresh_token: Some("new.rt".into()),
        };

        let out = build_refreshed_cred(&cred, &fresh);

        assert_eq!(
            out.account_id.as_deref(),
            Some("acct-from-id-token"),
            "stored Codex refresh must backfill ChatGPT-Account-Id from id_token"
        );
    }

    #[test]
    fn build_refreshed_cred_keeps_old_refresh_and_zero_exp_when_omitted() {
        // Non-JWT access token: exp falls back to id_token (also absent) → 0.
        let fresh = Refreshed {
            id_token: None,
            access_token: Some("plain.string.token".into()),
            refresh_token: None,
        };
        let out = build_refreshed_cred(&stored("acc-1"), &fresh);
        assert_eq!(out.access_token, "plain.string.token");
        assert_eq!(out.refresh_token.as_deref(), Some("old.rt"));
        assert_eq!(out.id_token.as_deref(), Some("old.id")); // preserved
        assert_eq!(out.expires_at, 0);
    }

    #[test]
    fn build_refreshed_cred_falls_back_to_id_token_exp() {
        // access_token is opaque; exp comes from the new id_token.
        let id_token = jwt_with_exp(2_000_000_000);
        let fresh = Refreshed {
            id_token: Some(id_token),
            access_token: Some("opaque".into()),
            refresh_token: Some("new.rt".into()),
        };
        let out = build_refreshed_cred(&stored("acc-1"), &fresh);
        assert_eq!(out.expires_at, 2_000_000_000_000);
    }

    #[test]
    fn apply_auth_refresh_fills_missing_account_id_from_id_token() {
        let id_token = jwt_with_claims(serde_json::json!({
            "https://api.openai.com/auth": { "chatgpt_account_id": "acct-new" },
        }));
        let auth = serde_json::json!({
            "tokens": {
                "access_token": "old.access",
                "refresh_token": "old.refresh"
            }
        });
        let fresh = Refreshed {
            id_token: Some(id_token),
            access_token: Some("new.access".into()),
            refresh_token: Some("new.refresh".into()),
        };

        let out = apply_auth_refresh(auth, &fresh, "2026-06-20T12:00:00Z").unwrap();

        assert_eq!(out["tokens"]["account_id"], "acct-new");
    }

    #[test]
    fn account_id_for_tokens_uses_id_token_when_field_is_missing() {
        let id_token = jwt_with_claims(serde_json::json!({
            "https://api.openai.com/auth": { "chatgpt_account_id": "acct-fallback" },
        }));
        let tokens = Tokens {
            access_token: "access".into(),
            refresh_token: None,
            id_token: Some(id_token),
            account_id: None,
        };

        assert_eq!(
            account_id_for_tokens(&tokens).as_deref(),
            Some("acct-fallback")
        );
    }

    #[test]
    fn access_refresh_is_only_for_jwt_near_expiry() {
        assert!(access_needs_refresh(&jwt_with_exp(1_000), 800));
        assert!(!access_needs_refresh(&jwt_with_exp(2_000), 800));
        assert!(!access_needs_refresh("opaque.access.token", 800));
    }

    #[test]
    fn apply_auth_refresh_updates_tokens_and_last_refresh() {
        let auth: Value =
            serde_json::from_str(include_str!("../../../tests/codex_auth_fixture.json")).unwrap();
        let fresh = Refreshed {
            id_token: Some("new.id".into()),
            access_token: Some("new.access".into()),
            refresh_token: Some("new.refresh".into()),
        };
        let out = apply_auth_refresh(auth, &fresh, "2026-06-20T12:00:00Z").unwrap();
        assert_eq!(out["tokens"]["access_token"], "new.access");
        assert_eq!(out["tokens"]["id_token"], "new.id");
        assert_eq!(out["tokens"]["refresh_token"], "new.refresh");
        assert_eq!(out["tokens"]["account_id"], "acct_1");
        assert_eq!(out["last_refresh"], "2026-06-20T12:00:00Z");
    }
}
