//! Codex (ChatGPT) via Codex CLI. Reads `~/.codex/auth.json` and calls the SAME
//! endpoint the official codex CLI polls: `chatgpt.com/backend-api/wham/usage` with
//! `Authorization: Bearer <access_token>`, `ChatGPT-Account-Id`, and a
//! `codex_cli_rs/` User-Agent. Surfaces the main 5h/weekly rate limits, every
//! additional rate limit (e.g. `GPT-5.3-Codex-Spark`), credits, and available
//! rate-limit resets. Stored manual accounts self-refresh via `refresh_stored`
//! (POST `auth.openai.com/oauth/token`, public CLI client_id from codex-rs/login).
//! Token-parsing only.

use std::path::Path;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::http;
use crate::model::{auto_service_id, LimitWindow, Provider, ServiceSource, ServiceUsage};
use crate::providers::ProviderError;
use crate::secrets;

/// Usage endpoint, per openai/codex `backend-client/rate_limit_resets.rs`
/// (`PathStyle::ChatGptApi` → `/wham/usage` under `/backend-api`).
const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
/// User-Agent. Cloudflare's allow-list keys on the `codex_cli_rs/` prefix
/// (stable identifier in codex-rs/login `DEFAULT_ORIGINATOR`); the version
/// mirrors the current CLI build so the UA looks like a real client.
/// `ai-usage-tracker` is appended for transparency (allowed UA suffix).
const CODEX_UA: &str = "codex_cli_rs/0.141.0 (ai-usage-tracker)";
/// Public OAuth client_id shipped in codex-rs/login (`auth/manager.rs::CLIENT_ID`).
/// Used for both the device-code login and `refresh_token` grant.
const CODEX_OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
/// OAuth token endpoint (codex-rs/login `REFRESH_TOKEN_URL`).
const CODEX_REFRESH_URL: &str = "https://auth.openai.com/oauth/token";

#[derive(Deserialize)]
struct AuthDotJson {
    #[serde(default)]
    tokens: Option<Tokens>,
}
#[derive(Deserialize)]
struct Tokens {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    account_id: Option<String>,
}

#[derive(Deserialize, Default)]
struct WhamUsage {
    #[serde(default)]
    plan_type: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    rate_limit: Option<RateLimit>,
    #[serde(default)]
    additional_rate_limits: Vec<AdditionalLimit>,
    #[serde(default)]
    credits: Option<Credits>,
    #[serde(default)]
    rate_limit_reset_credits: Option<ResetCredits>,
    #[serde(default)]
    code_review_rate_limit: Option<RateLimit>,
}
#[derive(Deserialize, Default)]
struct RateLimit {
    #[serde(default)]
    primary_window: Option<RateWindow>,
    #[serde(default)]
    secondary_window: Option<RateWindow>,
}
#[derive(Deserialize, Default)]
struct RateWindow {
    #[serde(default)]
    used_percent: Option<f64>,
    #[serde(default)]
    reset_at: Option<i64>, // epoch seconds
}
#[derive(Deserialize, Default)]
struct AdditionalLimit {
    #[serde(default)]
    limit_name: Option<String>,
    #[serde(default)]
    rate_limit: Option<RateLimit>,
}
#[derive(Deserialize, Default)]
struct Credits {
    #[serde(default)]
    balance: Option<String>,
    #[serde(default)]
    has_credits: Option<bool>,
    #[serde(default)]
    unlimited: Option<bool>,
}
#[derive(Deserialize, Default)]
struct ResetCredits {
    #[serde(default)]
    available_count: Option<i64>,
}

pub struct CodexProvider {
    http: reqwest::Client,
}
impl Default for CodexProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CodexProvider {
    pub fn new() -> Self {
        Self {
            http: http::shared(),
        }
    }
}

fn read_auth() -> Result<(Value, Tokens), ProviderError> {
    let v = secrets::read_json_file(&secrets::codex_auth_path())?;
    let a: AuthDotJson = serde_json::from_value(v.clone())
        .map_err(|e| ProviderError::Parse(format!("codex auth: {e}")))?;
    let tokens = a.tokens.ok_or_else(|| {
        ProviderError::NotLoggedIn("Codex not logged in (run `codex login`)".into())
    })?;
    Ok((v, tokens))
}

