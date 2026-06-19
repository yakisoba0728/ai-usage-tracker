//! GitHub Copilot — reads the **Copilot CLI**'s stored token (macOS Keychain
//! service `copilot-cli`, else `~/.copilot/config.json`) and calls the SAME
//! internal quota endpoint the VS Code Copilot extension uses:
//! `api.github.com/copilot_internal/user` with the Copilot editor headers.
//! Gives entitlement / remaining / percent / reset date. Token-parsing only.
//!
//! Note: the `gh` CLI's OAuth token does NOT work here (wrong client, no
//! Copilot scope). Use `copilot login` (the Copilot CLI) or a fine-grained PAT.

use std::path::PathBuf;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::http;
use crate::model::{LimitWindow, Provider, ServiceUsage};
use crate::providers::ProviderError;
use crate::secrets;

const USAGE_URL: &str = "https://api.github.com/copilot_internal/user";

#[derive(Deserialize, Default)]
struct CopilotUser {
    #[serde(default)] copilot_plan: Option<String>,
    #[serde(default)] quota_reset_date: Option<String>,
    #[serde(default)] quota_snapshots: Option<QuotaSnapshots>,
}
#[derive(Deserialize, Default)]
struct QuotaSnapshots {
    #[serde(default)] premium_interactions: Option<Quota>,
}
#[derive(Deserialize, Default)]
struct Quota {
    #[serde(default)] entitlement: Option<f64>,
    #[serde(default)] remaining: Option<f64>,
    #[serde(default)] percent_remaining: Option<f64>,
    #[serde(default)] unlimited: Option<bool>,
}

pub struct CopilotProvider {
    http: reqwest::Client,
}
impl CopilotProvider {
    pub fn new() -> Self {
        Self {
            http: http::build_client(),
        }
    }
}

fn copilot_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".copilot")
        .join("config.json")
}

/// Reuse the Copilot CLI's stored OAuth token. macOS Keychain service
/// `copilot-cli`; elsewhere `~/.copilot/config.json` → github.com.oauth_token.
fn read_copilot_token() -> Result<String, ProviderError> {
    #[cfg(target_os = "macos")]
    {
        if let Ok(tok) = secrets::read_macos_keychain("copilot-cli") {
            if !tok.is_empty() {
                return Ok(tok.trim().to_string());
            }
        }
    }
    if let Ok(v) = secrets::read_json_file(&copilot_config_path()) {
        if let Some(tok) = v
            .get("github.com")
            .and_then(|g| g.get("oauth_token"))
            .and_then(|t| t.as_str())
        {
            return Ok(tok.to_string());
        }
    }
    Err(ProviderError::NotLoggedIn(
        "Copilot CLI not logged in (run `copilot login`)".into(),
    ))
}

fn parse_reset_date(s: &str) -> Option<i64> {
    let d = chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()?;
    Some(d.and_hms_opt(0, 0, 0)?.and_utc().timestamp())
}

/// Pure: premium_interactions quota → window.
fn normalize(u: &CopilotUser) -> Vec<LimitWindow> {
    let Some(q) = u.quota_snapshots.as_ref().and_then(|s| s.premium_interactions.as_ref())
    else {
        return vec![];
    };
    if q.unlimited == Some(true) {
        return vec![LimitWindow {
            label: "Premium interactions".into(),
            used_percent: None,
            resets_at: u.quota_reset_date.as_deref().and_then(parse_reset_date),
            used: None,
            limit: None,
        }];
    }
    let limit = q.entitlement;
    let used = match (q.entitlement, q.remaining) {
        (Some(e), Some(r)) => Some(e - r),
        _ => None,
    };
    let used_percent = q
        .percent_remaining
        .map(|p| 100.0 - p)
        .or_else(|| match (used, limit) {
            (Some(u), Some(l)) if l > 0.0 => Some(u / l * 100.0),
            _ => None,
        });
    vec![LimitWindow {
        label: "Premium interactions".into(),
        used_percent: used_percent.map(|x| x as f32),
        resets_at: u.quota_reset_date.as_deref().and_then(parse_reset_date),
        used,
        limit,
    }]
}

#[async_trait]
impl crate::providers::ProviderApi for CopilotProvider {
    fn key(&self) -> Provider {
        Provider::Copilot
    }

    async fn fetch(&self) -> Result<ServiceUsage, ProviderError> {
        let token = read_copilot_token()?;
        let resp = self
            .http
            .get(USAGE_URL)
            .header("Authorization", format!("token {token}"))
            .header("Accept", "application/json")
            .header("User-Agent", "GitHubCopilotChat/0.35.0")
            .header("Editor-Version", "vscode/1.107.0")
            .header("Editor-Plugin-Version", "copilot-chat/0.35.0")
            .header("Copilot-Integration-Id", "vscode-chat")
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;
        let val: Value = http::send_for_json(resp, USAGE_URL).await.map_err(|e| {
            // 403/404 usually means the token lacks Copilot scope.
            ProviderError::NotLoggedIn(format!(
                "Copilot quota unavailable ({e}); run `copilot login`"
            ))
        })?;
        let u: CopilotUser =
            serde_json::from_value(val).map_err(|e| ProviderError::Parse(e.to_string()))?;
        Ok(ServiceUsage {
            provider: Provider::Copilot,
            connected: true,
            plan: u.copilot_plan.clone(),
            account: None,
            error: None,
            windows: normalize(&u),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_internal_fixture() {
        let u: CopilotUser =
            serde_json::from_str(include_str!("../../tests/copilot_internal_fixture.json")).unwrap();
        let w = &normalize(&u)[0];
        assert_eq!(w.limit, Some(300.0));
        assert_eq!(w.used, Some(229.0)); // 300 - 71
        assert_eq!(w.used_percent, Some(76.0)); // 100 - 24
        assert!(w.resets_at.is_some()); // 2026-07-01 parsed
    }

    #[test]
    fn normalize_empty_when_no_quota() {
        assert!(normalize(&CopilotUser::default()).is_empty());
    }
}
