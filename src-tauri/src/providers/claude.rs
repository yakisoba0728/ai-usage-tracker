//! Claude (via Claude Code) — modeled on claude-meter (MIT), with self-refresh.
//! Reads the OAuth token Claude Code stores (macOS Keychain service
//! `Claude Code-credentials`, else `~/.claude/.credentials.json`); when the
//! access token is expired it refreshes via Anthropic OAuth using Claude Code's
//! PUBLIC client_id (no own OAuth app) and writes the rotated token back so the
//! CLI and app stay in sync. Then calls `api.anthropic.com/api/oauth/usage` +
//! `/api/oauth/profile`. This removes the "run `claude` once" requirement.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::http;
use crate::model::{LimitWindow, Provider, ServiceUsage};
use crate::providers::ProviderError;
use crate::secrets;

const API_BASE: &str = "https://api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const KEYCHAIN_SERVICE: &str = "Claude Code-credentials";

/// The credential blob (keychain value or file content).
#[derive(Deserialize)]
struct CredBlob {
    #[serde(rename = "claudeAiOauth")]
    oauth: Option<OAuthCreds>,
    // legacy flat shape fallback
    #[serde(rename = "accessToken", default)] flat_access: Option<String>,
    #[serde(rename = "refreshToken", default)] flat_refresh: Option<String>,
    #[serde(rename = "expiresAt", default)] flat_expires: i64,
    #[serde(rename = "subscriptionType", default)] flat_sub: Option<String>,
}

#[derive(Deserialize)]
struct OAuthCreds {
    #[serde(rename = "accessToken")] access_token: String,
    #[serde(rename = "refreshToken", default)] refresh_token: Option<String>,
    #[serde(rename = "expiresAt", default)] expires_at: i64,
    #[serde(rename = "subscriptionType", default)] subscription_type: Option<String>,
}

struct ResolvedCreds {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: i64,
    subscription_type: Option<String>,
}

fn resolve_creds(blob: Value) -> Result<ResolvedCreds, ProviderError> {
    let parsed: CredBlob =
        serde_json::from_value(blob).map_err(|e| ProviderError::Parse(format!("claude creds: {e}")))?;
    if let Some(o) = parsed.oauth {
        return Ok(ResolvedCreds {
            access_token: o.access_token,
            refresh_token: o.refresh_token,
            expires_at: o.expires_at,
            subscription_type: o.subscription_type,
        });
    }
    // legacy flat
    parsed
        .flat_access
        .map(|access_token| ResolvedCreds {
            access_token,
            refresh_token: parsed.flat_refresh,
            expires_at: parsed.flat_expires,
            subscription_type: parsed.flat_sub,
        })
        .ok_or_else(|| ProviderError::Parse("claude creds: no accessToken".into()))
}

#[derive(Deserialize)]
struct Window {
    #[serde(default)] utilization: Option<f64>,
    #[serde(default)] resets_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Deserialize)]
struct ExtraUsage {
    #[serde(default)] is_enabled: Option<bool>,
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
    #[serde(default)] extra_usage: Option<ExtraUsage>,
}

#[derive(Deserialize)]
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
        used_percent: w.utilization.map(|v| (v * 100.0) as f32),
        resets_at: w.resets_at.map(|d| d.timestamp()),
        used: None,
        limit: None,
    }
}

/// Pure normalization (unit-testable, no network).
fn normalize(raw: &UsageResponse) -> Vec<LimitWindow> {
    let mut ws = Vec::new();
    if let Some(w) = &raw.five_hour {
        ws.push(window("5-hour", w));
    }
    if let Some(w) = &raw.seven_day {
        ws.push(window("7-day", w));
    }
    if let Some(w) = &raw.seven_day_sonnet {
        ws.push(window("7-day (Sonnet)", w));
    }
    if let Some(w) = &raw.seven_day_opus {
        ws.push(window("7-day (Opus)", w));
    }
    if let Some(w) = &raw.seven_day_oauth_apps {
        ws.push(window("7-day (OAuth Apps)", w));
    }
    if let Some(e) = &raw.extra_usage {
        if e.is_enabled.unwrap_or(false) {
            ws.push(LimitWindow {
                label: "Extra usage".into(),
                used_percent: e.utilization.map(|v| (v * 100.0) as f32),
                resets_at: None,
                used: e.used_credits,
                limit: None,
            });
        }
    }
    ws
}