fn write_auth(v: &Value) -> Result<(), ProviderError> {
    write_auth_to(&secrets::codex_auth_path(), v)
}

fn write_auth_to(path: &Path, v: &Value) -> Result<(), ProviderError> {
    let text = serde_json::to_string_pretty(v)
        .map_err(|e| ProviderError::Parse(format!("codex auth write: {e}")))?;
    // Atomic + owner-only: a crash/ENOSPC mid-write must not corrupt the CLI's
    // auth.json — by the time we get here the old refresh token is already
    // rotated away server-side, so a torn file would strand both tools (B-5).
    crate::util::write_atomic(path, text.as_bytes(), Some(0o600))
        .map_err(|e| ProviderError::Network(format!("write {}: {e}", path.display())))
}

fn access_needs_refresh(access_token: &str, now_sec: i64) -> bool {
    crate::jwt::jwt_exp(access_token)
        .map(|exp| exp <= now_sec + 300)
        .unwrap_or(false)
}

fn apply_auth_refresh(mut auth: Value, fresh: &Refreshed, refreshed_at: &str) -> Option<Value> {
    let access = fresh.access_token.as_ref()?;
    let tokens = auth.get_mut("tokens")?.as_object_mut()?;
    tokens.insert("access_token".into(), serde_json::json!(access));
    if let Some(id_token) = &fresh.id_token {
        tokens.insert("id_token".into(), serde_json::json!(id_token));
    }
    if let Some(refresh_token) = &fresh.refresh_token {
        tokens.insert("refresh_token".into(), serde_json::json!(refresh_token));
    }
    if let Some(obj) = auth.as_object_mut() {
        obj.insert("last_refresh".into(), serde_json::json!(refreshed_at));
    }
    Some(auth)
}

fn push_window(ws: &mut Vec<LimitWindow>, label: &str, w: &Option<RateWindow>) {
    if let Some(w) = w {
        ws.push(LimitWindow {
            label: label.into(),
            used_percent: w.used_percent.map(|x| x as f32),
            resets_at: w.reset_at,
            used: None,
            limit: None,
        });
    }
}

/// Pure: primary windows (5h/Weekly) + detail windows (Spark / code-review / credits / resets).
fn normalize(u: &WhamUsage) -> (Vec<LimitWindow>, Vec<LimitWindow>) {
    let mut ws = Vec::new();
    let mut detail = Vec::new();
    if let Some(rl) = &u.rate_limit {
        push_window(&mut ws, "5-hour", &rl.primary_window);
        push_window(&mut ws, "Weekly", &rl.secondary_window);
    }
    for al in &u.additional_rate_limits {
        let Some(rl) = &al.rate_limit else { continue };
        let name = al.limit_name.clone().unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        push_window(&mut detail, &format!("{name} · 5-hour"), &rl.primary_window);
        push_window(
            &mut detail,
            &format!("{name} · Weekly"),
            &rl.secondary_window,
        );
    }
    if let Some(rl) = &u.code_review_rate_limit {
        push_window(&mut detail, "Code review · 5-hour", &rl.primary_window);
        push_window(&mut detail, "Code review · Weekly", &rl.secondary_window);
    }
    if let Some(c) = &u.credits {
        let bal = c.balance.as_deref().and_then(|b| b.parse::<f64>().ok());
        if c.unlimited == Some(true) {
            detail.push(LimitWindow {
                label: "Credits (unlimited)".into(),
                used_percent: None,
                resets_at: None,
                used: None,
                limit: None,
            });
        } else if c.has_credits == Some(true) || matches!(bal, Some(v) if v > 0.0) {
            if let Some(v) = bal {
                // This is the *remaining* balance; keep it in the label rather
                // than `used` (which means consumption everywhere else) so the
                // semantics aren't inverted if the UI ever derives a percent or
                // `remaining = limit - used` (B-15).
                detail.push(LimitWindow {
                    label: format!("Credits balance: ${v:.2}"),
                    used_percent: None,
                    resets_at: None,
                    used: None,
                    limit: None,
                });
            }
        }
    }
    if let Some(r) = &u.rate_limit_reset_credits {
        if let Some(n) = r.available_count {
            detail.push(LimitWindow {
                label: "Available rate-limit resets".into(),
                used_percent: None,
                resets_at: None,
                used: Some(n as f64),
                limit: None,
            });
        }
    }
    (ws, detail)
}

