//! Claude (via Claude Code) — modeled on claude-meter (MIT), with deeper
//! parsing for the detail view. Reads the OAuth token Claude Code stores
//! (macOS Keychain `Claude Code-credentials`, else `~/.claude/.credentials.json`),
//! checks expiry, then calls `api.anthropic.com/api/oauth/usage` + `/profile`.
//! Surfaces every rolling window (five_hour, seven_day + per-model + cowork +
//! omelette) and `extra_usage`, and derives a human plan label like "Max 20x"
//! from `rateLimitTier`.
//!
//! Refresh: self-refreshes the OAuth token (the usage-API 429 is per access
//! token, see anthropics/claude-code#31021) against console.anthropic.com with
//! a platform.claude.com fallback, and exposes `refresh_stored` for stored
//! OAuth accounts. Session-key accounts (`fetch_with_session_key`) carry no
//! refresh_token, so `refresh_stored` returns None for them.

use async_trait::async_trait;
use serde::Deserialize;

use crate::http;
use crate::model::{LimitWindow, Provider, ServiceUsage};
use crate::providers::ProviderError;
use crate::secrets;

const API_BASE: &str = "https://api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// The credential blob (keychain value or file content).
#[derive(Deserialize)]
struct CredBlob {
    #[serde(rename = "claudeAiOauth")]
    oauth: Option<OAuthCreds>,
    // legacy flat shape fallback
    #[serde(rename = "accessToken", default)] flat_access: Option<String>,
    #[serde(rename = "expiresAt", default)] flat_expires: i64,
    #[serde(rename = "subscriptionType", default)] flat_sub: Option<String>,
    #[serde(rename = "rateLimitTier", default)] flat_tier: Option<String>,
}

#[derive(Deserialize)]
struct OAuthCreds {
    #[serde(rename = "accessToken")] access_token: String,
    #[serde(rename = "refreshToken", default)] refresh_token: Option<String>,
    #[serde(rename = "expiresAt", default)] expires_at: i64,
    #[serde(rename = "subscriptionType", default)] subscription_type: Option<String>,
    #[serde(rename = "rateLimitTier", default)] rate_limit_tier: Option<String>,
}

struct ResolvedCreds {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: i64,
    subscription_type: Option<String>,
    rate_limit_tier: Option<String>,
}

fn resolve_creds(blob: serde_json::Value) -> Result<ResolvedCreds, ProviderError> {
    let parsed: CredBlob =
        serde_json::from_value(blob).map_err(|e| ProviderError::Parse(format!("claude creds: {e}")))?;
    if let Some(o) = parsed.oauth {
        return Ok(ResolvedCreds {
            access_token: o.access_token,
            refresh_token: o.refresh_token,
            expires_at: o.expires_at,
            subscription_type: o.subscription_type,
            rate_limit_tier: o.rate_limit_tier,
        });
    }
    parsed
        .flat_access
        .map(|access_token| ResolvedCreds {
            access_token,
            refresh_token: None,
            expires_at: parsed.flat_expires,
            subscription_type: parsed.flat_sub,
            rate_limit_tier: parsed.flat_tier,
        })
        .ok_or_else(|| ProviderError::Parse("claude creds: no accessToken".into()))
}

/// Build a human plan label like "Max 20x" from `rateLimitTier`
/// (e.g. "default_claude_max_20x" → "Max 20x"). Falls back to the subscription
/// type capitalized.
fn format_plan(tier: &Option<String>, sub: &Option<String>) -> Option<String> {
    fn cap(s: &str) -> String {
        let mut c = s.chars();
        match c.next() {
            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            None => String::new(),
        }
    }
    if let Some(t) = tier {
        let lower = t.to_lowercase();
        let toks: Vec<&str> = lower.split('_').collect();
        let base = if toks.iter().any(|x| x.contains("max")) {
            "Max"
        } else if toks.iter().any(|x| *x == "pro" || x.contains("pro")) {
            "Pro"
        } else if toks.iter().any(|x| x.contains("team")) {
            "Team"
        } else if toks.iter().any(|x| x.contains("enterprise")) {
            "Enterprise"
        } else {
            return sub.as_deref().map(cap);
        };
        let mult = toks.iter().rev().find(|x| {
            x.ends_with('x')
                && x[..x.len() - 1].chars().all(|c| c.is_ascii_digit())
                && x.len() > 1
        });
        return Some(match mult {
            Some(m) => format!("{base} {m}"),
            None => base.to_string(),
        });
    }
    sub.as_deref().map(cap)
}

