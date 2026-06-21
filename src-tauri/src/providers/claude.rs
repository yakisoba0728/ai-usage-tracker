//! Claude (via Claude Code) — reads the OAuth token from the macOS keychain
//! (or ~/.claude/.credentials.json), auto-refreshes via the public Claude Code
//! client_id against platform.claude.com/v1/oauth/token, then fetches usage from
//! api.anthropic.com/api/oauth/usage + /profile. Modeled on claude-meter (MIT).

use async_trait::async_trait;
use serde::Deserialize;

use crate::http;
use crate::model::{auto_service_id, LimitWindow, Provider, ServiceSource, ServiceUsage};
use crate::providers::ProviderError;

const API_BASE: &str = "https://api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2023-06-01";

struct ResolvedCreds {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: i64,
    subscription_type: Option<String>,
    rate_limit_tier: Option<String>,
}

fn resolve_creds(blob: serde_json::Value) -> Result<ResolvedCreds, ProviderError> {
    resolve_creds_value(&blob)
}

fn resolve_creds_value(blob: &serde_json::Value) -> Result<ResolvedCreds, ProviderError> {
    if let serde_json::Value::String(raw) = blob {
        if let Some(access_token) = crate::secrets::normalize_claude_oauth_token(raw) {
            return Ok(ResolvedCreds {
                access_token,
                refresh_token: None,
                expires_at: 0,
                subscription_type: None,
                rate_limit_tier: None,
            });
        }
        if raw.trim_start().starts_with('{') {
            let nested = serde_json::from_str::<serde_json::Value>(raw)
                .map_err(|e| ProviderError::Parse(format!("claude creds: {e}")))?;
            return resolve_creds_value(&nested);
        }
    }

    let obj = blob
        .as_object()
        .ok_or_else(|| ProviderError::Parse("claude creds: expected object".into()))?;

    for key in ["claudeAiOauth", "claude_ai_oauth", "oauth"] {
        if let Some(nested) = obj.get(key) {
            if let Ok(creds) = resolve_creds_value(nested) {
                return Ok(creds);
            }
        }
    }

    resolve_creds_object(obj)
}

fn resolve_creds_object(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Result<ResolvedCreds, ProviderError> {
    let access_token = string_field(obj, &["accessToken", "access_token"])
        .ok_or_else(|| ProviderError::Parse("claude creds: no accessToken".into()))?;
    Ok(ResolvedCreds {
        access_token,
        refresh_token: string_field(obj, &["refreshToken", "refresh_token"]),
        expires_at: millis_field(obj, &["expiresAt", "expires_at"])?,
        subscription_type: string_field(obj, &["subscriptionType", "subscription_type"]),
        rate_limit_tier: string_field(obj, &["rateLimitTier", "rate_limit_tier"]),
    })
}

fn string_field(obj: &serde_json::Map<String, serde_json::Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| match obj.get(*key) {
        Some(serde_json::Value::String(value)) if !value.trim().is_empty() => {
            Some(value.trim().to_string())
        }
        _ => None,
    })
}

fn millis_field(
    obj: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Result<i64, ProviderError> {
    let Some(value) = keys.iter().find_map(|key| obj.get(*key)) else {
        return Ok(0);
    };
    match value {
        serde_json::Value::Null => Ok(0),
        serde_json::Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_u64().and_then(|v| i64::try_from(v).ok()))
            .ok_or_else(|| ProviderError::Parse("claude creds: invalid expiresAt".into())),
        serde_json::Value::String(s) if s.trim().is_empty() => Ok(0),
        serde_json::Value::String(s) => s
            .trim()
            .parse::<i64>()
            .map_err(|e| ProviderError::Parse(format!("claude creds: expiresAt: {e}"))),
        _ => Err(ProviderError::Parse(
            "claude creds: expiresAt must be number or string".into(),
        )),
    }
}

/// Build a human plan label like "Max 20x" from `rateLimitTier`
/// (e.g. "default_claude_max_20x" → "Max 20x"). Falls back to the subscription
/// type capitalized.
fn format_plan(tier: &Option<String>, sub: &Option<String>) -> Option<String> {
    use crate::util::capitalize as cap;
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
            x.ends_with('x') && x[..x.len() - 1].chars().all(|c| c.is_ascii_digit()) && x.len() > 1
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
    #[serde(default)]
    utilization: Option<f64>,
    #[serde(default)]
    resets_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Deserialize)]
struct ExtraUsage {
    #[serde(default)]
    is_enabled: Option<bool>,
    #[serde(default)]
    monthly_limit: Option<f64>,
    #[serde(default)]
    used_credits: Option<f64>,
    #[serde(default)]
    utilization: Option<f64>,
}

