//! z.ai GLM Coding Plan via API key. There is no z.ai CLI storing a token
//! locally, so the auto-detected path reads `ZAI_API_KEY` from the env; users
//! otherwise paste a key via Add account (stored accounts → `fetch_with`).
//!
//! Calls the SAME undocumented monitor endpoint the community tools poll:
//! `GET https://api.z.ai/api/monitor/usage/quota/limit` with
//! `Authorization: Bearer <api_key>`. It returns a `data.limits[]` array where
//! each entry is a quota window (5-hour / weekly token quotas, plus monthly
//! MCP-call quotas). The headline window is the higher-burn of the two token
//! windows; everything else (the other token window, MCP, per-model) drops
//! into `detail_windows`.
//!
//! When the plan is fully exhausted the server returns business `code` 1308
//! (short window) or 1310 (weekly/monthly) carrying a `data.next_flush_time`
//! reset timestamp — we surface that as a 100%-used window with `resets_at`.
//!
//! Source for the response shape: the `quotas` crate (`docs-usage/zai.md`,
//! `src/providers/zai.rs`) and the VS Code `vscode-zai-usage` extension, both
//! of which poll this same endpoint. Error codes per
//! https://docs.z.ai/api-reference/api-code.

mod parse;

use async_trait::async_trait;
use serde_json::Value;

use crate::http;
use crate::model::{auto_service_id, Provider, ServiceSource, ServiceUsage};
use crate::providers::ProviderError;

use parse::{
    exhausted_window, normalize, plan_from_level_str, ZaiResponse, CODE_PERIOD_EXHAUSTED,
    CODE_USAGE_EXHAUSTED,
};

/// Undocumented monitor endpoint shared by every community z.ai usage tool.
const USAGE_URL: &str = "https://api.z.ai/api/monitor/usage/quota/limit";
/// Env var the auto-detected path reads (z.ai ships no local CLI token).
const ENV_KEY: &str = "ZAI_API_KEY";

pub struct ZaiProvider {
    http: reqwest::Client,
}
impl Default for ZaiProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ZaiProvider {
    pub fn new() -> Self {
        Self {
            http: http::shared(),
        }
    }
}

#[async_trait]
impl crate::providers::ProviderApi for ZaiProvider {
    fn key(&self) -> Provider {
        Provider::Zai
    }

    async fn fetch(&self) -> Result<ServiceUsage, ProviderError> {
        let key = std::env::var(ENV_KEY).map_err(|_| {
            ProviderError::NotLoggedIn(
                "z.ai API key not set; add it via Add account or export ZAI_API_KEY".into(),
            )
        })?;
        fetch_with(&self.http, &key, None).await
    }
}

/// Fetch z.ai usage given an explicit API key (used for stored accounts and
/// the env-var auto-detected path). API keys do not expire.
pub(crate) async fn fetch_with(
    http: &reqwest::Client,
    api_key: &str,
    label_override: Option<&str>,
) -> Result<ServiceUsage, ProviderError> {
    let resp = http
        .get(USAGE_URL)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    // Capture as Value first — the endpoint is undocumented and the exhausted
    // path needs `code` / `data.next_flush_time` before struct conversion.
    let raw: Value = http::send_for_json(resp, USAGE_URL).await?;
    let raw_json = serde_json::to_string_pretty(&raw).ok();

    let code = raw.get("code").and_then(|v| v.as_i64());
    let exhausted_code = code.filter(|&c| c == CODE_USAGE_EXHAUSTED || c == CODE_PERIOD_EXHAUSTED);
    if let Some(c) = exhausted_code {
        let plan = plan_from_level_str(
            raw.get("data")
                .and_then(|d| d.get("level"))
                .and_then(|v| v.as_str()),
        );
        return Ok(ServiceUsage {
            id: auto_service_id(Provider::Zai),
            source: ServiceSource::Auto,
            provider: Provider::Zai,
            connected: true,
            plan,
            account: label_override.map(String::from),
            error: None,
            windows: vec![exhausted_window(c, raw.get("data"))],
            detail_windows: vec![],
            raw_response: raw_json.clone(),
        });
    }

    let u: ZaiResponse =
        serde_json::from_value(raw).map_err(|e| ProviderError::Parse(format!("zai usage: {e}")))?;
    let ok = u.success.unwrap_or(false) || u.code == Some(200);
    if !ok {
        let msg = u
            .msg
            .unwrap_or_else(|| "z.ai quota request failed".to_string());
        return Err(ProviderError::Status {
            status: 200,
            body: format!("{USAGE_URL}: {msg}"),
        });
    }

    let plan = plan_from_level_str(u.data.as_ref().and_then(|d| d.level.as_deref()));
    let (windows, detail_windows) = u.data.as_ref().map(normalize).unwrap_or_default();
    Ok(ServiceUsage {
        id: auto_service_id(Provider::Zai),
        source: ServiceSource::Auto,
        provider: Provider::Zai,
        connected: true,
        plan,
        account: label_override.map(String::from),
        error: None,
        windows,
        detail_windows,
        raw_response: raw_json,
    })
}

/// API keys do not expire, so there is nothing to refresh.
pub(crate) async fn refresh_stored(
    _: &reqwest::Client,
    _: &crate::store::StoredCredential,
) -> Option<crate::store::StoredCredential> {
    None
}

/// Fetch usage for a stored z.ai account (uniform stored-fetch adapter).
pub(crate) async fn fetch_stored(
    http: &reqwest::Client,
    cred: &crate::store::StoredCredential,
) -> Result<ServiceUsage, ProviderError> {
    fetch_with(http, &cred.access_token, Some(&cred.label)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn refresh_stored_is_none() {
        // API keys do not expire — nothing to refresh.
        let cred = crate::store::StoredCredential {
            id: "x".into(),
            provider: Provider::Zai,
            label: "x".into(),
            access_token: "k".into(),
            refresh_token: None,
            expires_at: 0,
            id_token: None,
            account_id: None,
        };
        assert!(refresh_stored(&http::build_client(), &cred).await.is_none());
    }
}
