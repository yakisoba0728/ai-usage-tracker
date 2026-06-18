//! GitHub Copilot — real usage via GitHub's documented billing API.
//! Auth reuses the `gh` CLI's stored token (`gh auth token`). The user-scoped
//! billing endpoint needs the `user` scope; if the token lacks it we surface an
//! actionable hint instead of guessing.

use std::path::PathBuf;

use async_trait::async_trait;
use serde::Deserialize;

use crate::http;
use crate::model::{LimitWindow, Provider, ServiceUsage};
use crate::providers::ProviderError;

const API_VERSION: &str = "2022-11-28";

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct BillingResp {
    #[serde(default)] usage_items: Vec<UsageItem>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageItem {
    #[serde(default)] net_quantity: Option<f64>,
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

fn gh_hosts_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap().join(".config"))
        .join("gh")
        .join("hosts.yml")
}

/// The GitHub username from `~/.config/gh/hosts.yml` (the `user:` line).
fn gh_user() -> Result<String, ProviderError> {
    let raw = std::fs::read_to_string(gh_hosts_path())
        .map_err(|e| ProviderError::NotLoggedIn(format!("gh hosts.yml: {e}")))?;
    for line in raw.lines() {
        let t = line.trim();
        if let Some(v) = t.strip_prefix("user:") {
            let v = v.trim().trim_matches('"').trim_matches('\'');
            if !v.is_empty() {
                return Ok(v.to_string());
            }
        }
    }
    Err(ProviderError::NotLoggedIn("no user in gh hosts.yml".into()))
}

/// Reuse the gh CLI's stored token via `gh auth token` (works across gh's
/// storage backends; we never store our own credentials).
fn gh_token() -> Result<String, ProviderError> {
    let out = std::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .map_err(|e| ProviderError::NotLoggedIn(format!("spawn gh: {e} (is gh installed?)")))?;
    if !out.status.success() {
        return Err(ProviderError::NotLoggedIn(
            "gh not logged in (run `gh auth login`)".into(),
        ));
    }
    let tok = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if tok.is_empty() {
        return Err(ProviderError::NotLoggedIn("gh auth token empty".into()));
    }
    Ok(tok)
}

/// Pure: sum netQuantity → used credits. Allowance/percent omitted (plan-dependent).
fn normalize(resp: &BillingResp) -> Vec<LimitWindow> {
    let used: f64 = resp.usage_items.iter().filter_map(|i| i.net_quantity).sum();
    vec![LimitWindow {
        label: "AI credits used (month)".into(),
        used_percent: None,
        resets_at: None,
        used: Some(used),
        limit: None,
    }]
}

#[async_trait]
impl crate::providers::ProviderApi for CopilotProvider {
    fn key(&self) -> Provider {
        Provider::Copilot
    }

    async fn fetch(&self) -> Result<ServiceUsage, ProviderError> {
        let user = gh_user()?;
        let token = gh_token()?;
        let url = format!(
            "https://api.github.com/users/{user}/settings/billing/ai_credit/usage?year={y}&month={m}",
            y = chrono::Utc::now().format("%Y"),
            m = chrono::Utc::now().format("%-m")
        );
        let extra = [
            ("Accept", "application/vnd.github+json"),
            ("X-GitHub-Api-Version", API_VERSION),
        ];
        let resp: BillingResp = http::get_json(&self.http, &token, &url, &extra).await.map_err(|e| {
            // Scope failures surface as Status; make the hint actionable.
            match e {
                ProviderError::Status { status, .. } if status == 403 || status == 404 => {
                    ProviderError::NotLoggedIn(format!(
                        "gh token lacks billing scope — run `gh auth refresh -h github.com -s user` {status}"
                    ))
                }
                other => other,
            }
        })?;
        Ok(ServiceUsage {
            provider: Provider::Copilot,
            connected: true,
            plan: None,
            account: Some(user),
            error: None,
            windows: normalize(&resp),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_sums_net_quantity() {
        let r: BillingResp =
            serde_json::from_str(include_str!("../../tests/copilot_billing_fixture.json")).unwrap();
        let w = &normalize(&r)[0];
        assert_eq!(w.used, Some(150.0));
        assert!(w.used_percent.is_none());
    }
}