#[async_trait]
impl crate::providers::ProviderApi for CodexProvider {
    fn key(&self) -> Provider {
        Provider::Codex
    }

    async fn fetch(&self) -> Result<ServiceUsage, ProviderError> {
        let (auth, mut t) = read_auth()?;
        if access_needs_refresh(&t.access_token, chrono::Utc::now().timestamp()) {
            let rt = t
                .refresh_token
                .as_ref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| ProviderError::Expired(
                    "Codex token expired and auth.json has no refresh_token — run `codex login`.".into(),
                ))?;
            let fresh = refresh_oauth(&self.http, rt).await?;
            if fresh.access_token.is_none() {
                return Err(ProviderError::Parse(
                    "codex refresh: missing access_token".into(),
                ));
            }
            if let Some(updated) =
                apply_auth_refresh(auth, &fresh, &chrono::Utc::now().to_rfc3339())
            {
                if let Err(e) = write_auth(&updated) {
                    eprintln!("codex: failed to persist refreshed auth.json: {e}");
                }
                if let Some(access_token) = fresh.access_token {
                    t.access_token = access_token;
                }
                if let Some(id_token) = fresh.id_token {
                    t.id_token = Some(id_token);
                }
                if let Some(refresh_token) = fresh.refresh_token {
                    t.refresh_token = Some(refresh_token);
                }
            }
        }
        fetch_with(&self.http, &t.access_token, t.account_id.as_deref(), None).await
    }
}

/// Fetch Codex usage given an explicit token (used for manually-added accounts).
pub(crate) async fn fetch_with(
    http: &reqwest::Client,
    access_token: &str,
    account_id: Option<&str>,
    label_override: Option<&str>,
) -> Result<ServiceUsage, ProviderError> {
    let mut extra: Vec<(&str, &str)> = vec![("User-Agent", CODEX_UA)];
    let acc_holder;
    if let Some(acc) = account_id {
        acc_holder = acc.to_string();
        extra.push(("ChatGPT-Account-Id", acc_holder.as_str()));
    }
    let raw: Value = http::get_json(http, access_token, USAGE_URL, &extra).await?;
    let raw_json = serde_json::to_string_pretty(&raw).ok();
    let u: WhamUsage = serde_json::from_value(raw)
        .map_err(|e| ProviderError::Parse(format!("codex usage: {e}")))?;
    let plan = u.plan_type.as_deref().map(crate::util::capitalize);
    let account = match label_override {
        Some(l) => Some(l.to_string()),
        None => u.email.clone(),
    };
    let (windows, detail_windows) = normalize(&u);
    Ok(ServiceUsage {
        id: auto_service_id(Provider::Codex),
        source: ServiceSource::Auto,
        provider: Provider::Codex,
        connected: true,
        plan,
        account,
        error: None,
        windows,
        detail_windows,
        raw_response: raw_json,
    })
}

