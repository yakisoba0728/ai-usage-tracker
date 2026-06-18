//! Claude (via Claude Code) — modeled on claude-meter (MIT).
//! Reads the OAuth token Claude Code stores (macOS Keychain service
//! `Claude Code-credentials`, else `~/.claude/.credentials.json`), checks expiry,
//! and calls `api.anthropic.com/api/oauth/usage` + `/api/oauth/profile`.
//! No self-refresh: the Claude Code CLI rotates the token.

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
    #[serde(rename = "expiresAt", default)] flat_expires: Option<i64>,
    #[serde(rename = "subscriptionType", default)] flat_sub: Option<String>,
}

#[derive(Deserialize)]
struct OAuthCreds {
    #[serde(rename = "accessToken")] access_token: String,
    #[serde(rename = "expiresAt", default)] expires_at: i64,
    #[serde(rename = "subscriptionType", default)] subscription_type: Option<String>,
}

struct ResolvedCreds {
    access_token: String,
    expires_at: i64,
    subscription_type: Option<String>,
}

fn resolve_creds(blob: serde_json::Value) -> Result<ResolvedCreds, ProviderError> {
    let parsed: CredBlob = serde_json::from_value(blob)
        .map_err(|e| ProviderError::Parse(format!("claude creds: {e}")))?;
    if let Some(o) = parsed.oauth {
        return Ok(ResolvedCreds {
            access_token: o.access_token,
            expires_at: o.expires_at,
            subscription_type: o.subscription_type,
        });
    }
    // legacy flat
    parsed
        .flat_access
        .map(|access_token| ResolvedCreds {
            access_token,
            expires_at: parsed.flat_expires.unwrap_or(0),
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
pub fn normalize(raw: &UsageResponse) -> Vec<LimitWindow> {
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

#[async_trait]
impl crate::providers::ProviderApi for ClaudeProvider {
    fn key(&self) -> Provider {
        Provider::Claude
    }

    async fn fetch(&self) -> Result<ServiceUsage, ProviderError> {
        let creds = resolve_creds(secrets::read_claude_creds_json()?)?;
        let now_ms = chrono::Utc::now().timestamp_millis();
        if creds.expires_at > 0 && creds.expires_at < now_ms {
            return Err(ProviderError::Expired(
                "Claude Code token expired — run `claude` once to refresh".into(),
            ));
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
    fn resolve_nested_and_flat() {
        let nested = serde_json::json!({"claudeAiOauth":{"accessToken":"a","expiresAt":0,"subscriptionType":"max"}});
        let r = resolve_creds(nested).unwrap();
        assert_eq!(r.access_token, "a");
        assert_eq!(r.subscription_type.as_deref(), Some("max"));

        let flat = serde_json::json!({"accessToken":"b","expiresAt":0});
        let r2 = resolve_creds(flat).unwrap();
        assert_eq!(r2.access_token, "b");
    }
}
