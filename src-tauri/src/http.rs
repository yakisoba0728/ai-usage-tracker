//! Shared HTTP client + helpers. A single client with a timeout so a hung
//! provider can never block the scheduler slot indefinitely.

use std::time::Duration;

use crate::providers::ProviderError;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

pub fn build_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .user_agent("ai-usage-tracker/0.1")
        .build()
        .expect("reqwest client")
}

/// A process-wide shared client. `reqwest::Client` pools connections internally
/// and is cheap to clone (it is `Arc`-backed), so every provider and the
/// stored-account refresh path reuse one client instead of building a fresh
/// TLS/DNS stack on each poll. Per-request headers (auth, user-agent overrides)
/// are set per call, so a single shared client is safe.
pub fn shared() -> reqwest::Client {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(build_client).clone()
}

/// Authenticated GET that JSON-decodes the body. `extra` adds per-provider
/// headers (e.g. anthropic-version, chatgpt-account-id).
pub async fn get_json<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    token: &str,
    url: &str,
    extra: &[(&str, &str)],
) -> Result<T, ProviderError> {
    let mut req = client
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/json");
    for (k, v) in extra {
        req = req.header(*k, *v);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    decode_json(resp, url).await
}

/// Read a response to a status/body, mapping non-2xx to ProviderError::Status.
pub async fn send_for_json(
    resp: reqwest::Response,
    url: &str,
) -> Result<serde_json::Value, ProviderError> {
    decode_json(resp, url).await
}

async fn decode_json<T: serde::de::DeserializeOwned>(
    resp: reqwest::Response,
    url: &str,
) -> Result<T, ProviderError> {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        // Don't leak raw HTML (Cloudflare challenges, error pages) to the UI.
        let snippet = if body.trim_start().starts_with('<') {
            match status.as_u16() {
                401 | 403 => "access denied — token expired or invalid".to_string(),
                404 => "endpoint not found".to_string(),
                429 => "rate limited — try again later".to_string(),
                500..=599 => "server error".to_string(),
                _ => "unexpected response".to_string(),
            }
        } else {
            body.chars().take(200).collect()
        };
        return Err(ProviderError::Status {
            status: status.as_u16(),
            body: format!("{url}: {snippet}"),
        });
    }
    serde_json::from_str::<T>(&body).map_err(|e| ProviderError::Parse(format!("{url}: {e}")))
}