/// Successful OAuth refresh response from `auth.openai.com/oauth/token`
/// (matches codex-rs/login `RefreshResponse`). `id_token`/`refresh_token`
/// may be omitted; `access_token` is always present on a successful refresh.
#[derive(serde::Deserialize)]
struct Refreshed {
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
    crate::store::rotate_credential(
        cred,
        new_access,
        fresh.refresh_token.clone(),
        fresh.id_token.clone(),
        expires_at,
    )
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
pub(crate) async fn refresh_stored(
    http: &reqwest::Client,
    cred: &crate::store::StoredCredential,
) -> Option<crate::store::StoredCredential> {
    let rt = cred.refresh_token.as_ref().filter(|s| !s.is_empty())?;
    let fresh = refresh_oauth(http, rt).await.ok()?;
    let _ = fresh.access_token.as_ref()?;
    Some(build_refreshed_cred(cred, &fresh))
}

/// Fetch usage for a stored Codex account (uniform stored-fetch adapter).
pub(crate) async fn fetch_stored(
    http: &reqwest::Client,
    cred: &crate::store::StoredCredential,
) -> Result<ServiceUsage, ProviderError> {
    fetch_with(
        http,
        &cred.access_token,
        cred.account_id.as_deref(),
        Some(&cred.label),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn write_auth_persists_atomically_and_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("ait_codex_auth_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("auth.json");
        let v = serde_json::json!({"tokens":{"access_token":"a","refresh_token":"r"}});

        write_auth_to(&path, &v).unwrap();

        // Round-trips as valid JSON (not torn).
        let back: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back["tokens"]["access_token"], "a");
        // Credential file → owner-only (B-5 / consistency with X-1).
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        // The temp file is consumed by the rename.
        assert!(!dir.join("auth.json.tmp").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn normalize_wham_fixture_includes_spark_and_credits() {
        let u: WhamUsage =
            serde_json::from_str(include_str!("../../tests/codex_wham_fixture.json")).unwrap();
        let (ws, detail) = normalize(&u);
        let labels: Vec<&str> = ws.iter().map(|w| w.label.as_str()).collect();
        assert_eq!(labels, vec!["5-hour", "Weekly"]); // primary only
        let dlabels: Vec<&str> = detail.iter().map(|w| w.label.as_str()).collect();
        assert!(dlabels
            .iter()
            .any(|l| l.contains("Spark") && l.contains("5-hour")));
        // The *remaining* credits balance lives in the label, not the `used`
        // field (which everywhere else means consumption) — B-15.
        let credits = detail
            .iter()
            .find(|w| w.label.starts_with("Credits balance"))
            .unwrap();
        assert_eq!(credits.label, "Credits balance: $9.99");
        assert_eq!(credits.used, None);
        assert!(dlabels.contains(&"Available rate-limit resets"));
        let five = ws.iter().find(|w| w.label == "5-hour").unwrap();
        assert_eq!(five.used_percent, Some(1.0));
    }

    #[test]
    fn reads_tokens_from_fixture() {
        let v: Value =
            serde_json::from_str(include_str!("../../tests/codex_auth_fixture.json")).unwrap();
        let a: AuthDotJson = serde_json::from_value(v).unwrap();
        let tokens = a.tokens.unwrap();
        assert_eq!(tokens.refresh_token.as_deref(), Some("r"));
    }

    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;

    fn jwt_with_exp(exp: i64) -> String {
        let payload = URL_SAFE_NO_PAD.encode(format!(r#"{{"exp":{exp}}}"#));
        format!("hdr.{payload}.sig")
    }

    fn stored(id: &str) -> crate::store::StoredCredential {
        crate::store::StoredCredential {
            id: id.into(),
            provider: Provider::Codex,
            label: "me@x.com".into(),
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
        assert_eq!(out.label, "me@x.com");
        assert_eq!(out.account_id.as_deref(), Some("acct-1"));
        assert_eq!(out.access_token, access_token);
        assert_eq!(out.refresh_token.as_deref(), Some("new.rt"));
        assert_eq!(out.id_token.as_deref(), Some("new.id"));
        assert_eq!(out.expires_at, 1_700_000_000_000); // exp → epoch ms
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
    fn access_refresh_is_only_for_jwt_near_expiry() {
        assert!(access_needs_refresh(&jwt_with_exp(1_000), 800));
        assert!(!access_needs_refresh(&jwt_with_exp(2_000), 800));
        assert!(!access_needs_refresh("opaque.access.token", 800));
    }

    #[test]
    fn apply_auth_refresh_updates_tokens_and_last_refresh() {
        let auth: Value =
            serde_json::from_str(include_str!("../../tests/codex_auth_fixture.json")).unwrap();
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
