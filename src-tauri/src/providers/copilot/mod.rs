//! GitHub Copilot — reads the Copilot CLI's stored token (macOS Keychain
//! `copilot-cli`, else `~/.copilot/config.json`) and calls the internal
//! `GET /copilot_internal/user` endpoint, which returns the `quota_snapshots`
//! gauge (chat / premium_interactions / completions) plus `quota_reset_date`.
//!
//! Verified against `vbgate/opencode-mystatus` (MIT, the reference cited in the
//! README): `/copilot_internal/v2/token` is a *token-exchange* endpoint that
//! returns `{token, expires_at, refresh_in, endpoints}` — NOT the quota source.
//! The quota lives at `/copilot_internal/user`. (The previous code hit the
//! token endpoint and would never have parsed `quota_snapshots` from a live
//! response.) For manual accounts use a fine-grained PAT with "Copilot
//! Requests: Read" permission (paste via Add account); `Authorization: token
//! <key>` works for both Copilot CLI OAuth tokens and PATs on this endpoint.

mod creds;
mod parse;

use async_trait::async_trait;
use serde_json::Value;

use crate::http;
use crate::model::{auto_service_id, Provider, ServiceSource, ServiceUsage};
use crate::providers::ProviderError;

use creds::read_copilot_token;
use parse::{normalize, CopilotUsageResp};

/// Quota source per `vbgate/opencode-mystatus` `plugin/lib/copilot.ts`
/// (Strategy 2 — direct call with the OAuth/PAT token using the legacy
/// `Authorization: token <t>` scheme).
const USAGE_URL: &str = "https://api.github.com/copilot_internal/user";

/// Header values mirror the current VS Code Copilot extension (Jan 2026),
/// per `vbgate/opencode-mystatus` `plugin/lib/copilot.ts`. These identify the
/// client as VS Code Copilot Chat, which `/copilot_internal/user` expects.
const EDITOR_VERSION: &str = "vscode/1.107.0";
const EDITOR_PLUGIN_VERSION: &str = "copilot-chat/0.35.0";
const USER_AGENT: &str = "GitHubCopilotChat/0.35.0";

pub struct CopilotProvider {
    http: reqwest::Client,
}

impl Default for CopilotProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CopilotProvider {
    pub fn new() -> Self {
        Self {
            http: http::shared(),
        }
    }
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
/// or a manually-pasted PAT). Calls `GET /copilot_internal/user`.
pub(crate) async fn fetch_with(
    http: &reqwest::Client,
    token: &str,
) -> Result<ServiceUsage, ProviderError> {
    let resp = http
        .get(USAGE_URL)
        .header("Authorization", format!("token {token}"))
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("User-Agent", USER_AGENT)
        .header("Editor-Version", EDITOR_VERSION)
        .header("Editor-Plugin-Version", EDITOR_PLUGIN_VERSION)
        .header("Copilot-Integration-Id", "vscode-chat")
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    let val: Value = http::send_for_json(resp, USAGE_URL).await?;
    let raw_json = serde_json::to_string_pretty(&val).ok();
    let u: CopilotUsageResp =
        serde_json::from_value(val).map_err(|e| ProviderError::Parse(e.to_string()))?;
    let (windows, detail_windows) = normalize(&u);
    Ok(ServiceUsage {
        id: auto_service_id(Provider::Copilot),
        source: ServiceSource::Auto,
        provider: Provider::Copilot,
        connected: true,
        plan: u.copilot_plan,
        account: u.login,
        error: None,
        windows,
        detail_windows,
        raw_response: raw_json,
    })
}

/// Refresh a stored credential's access_token using its refresh_token.
///
/// Returns `None` (no-op) for Copilot: the `gho_…` user OAuth tokens the
/// Copilot CLI stores in `copilot-cli` / `~/.copilot/config.json` **do not
/// expire and have no refresh grant** — they persist until revoked, and the
/// device-code flow the CLI uses (`cli/cli` `internal/authflow/flow.go`,
/// client_id `178c6fc778ccc68e1d6a`) does not return a `refresh_token`. Only
/// GitHub-App-installed tokens can expire/refresh, and the Copilot CLI does not
/// issue those. PATs are similarly non-expiring. The caller therefore falls
/// back to the existing token. See `docs/oauth-credential-research.md` §4.B.
pub(crate) async fn refresh_stored(
    _http: &reqwest::Client,
    _cred: &crate::store::StoredCredential,
) -> Option<crate::store::StoredCredential> {
    None
}

/// Fetch usage for a stored Copilot account (uniform stored-fetch adapter).
pub(crate) async fn fetch_stored(
    http: &reqwest::Client,
    cred: &crate::store::StoredCredential,
) -> Result<ServiceUsage, ProviderError> {
    fetch_with(http, &cred.access_token).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn refresh_stored_is_noop() {
        // Copilot tokens don't expire refreshably; refresh is a documented no-op.
        let http = crate::http::build_client();
        let cred = crate::store::StoredCredential {
            id: "x".into(),
            provider: Provider::Copilot,
            label: "test".into(),
            access_token: "gho_x".into(),
            refresh_token: None,
            expires_at: 0,
            id_token: None,
            account_id: None,
        };
        assert!(refresh_stored(&http, &cred).await.is_none());
    }
}
