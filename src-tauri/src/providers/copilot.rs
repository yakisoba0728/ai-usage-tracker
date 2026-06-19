//! GitHub Copilot — reads the Copilot CLI's stored token (macOS Keychain
//! `copilot-cli`, else `~/.copilot/config.json`) and calls the internal
//! `GET /copilot_internal/v2/token` endpoint, which returns the real
//! `quota_snapshots` gauge (chat / premium_interactions / completions).
//! Token-parsing only; for manual accounts use a fine-grained PAT with
//! "Copilot Requests: Read" permission (paste via Add account).

use std::path::PathBuf;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::http;
use crate::model::{LimitWindow, Provider, ServiceUsage};
use crate::providers::ProviderError;
use crate::secrets;

const USAGE_URL: &str = "https://api.github.com/copilot_internal/v2/token";

#[derive(Deserialize, Default)]
struct CopilotTokenResp {
    #[serde(default)] copilot_plan: Option<String>,
    #[serde(default)] quota_reset_date_utc: Option<String>,
    #[serde(default)] quota_snapshots: QuotaSnapshots,
}

#[derive(Deserialize, Default)]
struct QuotaSnapshots {
    #[serde(default)] chat: Option<QuotaSnapshot>,
    #[serde(default)] premium_interactions: Option<QuotaSnapshot>,
    #[serde(default)] completions: Option<QuotaSnapshot>,
}

#[derive(Deserialize, Default)]
struct QuotaSnapshot {
    #[serde(default)] entitlement: Option<f64>,
    #[serde(default)] quota_remaining: Option<f64>,
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


/// Reuse the Copilot CLI's stored OAuth token. macOS: Keychain `copilot-cli`;
/// elsewhere: `~/.copilot/config.json` → `github.com.oauth_token`.
fn read_copilot_token() -> Result<String, ProviderError> {
    #[cfg(target_os = "macos")]
    {
        if let Ok(raw) = secrets::read_macos_keychain("copilot-cli") {
            if let Ok(v) = serde_json::from_str::<Value>(&raw) {
                if let Some(t) = v
                    .get("github.com")
                    .and_then(|g| g.get("oauth_token"))
                    .and_then(|t| t.as_str())
                {
                    return Ok(t.to_string());
                }
            }
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.to_string());
            }
        }
    }
    let path = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".copilot/config.json");
    let v = secrets::read_json_file(&path)?;
    v.get("github.com")
        .and_then(|g| g.get("oauth_token"))
        .and_then(|t| t.as_str())
        .map(String::from)
        .ok_or_else(|| {
            ProviderError::NotLoggedIn(
                "Copilot CLI token not found (run `copilot login` or paste a PAT)".into(),
            )
        })
}

fn parse_reset_date(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp())
}

/// Pure: quota_snapshots → LimitWindows, sorted by highest usage first.
fn normalize(resp: &CopilotTokenResp) -> Vec<LimitWindow> {
    let reset = resp.quota_reset_date_utc.as_deref().and_then(parse_reset_date);
    let categories: [(&str, &Option<QuotaSnapshot>); 3] = [
        ("Chat", &resp.quota_snapshots.chat),
        ("Premium requests", &resp.quota_snapshots.premium_interactions),
        ("Completions", &resp.quota_snapshots.completions),
    ];
    let mut windows: Vec<LimitWindow> = categories
        .iter()
        .filter_map(|(label, q)| {
            let q = q.as_ref()?;
            if q.unlimited.unwrap_or(false) {
                return None;
            }
            let pct = q.percent_remaining.map(|r| (100.0 - r) as f32);
            let used = match (q.entitlement, q.quota_remaining) {
                (Some(e), Some(r)) => Some(e - r),
                _ => None,
            };
            Some(LimitWindow {
                label: (*label).into(),
                used_percent: pct,
                resets_at: reset,
                used,
                limit: q.entitlement,
            })
        })
        .collect();
    windows.sort_by(|a, b| {
        b.used_percent
            .unwrap_or(0.0)
            .partial_cmp(&a.used_percent.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    windows
}

#[async_trait]
impl crate::providers::ProviderApi for CopilotProvider {
    fn key(&self) -> Provider {
        Provider::Copilot
    }

    async fn fetch(&self) -> Result<ServiceUsage, ProviderError> {
        let token = read_copilot_token()?;
        fetch_with(&self.http, &token).await
    }
}

/// Fetch Copilot usage given an explicit token (auto-detected Copilot CLI token
/// or a manually-pasted PAT). Calls `/copilot_internal/v2/token`.
pub(crate) async fn fetch_with(
    http: &reqwest::Client,
    token: &str,
) -> Result<ServiceUsage, ProviderError> {
    let resp = http
        .get(USAGE_URL)
        .header("Authorization", format!("token {token}"))
        .header("Accept", "application/json")
        .header("Editor-Version", "vscode/1.96.2")
        .header("Editor-Plugin-Version", "copilot-chat/0.35.0")
        .header("Copilot-Integration-Id", "vscode-chat")
        .header("X-Github-Api-Version", "2025-04-01")
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    let val: Value = http::send_for_json(resp, USAGE_URL).await?;
    let u: CopilotTokenResp =
        serde_json::from_value(val).map_err(|e| ProviderError::Parse(e.to_string()))?;
    let windows = normalize(&u);
    Ok(ServiceUsage {
        provider: Provider::Copilot,
        connected: true,
        plan: u.copilot_plan,
        account: None,
        error: None,
        windows,
        detail_windows: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_quota_snapshots() {
        let resp = CopilotTokenResp {
            copilot_plan: Some("pro".into()),
            quota_reset_date_utc: Some("2026-07-01T00:00:00Z".into()),
            quota_snapshots: QuotaSnapshots {
                chat: Some(QuotaSnapshot {
                    entitlement: Some(300.0),
                    quota_remaining: Some(250.0),
                    percent_remaining: Some(83.33),
                    unlimited: Some(false),
                }),
                premium_interactions: Some(QuotaSnapshot {
                    entitlement: Some(300.0),
                    quota_remaining: Some(50.0),
                    percent_remaining: Some(16.67),
                    unlimited: Some(false),
                }),
                completions: None,
            },
        };
        let ws = normalize(&resp);
        assert_eq!(ws.len(), 2);
        // Higher usage (lower percent_remaining) first.
        assert_eq!(ws[0].label, "Premium requests");
        assert_eq!(ws[0].used_percent, Some(83.33));
        assert_eq!(ws[0].used, Some(250.0));
        assert_eq!(ws[0].limit, Some(300.0));
    }
}