#[derive(Deserialize)]
struct Window {
    #[serde(default)] utilization: Option<f64>,
    #[serde(default)] resets_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Deserialize)]
struct ExtraUsage {
    #[serde(default)] is_enabled: Option<bool>,
    #[serde(default)] monthly_limit: Option<f64>,
    #[serde(default)] used_credits: Option<f64>,
    #[serde(default)] utilization: Option<f64>,
}

#[derive(Deserialize, Default)]
struct UsageResponse {
    #[serde(default)] five_hour: Option<Window>,
    #[serde(default)] seven_day: Option<Window>,
    #[serde(default)] seven_day_sonnet: Option<Window>,
    #[serde(default)] seven_day_opus: Option<Window>,
    #[serde(default)] seven_day_oauth_apps: Option<Window>,
    #[serde(default)] seven_day_omelette: Option<Window>,
    #[serde(default)] seven_day_cowork: Option<Window>,
    #[serde(default)] extra_usage: Option<ExtraUsage>,
}

#[derive(Deserialize, Default)]
struct Profile {
    #[serde(default)] account: Option<ProfileAccount>,
}
#[derive(Deserialize, Default)]
struct ProfileAccount {
    #[serde(default)] email: Option<String>,
}

pub struct ClaudeProvider {
    http: reqwest::Client,
}

impl Default for ClaudeProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeProvider {
    pub fn new() -> Self {
        Self {
            http: http::build_client(),
        }
    }
}

fn window(label: &str, w: &Window) -> LimitWindow {
    LimitWindow {
        label: label.into(),
        used_percent: w.utilization.map(|v| v as f32),
        resets_at: w.resets_at.map(|d| d.timestamp()),
        used: None,
        limit: None,
    }
}

/// Pure normalization (unit-testable, no network). utilization is already 0..100;
/// extra_usage credits are cents → dollars.
fn normalize(raw: &UsageResponse) -> (Vec<LimitWindow>, Vec<LimitWindow>) {
    let mut ws = Vec::new();
    let mut detail = Vec::new();
    // Primary (card): the two headline rolling windows.
    if let Some(w) = &raw.five_hour {
        ws.push(window("5-hour", w));
    }
    if let Some(w) = &raw.seven_day {
        ws.push(window("7-day", w));
    }
    // Detail (modal only): per-model windows + extra usage.
    if let Some(w) = &raw.seven_day_sonnet {
        detail.push(window("7-day (Sonnet)", w));
    }
    if let Some(w) = &raw.seven_day_opus {
        detail.push(window("7-day (Opus)", w));
    }
    if let Some(w) = &raw.seven_day_oauth_apps {
        detail.push(window("7-day (OAuth Apps)", w));
    }
    if let Some(w) = &raw.seven_day_omelette {
        detail.push(window("7-day (Omelette)", w));
    }
    if let Some(w) = &raw.seven_day_cowork {
        detail.push(window("7-day (Cowork)", w));
    }
    if let Some(e) = &raw.extra_usage {
        if e.is_enabled.unwrap_or(false) {
            detail.push(LimitWindow {
                label: "Extra usage".into(),
                used_percent: e.utilization.map(|v| v as f32),
                resets_at: None,
                used: e.used_credits.map(|c| c / 100.0),
                limit: e.monthly_limit.map(|c| c / 100.0),
            });
        }
    }
    (ws, detail)
}

const CLAUDE_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5942d1962f5e";
const CLAUDE_TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";
// Newer Claude Code builds are migrating to platform.claude.com; console is
// still authoritative today (verified via anthropics/claude-code#31021 + the
// cedws/3a24b2c gist). Try console first, fall back to platform on a hard
// failure so a future endpoint cutover doesn't strand stored accounts.
const CLAUDE_TOKEN_URL_FALLBACK: &str = "https://platform.claude.com/v1/oauth/token";

#[derive(serde::Deserialize)]
struct Refreshed {
    access_token: String,
    refresh_token: String,
    #[serde(default)] expires_in: Option<u64>,
}

/// Body for the OAuth `refresh_token` grant, built from the public Claude Code
/// client_id (pure / unit-tested). Refresh tokens rotate on success, so every
/// caller MUST persist the returned access+refresh pair.
fn refresh_request_body(rt: &str) -> serde_json::Value {
    serde_json::json!({
        "grant_type": "refresh_token",
        "refresh_token": rt,
        "client_id": CLAUDE_CLIENT_ID,
    })
}

async fn post_refresh(
    http: &reqwest::Client,
    url: &str,
    body: &serde_json::Value,
) -> Result<Refreshed, ProviderError> {
    let resp = http
        .post(url)
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(ProviderError::Status {
            status: status.as_u16(),
            body: text.chars().take(200).collect(),
        });
    }
    serde_json::from_str::<Refreshed>(&text)
        .map_err(|e| ProviderError::Parse(format!("refresh: {e}")))
}