#[derive(Deserialize, Default)]
struct UsageResponse {
    #[serde(default)]
    five_hour: Option<Window>,
    #[serde(default)]
    seven_day: Option<Window>,
    #[serde(default)]
    seven_day_sonnet: Option<Window>,
    #[serde(default)]
    seven_day_opus: Option<Window>,
    #[serde(default)]
    seven_day_oauth_apps: Option<Window>,
    #[serde(default)]
    seven_day_omelette: Option<Window>,
    #[serde(default)]
    seven_day_cowork: Option<Window>,
    #[serde(default)]
    extra_usage: Option<ExtraUsage>,
}

#[derive(Deserialize, Default)]
struct Profile {
    #[serde(default)]
    account: Option<ProfileAccount>,
}
#[derive(Deserialize, Default)]
struct ProfileAccount {
    #[serde(default)]
    email: Option<String>,
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
            http: http::shared(),
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

const CLAUDE_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const CLAUDE_TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
const CLAUDE_TOKEN_URL_FALLBACK: &str = "https://api.anthropic.com/v1/oauth/token";
/// OAuth scopes — MANDATORY in the refresh body (without it: HTTP 400
/// 'Invalid request format'). Extracted from Claude Code 2.1.181 binary.
const CLAUDE_OAUTH_SCOPES: &str =
    "user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";

#[derive(serde::Deserialize)]
struct Refreshed {
    access_token: String,
    refresh_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
}

/// Body for the OAuth `refresh_token` grant, built from the public Claude Code
/// client_id (pure / unit-tested). Refresh tokens rotate on success, so every
/// caller MUST persist the returned access+refresh pair.
fn refresh_request_body(rt: &str) -> serde_json::Value {
    serde_json::json!({
        "grant_type": "refresh_token",
        "refresh_token": rt,
        "client_id": CLAUDE_CLIENT_ID,
        "scope": CLAUDE_OAUTH_SCOPES,
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
    let v = http::send_for_json(resp, "claude refresh").await?;
    serde_json::from_value::<Refreshed>(v)
        .map_err(|e| ProviderError::Parse(format!("claude refresh: {e}")))
}

/// Refresh the OAuth token using the public Claude Code client_id. The usage-API
/// 429 is per access token (see anthropics/claude-code#31021), so a fresh token
/// reopens the rate-limit window. Refresh tokens rotate — callers write the new
/// pair back so the CLI and app stay in sync. Tries platform.claude.com first,
/// then api.anthropic.com on a hard (network / non-2xx) failure.
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
    let expires_at = fresh
        .expires_in
        .map(|s| now_ms + (s as i64) * 1000)
        .unwrap_or(0);
    crate::store::rotate_credential(
        cred,
        fresh.access_token,
        Some(fresh.refresh_token),
        None,
        expires_at,
    )
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

/// Fetch usage for a stored Claude account (uniform stored-fetch adapter).
/// Manual Claude accounts are session-key based, so this uses the claude.ai
/// web API path.
pub(crate) async fn fetch_stored(
    http: &reqwest::Client,
    cred: &crate::store::StoredCredential,
) -> Result<ServiceUsage, ProviderError> {
    fetch_with_session_key(http, &cred.access_token).await
}

fn write_back(
    orig: &serde_json::Value,
    access_token: &str,
    refresh_token: &str,
    expires_at: i64,
) -> Result<(), ProviderError> {
    let mut blob = orig.clone();
    let target = if let Some(o) = blob
        .get_mut("claudeAiOauth")
        .and_then(|v| v.as_object_mut())
    {
        o
    } else if let Some(obj) = blob.as_object_mut() {
        obj
    } else {
        return Err(ProviderError::Parse(
            "claude creds: cannot write back".into(),
        ));
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
        let acct = existing_keychain_account()
            .unwrap_or_else(|| std::env::var("USER").unwrap_or_default());
        let out = std::process::Command::new("/usr/bin/security")
            .args([
                "add-generic-password",
                "-s",
                "Claude Code-credentials",
                "-a",
                &acct,
                "-w",
                s,
                "-U",
            ])
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
        std::fs::write(&p, s)
            .map_err(|e| ProviderError::Network(format!("write {}: {e}", p.display())))?;
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
        let blob = crate::secrets::read_claude_creds_json()?;
        let mut creds = resolve_creds(blob.clone())?;
        let now_ms = chrono::Utc::now().timestamp_millis();
        // Auto-refresh when expired. The refresh rotates tokens and writes the
        // new pair back to the keychain so the CLI and app stay in sync.
        if creds.expires_at > 0 && creds.expires_at < now_ms {
            let rt = creds.refresh_token.clone().filter(|s| !s.is_empty());
            if let Some(rt) = rt {
                match refresh_oauth(&self.http, &rt).await {
                    Ok(fresh) => {
                        let exp = fresh
                            .expires_in
                            .map(|s| now_ms + (s as i64) * 1000)
                            .unwrap_or(0);
                        if let Err(e) =
                            write_back(&blob, &fresh.access_token, &fresh.refresh_token, exp)
                        {
                            eprintln!("claude: failed to persist refreshed token: {e}");
                        }
                        creds.access_token = fresh.access_token;
                        creds.refresh_token = Some(fresh.refresh_token);
                    }
                    Err(_) => {
                        return Err(ProviderError::Expired(
                            "Claude Code token expired and refresh failed — run `claude` to re-authenticate.".into(),
                        ));
                    }
                }
            } else {
                return Err(ProviderError::Expired(
                    "Claude Code token expired — run `claude` to refresh.".into(),
                ));
            }
        }
        // Fetch usage. Only refresh on an auth failure; a 429 is a real quota
        // signal and must not burn Claude Code's rotating refresh token.
        let plan = format_plan(&creds.rate_limit_tier, &creds.subscription_type);
        match fetch_with(&self.http, &creds.access_token, plan.clone(), None).await {
            Err(err @ ProviderError::Status { status, .. })
                if should_refresh_local_cli_after_usage_status(status) =>
            {
                let Some(rt) = creds.refresh_token.clone().filter(|s| !s.is_empty()) else {
                    return Err(err);
                };
                let fresh = refresh_oauth(&self.http, &rt).await?;
                let exp = fresh
                    .expires_in
                    .map(|s| now_ms + (s as i64) * 1000)
                    .unwrap_or(0);
                if let Err(e) = write_back(&blob, &fresh.access_token, &fresh.refresh_token, exp) {
                    eprintln!("claude: failed to persist refreshed token: {e}");
                }
                fetch_with(&self.http, &fresh.access_token, plan, None).await
            }
            other => other,
        }
    }
}

fn should_refresh_local_cli_after_usage_status(status: u16) -> bool {
    status == 401
}

/// Fetch Claude usage given an explicit access token (manually-added account).
pub(crate) async fn fetch_with(
    http: &reqwest::Client,
    access_token: &str,
    plan: Option<String>,
    account_override: Option<String>,
) -> Result<ServiceUsage, ProviderError> {
    let h = [("anthropic-version", ANTHROPIC_VERSION)];
    let usage_raw: serde_json::Value = http::get_json(
        http,
        access_token,
        &format!("{API_BASE}/api/oauth/usage"),
        &h,
    )
    .await?;
    let raw_json = serde_json::to_string_pretty(&usage_raw).ok();
    let usage: UsageResponse =
        serde_json::from_value(usage_raw).map_err(|e| ProviderError::Parse(e.to_string()))?;
    let profile: Profile = http::get_json(
        http,
        access_token,
        &format!("{API_BASE}/api/oauth/profile"),
        &h,
    )
    .await?;
    let (windows, detail_windows) = normalize(&usage);
    Ok(ServiceUsage {
        id: auto_service_id(Provider::Claude),
        source: ServiceSource::Auto,
        provider: Provider::Claude,
        connected: true,
        plan,
        account: account_override.or_else(|| profile.account.and_then(|a| a.email)),
        error: None,
        windows,
        detail_windows,
        raw_response: raw_json,
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
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    rate_limit_tier: Option<String>,
    #[serde(default)]
    memberships: Vec<WebMembership>,
}
#[derive(Deserialize)]
struct WebMembership {
    #[serde(default)]
    account: Option<WebMemberAccount>,
}
#[derive(Deserialize)]
struct WebMemberAccount {
    #[serde(default, rename = "email_address")]
    email_address: Option<String>,
}
#[derive(Deserialize)]
struct WebWindow {
    #[serde(default)]
    utilization: Option<f64>, // claude.ai returns int percent 0-100
    #[serde(default)]
    resets_at: Option<chrono::DateTime<chrono::Utc>>,
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "snake_case")]
struct WebUsage {
    #[serde(default)]
    five_hour: Option<WebWindow>,
    #[serde(default)]
    seven_day: Option<WebWindow>,
    #[serde(default)]
    seven_day_sonnet: Option<WebWindow>,
    #[serde(default)]
    seven_day_opus: Option<WebWindow>,
}

pub(crate) async fn fetch_with_session_key(
    http: &reqwest::Client,
    session_key: &str,
) -> Result<ServiceUsage, ProviderError> {
    let cookie = format!("sessionKey={}", session_key_cookie_value(session_key));

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
        .get(format!(
            "https://claude.ai/api/organizations/{}/usage",
            org.uuid
        ))
        .header("Cookie", &cookie)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    let v: serde_json::Value = crate::http::send_for_json(resp, "claude.ai/usage").await?;
    let raw_json = serde_json::to_string_pretty(&v).ok();
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
        id: auto_service_id(Provider::Claude),
        source: ServiceSource::Auto,
        provider: Provider::Claude,
        connected: true,
        plan: format_plan(&org.rate_limit_tier, &None).or(org.name),
        account: email,
        error: None,
        windows,
        detail_windows: detail,
        raw_response: raw_json,
    })
}

fn session_key_cookie_value(input: &str) -> String {
    let trimmed = input.trim();
    let payload = trimmed
        .strip_prefix("Cookie:")
        .or_else(|| trimmed.strip_prefix("cookie:"))
        .unwrap_or(trimmed)
        .trim();
    for part in payload.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("sessionKey=") {
            return value.trim().to_string();
        }
    }
    payload.to_string()
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
        assert_eq!(
            format_plan(&None, &Some("max".into())).as_deref(),
            Some("Max")
        );
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
    fn resolve_snake_case_and_raw_oauth_token() {
        let snake = serde_json::json!({
            "claude_ai_oauth": {
                "access_token": "sk-ant-oat01-snake",
                "refresh_token": "sk-ant-ort01-snake",
                "expires_at": 12345,
                "subscription_type": "team",
                "rate_limit_tier": "default_claude_max_5x"
            }
        });
        let r = resolve_creds(snake).unwrap();
        assert_eq!(r.access_token, "sk-ant-oat01-snake");
        assert_eq!(r.refresh_token.as_deref(), Some("sk-ant-ort01-snake"));
        assert_eq!(r.expires_at, 12345);
        assert_eq!(r.subscription_type.as_deref(), Some("team"));
        assert_eq!(r.rate_limit_tier.as_deref(), Some("default_claude_max_5x"));

        let raw = resolve_creds(serde_json::json!("sk-ant-oat01-env")).unwrap();
        assert_eq!(raw.access_token, "sk-ant-oat01-env");
        assert!(raw.refresh_token.is_none());
    }

    #[test]
    fn resolve_flat_refresh_token_and_string_expiry() {
        let flat = serde_json::json!({
            "accessToken": "sk-ant-oat01-flat",
            "refreshToken": "sk-ant-ort01-flat",
            "expiresAt": "1770412938485"
        });
        let r = resolve_creds(flat).unwrap();
        assert_eq!(r.access_token, "sk-ant-oat01-flat");
        assert_eq!(r.refresh_token.as_deref(), Some("sk-ant-ort01-flat"));
        assert_eq!(r.expires_at, 1770412938485);
    }

    #[test]
    fn resolve_falls_back_to_flat_when_nested_has_no_token() {
        let mixed = serde_json::json!({
            "claudeAiOauth": {
                "expiresAt": 0
            },
            "accessToken": "sk-ant-oat01-flat-fallback",
            "refreshToken": "sk-ant-ort01-flat-fallback"
        });
        let r = resolve_creds(mixed).unwrap();
        assert_eq!(r.access_token, "sk-ant-oat01-flat-fallback");
        assert_eq!(
            r.refresh_token.as_deref(),
            Some("sk-ant-ort01-flat-fallback")
        );
    }

    #[test]
    fn normalizes_pasted_session_key_cookie_values() {
        assert_eq!(
            session_key_cookie_value("sk-ant-sid01-raw"),
            "sk-ant-sid01-raw"
        );
        assert_eq!(
            session_key_cookie_value("sessionKey=sk-ant-sid01-direct"),
            "sk-ant-sid01-direct"
        );
        assert_eq!(
            session_key_cookie_value("Cookie: other=1; sessionKey=sk-ant-sid01-cookie; x=2"),
            "sk-ant-sid01-cookie"
        );
    }

    #[test]
    fn local_cli_refresh_policy_does_not_consume_refresh_token_for_rate_limit() {
        assert!(!should_refresh_local_cli_after_usage_status(429));
        assert!(!should_refresh_local_cli_after_usage_status(403));
        assert!(should_refresh_local_cli_after_usage_status(401));
    }

    #[test]
    fn refresh_request_body_shape() {
        let b = refresh_request_body("rt-123");
        assert_eq!(b["grant_type"], "refresh_token");
        assert_eq!(b["refresh_token"], "rt-123");
        assert_eq!(b["client_id"], CLAUDE_CLIENT_ID);
        assert_eq!(b["scope"], CLAUDE_OAUTH_SCOPES);
        assert_eq!(b.as_object().unwrap().len(), 4);
    }

    #[test]
    fn oauth_client_id_matches_current_claude_code_client() {
        assert_eq!(CLAUDE_CLIENT_ID, "9d1c250a-e61b-44d9-88ed-5944d1962f5e");
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
        assert_eq!(
            org.rate_limit_tier.as_deref(),
            Some("default_claude_max_20x")
        );
        let email = org
            .memberships
            .iter()
            .find_map(|m| m.account.as_ref())
            .and_then(|a| a.email_address.clone());
        assert_eq!(email.as_deref(), Some("user@example.com"));
    }
}