/// Build the refreshed credential blob, preserving all other fields (scopes,
/// plan, tier). Pure (no I/O) so it can be unit-tested.
fn merge_refresh(
    orig: &Value,
    access_token: &str,
    refresh_token: &str,
    expires_at: i64,
) -> Result<Value, ProviderError> {
    let mut blob = orig.clone();
    let target: &mut serde_json::map::Map<String, Value> = if let Some(o) =
        blob.get_mut("claudeAiOauth").and_then(|v| v.as_object_mut())
    {
        o
    } else if let Some(obj) = blob.as_object_mut() {
        obj
    } else {
        return Err(ProviderError::Parse(
            "claude creds: cannot write back (unexpected blob shape)".into(),
        ));
    };
    target.insert("accessToken".into(), serde_json::json!(access_token));
    target.insert("refreshToken".into(), serde_json::json!(refresh_token));
    target.insert("expiresAt".into(), serde_json::json!(expires_at));
    Ok(blob)
}

fn write_creds(s: &str) -> Result<(), ProviderError> {
    #[cfg(target_os = "macos")]
    {
        let acct = std::env::var("USER").unwrap_or_default();
        let out = std::process::Command::new("/usr/bin/security")
            .args([
                "add-generic-password",
                "-s",
                KEYCHAIN_SERVICE,
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
                "security write failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        return Ok(());
    }
    #[cfg(not(target_os = "macos"))]
    {
        let p = secrets::claude_token_path();
        std::fs::write(&p, s).map_err(|e| ProviderError::Network(format!("write {}: {e}", p.display())))?;
        Ok(())
    }
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

        if creds.expires_at > 0 && creds.expires_at < now_ms {
            let Some(rt) = creds.refresh_token.clone() else {
                return Err(ProviderError::Expired(
                    "Claude Code token expired and no refresh_token available".into(),
                ));
            };
            let form = [
                ("grant_type", "refresh_token"),
                ("refresh_token", rt.as_str()),
                ("client_id", crate::oauth::CLAUDE_CLIENT_ID),
            ];
            let refreshed =
                crate::oauth::refresh_form(&self.http, crate::oauth::CLAUDE_TOKEN_URL, &form).await?;
            let new_expires = refreshed
                .expires_in
                .map(|s| now_ms + (s as i64) * 1000)
                .unwrap_or(0);
            let new_blob = merge_refresh(&blob, &refreshed.access_token, &refreshed.refresh_token, new_expires)?;
            let _ = write_creds(&serde_json::to_string(&new_blob).unwrap_or_default());
            creds.access_token = refreshed.access_token;
        }

        let h = [("anthropic-version", ANTHROPIC_VERSION)];
        let usage: UsageResponse =
            http::get_json(&self.http, &creds.access_token, &format!("{API_BASE}/api/oauth/usage"), &h)
                .await?;
        let profile: Profile =
            http::get_json(&self.http, &creds.access_token, &format!("{API_BASE}/api/oauth/profile"), &h)
                .await?;

        Ok(ServiceUsage {
            provider: Provider::Claude,
            connected: true,
            plan: creds.subscription_type,
            account: profile.account.and_then(|a| a.email),
            error: None,
            windows: normalize(&usage),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_fixture() {
        let raw: UsageResponse =
            serde_json::from_str(include_str!("../../tests/claude_fixture.json")).unwrap();
        let ws = normalize(&raw);
        let labels: Vec<&str> = ws.iter().map(|w| w.label.as_str()).collect();
        assert!(labels.contains(&"5-hour"));
        assert!(labels.contains(&"7-day"));
        assert!(labels.contains(&"Extra usage"));
        let five = ws.iter().find(|w| w.label == "5-hour").unwrap();
        assert_eq!(five.used_percent, Some(23.5));
        assert!(five.resets_at.is_some());
    }

    #[test]
    fn resolve_nested_and_flat_including_refresh() {
        let nested = serde_json::json!({"claudeAiOauth":{"accessToken":"a","refreshToken":"r","expiresAt":0,"subscriptionType":"max"}});
        let r = resolve_creds(nested).unwrap();
        assert_eq!(r.access_token, "a");
        assert_eq!(r.refresh_token.as_deref(), Some("r"));
        assert_eq!(r.subscription_type.as_deref(), Some("max"));

        let flat = serde_json::json!({"accessToken":"b","expiresAt":0});
        let r2 = resolve_creds(flat).unwrap();
        assert_eq!(r2.access_token, "b");
        assert!(r2.refresh_token.is_none());
    }

    #[test]
    fn merge_refresh_preserves_other_fields() {
        let blob = serde_json::json!({"claudeAiOauth":{"accessToken":"old","refreshToken":"oldr","expiresAt":1,"scopes":["user:profile"],"subscriptionType":"max","rateLimitTier":"t"}});
        let merged = merge_refresh(&blob, "new", "newr", 999).unwrap();
        let o = &merged["claudeAiOauth"];
        assert_eq!(o["accessToken"], "new");
        assert_eq!(o["refreshToken"], "newr");
        assert_eq!(o["expiresAt"], 999);
        assert_eq!(o["subscriptionType"], "max"); // preserved
        assert_eq!(o["scopes"][0], "user:profile"); // preserved
    }
}