/// Refresh the OAuth token using the public Claude Code client_id. The usage-API
/// 429 is per access token (see anthropics/claude-code#31021), so a fresh token
/// reopens the rate-limit window. Refresh tokens rotate — callers write the new
/// pair back so the CLI and app stay in sync. Tries console.anthropic.com first,
/// then platform.claude.com on a hard (network / non-2xx) failure.
async fn refresh_oauth(http: &reqwest::Client, rt: &str) -> Result<Refreshed, ProviderError> {
    let body = refresh_request_body(rt);
    match post_refresh(http, CLAUDE_TOKEN_URL, &body).await {
        Ok(r) => Ok(r),
        Err(e) => {
            // Retry ONLY when console could not have refreshed (network error
            // or non-2xx). A 2xx parse error is excluded on purpose: a
            // successful-looking response may already have rotated the
            // refresh_token server-side, and reusing it on the fallback would
            // burn the (single-use) token.
            let retry = matches!(e, ProviderError::Network(_) | ProviderError::Status { .. });
            if retry {
                if let Ok(r) = post_refresh(http, CLAUDE_TOKEN_URL_FALLBACK, &body).await {
                    return Ok(r);
                }
            }
            Err(e)
        }
    }
}

/// Pure core of `refresh_stored`: stamp a stored credential with the rotated
/// tokens. `now_ms` is injected so the expiry math is deterministic under test.
/// Unchanged fields (`id`/`provider`/`label`/`id_token`/`account_id`) are
/// preserved; `expires_at` is 0 when the server omits `expires_in`.
fn apply_refresh(
    cred: &crate::store::StoredCredential,
    fresh: Refreshed,
    now_ms: i64,
) -> crate::store::StoredCredential {
    let expires_at = fresh.expires_in.map(|s| now_ms + (s as i64) * 1000).unwrap_or(0);
    crate::store::StoredCredential {
        id: cred.id.clone(),
        provider: cred.provider,
        label: cred.label.clone(),
        access_token: fresh.access_token,
        refresh_token: Some(fresh.refresh_token),
        expires_at,
        id_token: cred.id_token.clone(),
        account_id: cred.account_id.clone(),
    }
}

/// Refresh a stored credential's access_token using its refresh_token.
/// Returns Some(updated_cred) if a refresh happened (the caller persists it);
/// None when it does not apply — no refresh_token (session-key / API-key
/// accounts) or the refresh failed (caller falls back to the existing token).
pub(crate) async fn refresh_stored(
    http: &reqwest::Client,
    cred: &crate::store::StoredCredential,
) -> Option<crate::store::StoredCredential> {
    let rt = cred.refresh_token.as_deref()?;
    let fresh = refresh_oauth(http, rt).await.ok()?;
    Some(apply_refresh(
        cred,
        fresh,
        chrono::Utc::now().timestamp_millis(),
    ))
}

