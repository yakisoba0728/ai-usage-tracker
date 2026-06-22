//! Gemini — usage fetch via stored OAuth accounts only.
//!
//! The Gemini CLI migrated off `~/.gemini/oauth_creds.json` (it deletes the
//! file) to an OS keychain / encrypted store we can't reliably read, so CLI
//! auto-detect is dropped. Gemini is supported exclusively via in-app OAuth
//! ("Add account") stored accounts; refresh uses the same Google OAuth endpoint
//! (`refresh_gemini_token`) and the CLI's public client_id/secret
//! (env override `GEMINI_OAUTH_CLIENT_ID`/`..._SECRET`).

mod parse;
mod refresh;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::http;
use crate::model::{auto_service_id, Provider, ServiceSource, ServiceUsage};
use crate::providers::ProviderError;

use parse::{normalize, QuotaResp};

/// Refresh a stored Gemini OAuth credential (see `refresh::refresh_stored`).
pub(crate) use refresh::refresh_stored;

const CODE_ASSIST_BASE: &str = "https://cloudcode-pa.googleapis.com/v1internal";

pub struct GeminiProvider;

impl Default for GeminiProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl GeminiProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl crate::providers::ProviderApi for GeminiProvider {
    fn key(&self) -> Provider {
        Provider::Gemini
    }

    async fn fetch(&self) -> Result<ServiceUsage, ProviderError> {
        // Gemini is supported via in-app OAuth (Add account → stored) ONLY. The
        // Gemini CLI migrated off ~/.gemini/oauth_creds.json (deletes it) to an OS
        // keychain / encrypted file we can't reliably read, so CLI auto-detect is
        // dropped. A connected Gemini comes only from a stored OAuth account.
        Err(ProviderError::NotLoggedIn(
            "Gemini auto-detect disabled — add via Add account (OAuth)".into(),
        ))
    }
}

async fn post_code_assist(
    http: &reqwest::Client,
    token: &str,
    method: &str,
    payload: Value,
) -> Result<Value, ProviderError> {
    http::post_json(
        http,
        token,
        &format!("{CODE_ASSIST_BASE}:{method}"),
        &payload,
    )
    .await
}

/// Fetch Gemini usage given an explicit token (manually-added accounts).
pub(crate) async fn fetch_with(
    http: &reqwest::Client,
    token: &str,
    label_override: Option<&str>,
) -> Result<ServiceUsage, ProviderError> {
    let load = post_code_assist(
        http,
        token,
        "loadCodeAssist",
        json!({ "metadata": { "ideType": "IDE_UNSPECIFIED", "platform": "PLATFORM_UNSPECIFIED", "pluginType": "GEMINI" } }),
    )
    .await?;
    let project = load
        .get("cloudaicompanionProject")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| ProviderError::Parse("no cloudaicompanionProject".into()))?;
    let quota_val = post_code_assist(
        http,
        token,
        "retrieveUserQuota",
        json!({ "project": project }),
    )
    .await?;
    let raw_json = serde_json::to_string_pretty(&serde_json::json!({
        "loadCodeAssist": &load,
        "retrieveUserQuota": &quota_val,
    }))
    .ok();
    let quota: QuotaResp =
        serde_json::from_value(quota_val).map_err(|e| ProviderError::Parse(e.to_string()))?;
    let tier = load
        .get("paidTier")
        .and_then(|t| t.get("name"))
        .and_then(|n| n.as_str())
        .map(String::from);
    let (windows, detail_windows) = normalize(&quota);
    Ok(ServiceUsage {
        id: auto_service_id(Provider::Gemini),
        source: ServiceSource::Auto,
        provider: Provider::Gemini,
        connected: true,
        plan: tier,
        account: label_override.map(|s| s.to_string()),
        error: None,
        windows,
        detail_windows,
        raw_response: raw_json,
    })
}

/// Fetch usage for a stored Gemini account (uniform stored-fetch adapter).
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
    async fn auto_fetch_is_oauth_only_not_logged_in() {
        use crate::providers::ProviderApi;
        // Gemini auto-detect of the CLI is unsupported; only in-app OAuth (stored)
        // is. The auto provider must report not_logged_in without touching disk.
        let out = GeminiProvider::new().fetch().await;
        match out {
            Err(crate::providers::ProviderError::NotLoggedIn(_)) => {}
            other => panic!("expected NotLoggedIn, got {other:?}"),
        }
    }
}
