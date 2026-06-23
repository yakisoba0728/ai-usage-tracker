//! Codex (ChatGPT) via Codex CLI. Reads `~/.codex/auth.json` and calls the SAME
//! endpoint the official codex CLI polls: `chatgpt.com/backend-api/wham/usage` with
//! `Authorization: Bearer <access_token>`, `ChatGPT-Account-Id`, and a
//! `codex_cli_rs/` User-Agent. Surfaces the main 5h/weekly rate limits, every
//! additional rate limit (e.g. `GPT-5.3-Codex-Spark`), credits, and available
//! rate-limit resets. Stored manual accounts self-refresh via `refresh_stored`
//! (POST `auth.openai.com/oauth/token`, public CLI client_id from codex-rs/login).
//! Token-parsing only.

mod creds;
mod parse;
mod refresh;

use async_trait::async_trait;
use serde_json::Value;

use crate::http;
use crate::model::{auto_service_id, Provider, ServiceSource, ServiceUsage};
use crate::providers::ProviderError;

use creds::read_auth;
use parse::{normalize, WhamUsage};
use refresh::{account_id_for_tokens, refresh_auto_auth_if_needed};

/// Usage endpoint, per openai/codex `backend-client/rate_limit_resets.rs`
/// (`PathStyle::ChatGptApi` → `/wham/usage` under `/backend-api`).
const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
/// User-Agent. Cloudflare's allow-list keys on the `codex_cli_rs/` prefix
/// (stable identifier in codex-rs/login `DEFAULT_ORIGINATOR`); the version
/// mirrors the current CLI build so the UA looks like a real client.
/// `ai-usage-tracker` is appended for transparency (allowed UA suffix).
const CODEX_UA: &str = "codex_cli_rs/0.141.0 (ai-usage-tracker)";

pub struct CodexProvider {
    http: reqwest::Client,
}
impl Default for CodexProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CodexProvider {
    pub fn new() -> Self {
        Self {
            http: http::shared(),
        }
    }
}

#[async_trait]
impl crate::providers::ProviderApi for CodexProvider {
    fn key(&self) -> Provider {
        Provider::Codex
    }

    async fn fetch(&self) -> Result<ServiceUsage, ProviderError> {
        let (access_token, account_id) = prepare_auto_auth(&self.http).await?;
        fetch_with(&self.http, &access_token, account_id.as_deref(), None).await
    }
}

/// Prepare Codex CLI auth exactly once for auto consumers: read auth.json,
/// refresh an expiring access token, and derive ChatGPT-Account-Id from the
/// id_token when auth.json lacks the explicit field.
pub(crate) async fn prepare_auto_auth(
    http: &reqwest::Client,
) -> Result<(String, Option<String>), ProviderError> {
    let (_, t) = read_auth()?;
    let t = refresh_auto_auth_if_needed(http, t).await?;
    let account_id = account_id_for_tokens(&t);
    Ok((t.access_token, account_id))
}

/// Fetch Codex usage given an explicit token (used for manually-added accounts).
pub(crate) async fn fetch_with(
    http: &reqwest::Client,
    access_token: &str,
    account_id: Option<&str>,
    label_override: Option<&str>,
) -> Result<ServiceUsage, ProviderError> {
    let mut extra: Vec<(&str, &str)> = vec![("User-Agent", CODEX_UA)];
    let acc_holder;
    if let Some(acc) = account_id {
        acc_holder = acc.to_string();
        extra.push(("ChatGPT-Account-Id", acc_holder.as_str()));
    }
    let raw: Value = http::get_json(http, access_token, USAGE_URL, &extra).await?;
    let raw_json = serde_json::to_string_pretty(&raw).ok();
    let u: WhamUsage = serde_json::from_value(raw)
        .map_err(|e| ProviderError::Parse(format!("codex usage: {e}")))?;
    let plan = u.plan_type.as_deref().map(crate::util::capitalize);
    let account = match label_override {
        Some(l) => Some(l.to_string()),
        None => u.email.clone(),
    };
    let (windows, detail_windows) = normalize(&u);
    Ok(ServiceUsage {
        id: auto_service_id(Provider::Codex),
        source: ServiceSource::Auto,
        provider: Provider::Codex,
        connected: true,
        plan,
        account,
        error: None,
        windows,
        detail_windows,
        raw_response: raw_json,
    })
}

/// Refresh a stored Codex OAuth credential (see `refresh::refresh_stored`).
pub(crate) async fn refresh_stored(
    http: &reqwest::Client,
    cred: &crate::store::StoredCredential,
) -> Result<Option<crate::store::StoredCredential>, ProviderError> {
    refresh::refresh_stored(http, cred).await
}

/// Fetch usage for a stored Codex account (uniform stored-fetch adapter).
pub(crate) async fn fetch_stored(
    http: &reqwest::Client,
    cred: &crate::store::StoredCredential,
) -> Result<ServiceUsage, ProviderError> {
    fetch_with(
        http,
        &cred.access_token,
        cred.account_id.as_deref(),
        Some(&cred.label),
    )
    .await
}