fn write_back(
    orig: &serde_json::Value,
    access_token: &str,
    refresh_token: &str,
    expires_at: i64,
) -> Result<(), ProviderError> {
    let mut blob = orig.clone();
    let target = if let Some(o) = blob.get_mut("claudeAiOauth").and_then(|v| v.as_object_mut()) {
        o
    } else if let Some(obj) = blob.as_object_mut() {
        obj
    } else {
        return Err(ProviderError::Parse("claude creds: cannot write back".into()));
    };
    target.insert("accessToken".into(), serde_json::json!(access_token));
    target.insert("refreshToken".into(), serde_json::json!(refresh_token));
    target.insert("expiresAt".into(), serde_json::json!(expires_at));
    let s = serde_json::to_string(&blob).map_err(|e| ProviderError::Parse(e.to_string()))?;
    write_creds(&s)
}

fn write_creds(s: &str) -> Result<(), ProviderError> {
    #[cfg(target_os = "macos")]
    {
        // Match the account of the EXISTING entry so `add-generic-password -U`
        // updates it in place. Claude Code writes under the OS username
        // (verified live: acct == $USER), but if a pre-existing entry lives
        // under a different account (shared machine, renamed user), blindly
        // using $USER would fork into a duplicate that `secrets::read_macos_keychain`
        // (which reads any account) might not surface. Fall back to $USER on
        // first write when no entry exists yet.
        let acct =
            existing_keychain_account().unwrap_or_else(|| std::env::var("USER").unwrap_or_default());
        let out = std::process::Command::new("/usr/bin/security")
            .args(["add-generic-password", "-s", "Claude Code-credentials", "-a", &acct, "-w", s, "-U"])
            .output()
            .map_err(|e| ProviderError::Network(format!("security write: {e}")))?;
        if !out.status.success() {
            return Err(ProviderError::Network(format!(
                "security write: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let p = crate::secrets::claude_token_path();
        std::fs::write(&p, s).map_err(|e| ProviderError::Network(format!("write {}: {e}", p.display())))?;
        Ok(())
    }
}

/// Best-effort `acct` attribute of the existing `Claude Code-credentials`
/// keychain item — i.e. the exact entry `secrets::read_macos_keychain` reads.
/// None if there is no item or `security` can't run (caller falls back to $USER).
#[cfg(target_os = "macos")]
fn existing_keychain_account() -> Option<String> {
    let out = std::process::Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", "Claude Code-credentials"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    // `security find-generic-password -s X` (no -w/-g) prints attributes
    // without prompting; the account line looks like:  "acct"<blob>="yakisoba"
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines().find_map(|l| {
        let l = l.trim();
        let after = l.strip_prefix("\"acct\"<blob>=\"")?;
        after
            .strip_suffix('"')
            .map(str::to_string)
            .filter(|s| !s.is_empty())
    })
}

#[async_trait]
impl crate::providers::ProviderApi for ClaudeProvider {
    fn key(&self) -> Provider {
        Provider::Claude
    }

    async fn fetch(&self) -> Result<ServiceUsage, ProviderError> {
        let blob = secrets::read_claude_creds_json()?;
        let mut creds = resolve_creds(blob.clone())?;
        let now_ms = chrono::Utc::now().timestamp_millis();

        // Refresh on expiry — rotates the refresh_token; writes back so the CLI
        // and app stay in sync.
        if creds.expires_at > 0 && creds.expires_at < now_ms {
            match creds.refresh_token.clone() {
                Some(rt) => match refresh_oauth(&self.http, &rt).await {
                    Ok(fresh) => {
                        let exp = fresh.expires_in.map(|s| now_ms + (s as i64) * 1000).unwrap_or(0);
                        let _ = write_back(&blob, &fresh.access_token, &fresh.refresh_token, exp);
                        creds.access_token = fresh.access_token;
                        creds.refresh_token = Some(fresh.refresh_token);
                    }
                    Err(_) => {
                        return Err(ProviderError::Expired(
                            "Claude token expired and refresh failed (rate-limited?)".into(),
                        ))
                    }
                },
                None => {
                    return Err(ProviderError::Expired(
                        "Claude Code token expired — no refresh_token; run `claude` once".into(),
                    ))
                }
            }
        }

        match fetch_with(
            &self.http,
            &creds.access_token,
            format_plan(&creds.rate_limit_tier, &creds.subscription_type),
            None,
        )
        .await
        {
            // usage 429 is per-access-token — refresh once for a fresh window, then retry.
            Err(ProviderError::Status { status: 429, .. }) => {
                let rt = creds
                    .refresh_token
                    .clone()
                    .ok_or_else(|| ProviderError::Expired("no refresh_token".into()))?;
                let fresh = refresh_oauth(&self.http, &rt).await?;
                let exp = fresh
                    .expires_in
                    .map(|s| chrono::Utc::now().timestamp_millis() + (s as i64) * 1000)
                    .unwrap_or(0);
                let _ = write_back(&blob, &fresh.access_token, &fresh.refresh_token, exp);
                fetch_with(
                    &self.http,
                    &fresh.access_token,
                    format_plan(&creds.rate_limit_tier, &creds.subscription_type),
                    None,
                )
                .await
            }
            other => other,
        }
    }
}

/// Fetch Claude usage given an explicit access token (manually-added account).
pub(crate) async fn fetch_with(
    http: &reqwest::Client,
    access_token: &str,
    plan: Option<String>,
    account_override: Option<String>,
) -> Result<ServiceUsage, ProviderError> {
    let h = [("anthropic-version", ANTHROPIC_VERSION)];
    let usage: UsageResponse =
        http::get_json(http, access_token, &format!("{API_BASE}/api/oauth/usage"), &h).await?;
    let profile: Profile =
        http::get_json(http, access_token, &format!("{API_BASE}/api/oauth/profile"), &h).await?;
    let (windows, detail_windows) = normalize(&usage);
    Ok(ServiceUsage {
        provider: Provider::Claude,
        connected: true,
        plan,
        account: account_override.or_else(|| profile.account.and_then(|a| a.email)),
        error: None,
        windows,
        detail_windows,
        raw_response: None,
    })
}

/// Fetch Claude usage via the claude.ai web API using a `sessionKey` cookie
/// (manually-added session-key account). No OAuth / CLI involved.
///
/// Shape verified against the live `/api/organizations` response: each org
/// carries `rate_limit_tier` directly and `memberships[].account.email_address`
/// inline, so no separate `/account` round-trip is needed (the shape that call
/// used to assume was wrong and always degraded to no email / no tier).
#[derive(Deserialize)]
struct WebOrg {
    uuid: String, // the identifier the usage endpoint expects
    #[serde(default)] name: Option<String>,
    #[serde(default)] rate_limit_tier: Option<String>,
    #[serde(default)] memberships: Vec<WebMembership>,
}
#[derive(Deserialize)]
struct WebMembership {
    #[serde(default)] account: Option<WebMemberAccount>,
}
#[derive(Deserialize)]
struct WebMemberAccount {
    #[serde(default, rename = "email_address")] email_address: Option<String>,
}
#[derive(Deserialize)]
struct WebWindow {
    #[serde(default)] utilization: Option<f64>, // claude.ai returns int percent 0-100
    #[serde(default)] resets_at: Option<chrono::DateTime<chrono::Utc>>,
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "snake_case")]
struct WebUsage {
    #[serde(default)] five_hour: Option<WebWindow>,
    #[serde(default)] seven_day: Option<WebWindow>,
    #[serde(default)] seven_day_sonnet: Option<WebWindow>,
    #[serde(default)] seven_day_opus: Option<WebWindow>,
}

pub(crate) async fn fetch_with_session_key(
    http: &reqwest::Client,
    session_key: &str,
) -> Result<ServiceUsage, ProviderError> {
    let cookie = format!("sessionKey={session_key}");

    let resp = http
        .get("https://claude.ai/api/organizations")
        .header("Cookie", &cookie)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    let v: serde_json::Value = crate::http::send_for_json(resp, "claude.ai/organizations").await?;
    let orgs: Vec<WebOrg> =
        serde_json::from_value(v).map_err(|e| ProviderError::Parse(format!("orgs: {e}")))?;
    let org = orgs
        .into_iter()
        .next()
        .ok_or_else(|| ProviderError::Parse("no claude organization".into()))?;

    let resp = http
        .get(format!("https://claude.ai/api/organizations/{}/usage", org.uuid))
        .header("Cookie", &cookie)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    let v: serde_json::Value = crate::http::send_for_json(resp, "claude.ai/usage").await?;
    let u: WebUsage =
        serde_json::from_value(v).map_err(|e| ProviderError::Parse(format!("usage: {e}")))?;

    // Email + plan tier come straight off the org object we already fetched.
    let email = org
        .memberships
        .iter()
        .find_map(|m| m.account.as_ref())
        .and_then(|a| a.email_address.clone());

    let win = |label: &str, w: &Option<WebWindow>| -> Option<LimitWindow> {
        w.as_ref().map(|w| LimitWindow {
            label: label.into(),
            used_percent: w.utilization.map(|v| v as f32),
            resets_at: w.resets_at.map(|d| d.timestamp()),
            used: None,
            limit: None,
        })
    };
    let mut windows = vec![];
    let mut detail = vec![];
    if let Some(x) = win("5-hour", &u.five_hour) {
        windows.push(x);
    }
    if let Some(x) = win("7-day", &u.seven_day) {
        windows.push(x);
    }
    if let Some(x) = win("7-day (Sonnet)", &u.seven_day_sonnet) {
        detail.push(x);
    }
    if let Some(x) = win("7-day (Opus)", &u.seven_day_opus) {
        detail.push(x);
    }
    Ok(ServiceUsage {
        provider: Provider::Claude,
        connected: true,
        plan: format_plan(&org.rate_limit_tier, &None).or(org.name),
        account: email,
        error: None,
        windows,
        detail_windows: detail,
        raw_response: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_fixture() {
        let raw: UsageResponse =
            serde_json::from_str(include_str!("../../tests/claude_fixture.json")).unwrap();
        let (ws, detail) = normalize(&raw);
        let labels: Vec<&str> = ws.iter().map(|w| w.label.as_str()).collect();
        assert!(labels.contains(&"5-hour"));
        assert!(labels.contains(&"7-day"));
        let five = ws.iter().find(|w| w.label == "5-hour").unwrap();
        assert_eq!(five.used_percent, Some(23.5)); // utilization is already 0..100
        assert!(five.resets_at.is_some());
        let dlabels: Vec<&str> = detail.iter().map(|w| w.label.as_str()).collect();
        assert!(dlabels.contains(&"Extra usage"));
        let extra = detail.iter().find(|w| w.label == "Extra usage").unwrap();
        assert_eq!(extra.used, Some(12.5)); // 1250 cents -> $12.50
        assert_eq!(extra.limit, Some(100.0)); // 10000 cents -> $100.00
        assert_eq!(extra.used_percent, Some(12.5));
    }

    #[test]
    fn format_plan_from_tier() {
        assert_eq!(
            format_plan(&Some("default_claude_max_20x".into()), &Some("max".into())).as_deref(),
            Some("Max 20x")
        );
        assert_eq!(
            format_plan(&Some("default_claude_max_5x".into()), &Some("max".into())).as_deref(),
            Some("Max 5x")
        );
        assert_eq!(
            format_plan(&Some("default_claude_pro".into()), &Some("pro".into())).as_deref(),
            Some("Pro")
        );
        assert_eq!(format_plan(&None, &Some("max".into())).as_deref(), Some("Max"));
        assert_eq!(format_plan(&None, &None), None);
    }

    #[test]
    fn resolve_nested_and_flat() {
        let nested = serde_json::json!({"claudeAiOauth":{"accessToken":"a","expiresAt":0,"subscriptionType":"max","rateLimitTier":"default_claude_max_20x"}});
        let r = resolve_creds(nested).unwrap();
        assert_eq!(r.access_token, "a");
        assert_eq!(r.rate_limit_tier.as_deref(), Some("default_claude_max_20x"));

        let flat = serde_json::json!({"accessToken":"b","expiresAt":0});
        let r2 = resolve_creds(flat).unwrap();
        assert_eq!(r2.access_token, "b");
        assert!(r2.rate_limit_tier.is_none());
    }
    #[test]
    fn refresh_request_body_shape() {
        let b = refresh_request_body("rt-123");
        assert_eq!(b["grant_type"], "refresh_token");
        assert_eq!(b["refresh_token"], "rt-123");
        assert_eq!(b["client_id"], CLAUDE_CLIENT_ID);
        assert_eq!(b.as_object().unwrap().len(), 3);
    }

    #[test]
    fn apply_refresh_copies_and_rotates() {
        let cred = crate::store::StoredCredential {
            id: "id1".into(),
            provider: crate::model::Provider::Claude,
            label: "user@example.com".into(),
            access_token: "old-access".into(),
            refresh_token: Some("old-refresh".into()),
            expires_at: 1000,
            id_token: Some("idt".into()),
            account_id: Some("acct".into()),
        };
        let fresh = Refreshed {
            access_token: "new-access".into(),
            refresh_token: "new-refresh".into(),
            expires_in: Some(28800),
        };
        let out = apply_refresh(&cred, fresh, 1_000_000);
        // rotated
        assert_eq!(out.access_token, "new-access");
        assert_eq!(out.refresh_token.as_deref(), Some("new-refresh"));
        assert_eq!(out.expires_at, 1_000_000 + 28800 * 1000);
        // preserved
        assert_eq!(out.id, "id1");
        assert_eq!(out.provider, crate::model::Provider::Claude);
        assert_eq!(out.label, "user@example.com");
        assert_eq!(out.id_token.as_deref(), Some("idt"));
        assert_eq!(out.account_id.as_deref(), Some("acct"));
    }

    #[test]
    fn apply_refresh_unknown_expiry_when_missing() {
        let cred = crate::store::StoredCredential {
            id: "id1".into(),
            provider: crate::model::Provider::Claude,
            label: "x".into(),
            access_token: "old".into(),
            refresh_token: Some("r".into()),
            expires_at: 0,
            id_token: None,
            account_id: None,
        };
        let fresh = Refreshed {
            access_token: "a".into(),
            refresh_token: "r2".into(),
            expires_in: None,
        };
        let out = apply_refresh(&cred, fresh, 5_000_000);
        assert_eq!(out.expires_at, 0);
    }

    #[tokio::test]
    async fn refresh_stored_none_without_refresh_token() {
        // Session-key / API-key accounts have no refresh_token → None, no network.
        let cred = crate::store::StoredCredential {
            id: "id".into(),
            provider: crate::model::Provider::Claude,
            label: "x".into(),
            access_token: "a".into(),
            refresh_token: None,
            expires_at: 0,
            id_token: None,
            account_id: None,
        };
        let http = crate::http::build_client();
        assert!(refresh_stored(&http, &cred).await.is_none());
    }

    #[test]
    fn web_orgs_parse_tier_and_email() {
        let v: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/claude_web_orgs.json")).unwrap();
        let orgs: Vec<WebOrg> = serde_json::from_value(v).unwrap();
        let org = orgs.into_iter().next().unwrap();
        assert_eq!(org.uuid, "11111111-2222-3333-4444-555555555555");
        assert_eq!(org.rate_limit_tier.as_deref(), Some("default_claude_max_20x"));
        let email = org
            .memberships
            .iter()
            .find_map(|m| m.account.as_ref())
            .and_then(|a| a.email_address.clone());
        assert_eq!(email.as_deref(), Some("user@example.com"));
    }
}
