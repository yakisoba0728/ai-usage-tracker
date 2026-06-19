//! Claude (via Claude Code) — modeled on claude-meter (MIT), with deeper
//! parsing for the detail view. Reads the OAuth token Claude Code stores
//! (macOS Keychain `Claude Code-credentials`, else `~/.claude/.credentials.json`),
//! checks expiry, then calls `api.anthropic.com/api/oauth/usage` + `/profile`.
//! Surfaces every rolling window (five_hour, seven_day + per-model + cowork +
//! omelette) and `extra_usage`, and derives a human plan label like "Max 20x"
//! from `rateLimitTier`. Token-parsing only.

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
    #[serde(rename = "expiresAt", default)] expires_at: i64,
    #[serde(rename = "subscriptionType", default)] subscription_type: Option<String>,
    #[serde(rename = "rateLimitTier", default)] rate_limit_tier: Option<String>,
}

struct ResolvedCreds {
    access_token: String,
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
            expires_at: o.expires_at,
            subscription_type: o.subscription_type,
            rate_limit_tier: o.rate_limit_tier,
        });
    }
    parsed
        .flat_access
        .map(|access_token| ResolvedCreds {
            access_token,
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

        let (windows, detail_windows) = normalize(&usage);
        Ok(ServiceUsage {
            provider: Provider::Claude,
            connected: true,
            plan: format_plan(&creds.rate_limit_tier, &creds.subscription_type),
            account: profile.account.and_then(|a| a.email),
            error: None,
            windows,
            detail_windows,
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
}
