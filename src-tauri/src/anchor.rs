//! Window anchoring: send a minimal throwaway message so a provider's rolling
//! usage window starts at a predictable time. The app's only write path — kept
//! entirely in Rust (tokens never cross IPC). Supported: Claude, Codex, z.ai.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::http;
use crate::model::{stored_service_id, Provider};
use crate::providers::ProviderError;

const ZAI_CHAT_URL: &str = "https://api.z.ai/api/paas/v4/chat/completions";
const ZAI_MODEL: &str = "glm-4-flash";

const CLAUDE_MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
const CLAUDE_VERSION: &str = "2023-06-01";
// Claude Code's OAuth tokens require the OAuth beta flag on the Messages API.
// Verify the exact tag with the guarded test-send (Step 8) and adjust if 401/400.
const CLAUDE_OAUTH_BETA: &str = "oauth-2025-04-20";
const CLAUDE_MODEL: &str = "claude-3-5-haiku-20241022";

/// Cooldown so the auto-trigger sends at most once per window per service.
const COOLDOWN_SECS: i64 = 600;

/// Providers where anchoring is meaningful AND a send path exists.
pub fn supported(provider: Provider) -> bool {
    matches!(provider, Provider::Claude | Provider::Codex | Provider::Zai)
}

/// A minimal 1-token user message body (shared by the chat-completions shapes).
pub fn anchor_body(model: &str) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "max_tokens": 1,
        "messages": [{ "role": "user", "content": "." }]
    })
}

pub async fn send_zai(
    http: &reqwest::Client,
    api_key: &str,
    url: &str,
) -> Result<(), ProviderError> {
    let _: serde_json::Value = http::post_json(http, api_key, url, &anchor_body(ZAI_MODEL)).await?;
    Ok(())
}

pub async fn send_claude(
    http: &reqwest::Client,
    token: &str,
    url: &str,
) -> Result<(), ProviderError> {
    let resp = http
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .header("anthropic-version", CLAUDE_VERSION)
        .header("anthropic-beta", CLAUDE_OAUTH_BETA)
        .header("Content-Type", "application/json")
        .json(&anchor_body(CLAUDE_MODEL))
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    let _ = http::send_for_json(resp, "claude anchor").await?;
    Ok(())
}

/// Extract the current Claude access token from the Claude Code credential store.
fn resolve_claude_auto() -> Result<String, ProviderError> {
    let v = crate::secrets::read_claude_creds_json()?;
    for key in ["claudeAiOauth", "claude_ai_oauth", "oauth"] {
        if let Some(obj) = v.get(key).and_then(|o| o.as_object()) {
            if let Some(tok) = obj
                .get("accessToken")
                .or_else(|| obj.get("access_token"))
                .and_then(|s| s.as_str())
            {
                return Ok(tok.to_string());
            }
        }
    }
    Err(ProviderError::NotLoggedIn(
        "claude creds: no accessToken".into(),
    ))
}

/// Resolve the stored credential whose UI id is `service_id` (`stored:<id>`).
fn resolve_stored(service_id: &str) -> Option<crate::store::StoredCredential> {
    crate::store::list()
        .into_iter()
        .find(|c| stored_service_id(&c.id) == service_id)
}

/// Send an anchor message for the given UI `service_id`.
pub async fn send(service_id: &str) -> Result<(), ProviderError> {
    let http = http::shared();
    if service_id.starts_with("stored:") {
        let cred = resolve_stored(service_id)
            .ok_or_else(|| ProviderError::NotLoggedIn(format!("no stored account {service_id}")))?;
        if !supported(cred.provider) {
            return Err(ProviderError::NotLoggedIn(format!(
                "{:?} does not support anchoring",
                cred.provider
            )));
        }
        return match cred.provider {
            Provider::Zai => send_zai(&http, &cred.access_token, ZAI_CHAT_URL).await,
            Provider::Claude => send_claude(&http, &cred.access_token, CLAUDE_MESSAGES_URL).await,
            // Codex stored send added in a later task.
            other => Err(ProviderError::NotLoggedIn(format!(
                "anchoring for stored {other:?} not implemented yet"
            ))),
        };
    }
    match service_id {
        "auto:claude" => send_claude(&http, &resolve_claude_auto()?, CLAUDE_MESSAGES_URL).await,
        other => Err(ProviderError::NotLoggedIn(format!(
            "anchoring not implemented for {other}"
        ))),
    }
}

// ── Cooldown guard ──────────────────────────────────────────────────────────
static LAST_ANCHOR: Mutex<Option<HashMap<String, i64>>> = Mutex::new(None);

/// Atomically check the cooldown and, if clear, record `now` and return true.
/// Returns false when within the cooldown (caller must NOT send).
pub fn try_begin(service_id: &str, now_sec: i64) -> bool {
    let mut guard = LAST_ANCHOR.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    match map.get(service_id) {
        Some(&t) if now_sec - t < COOLDOWN_SECS => false,
        _ => {
            map.insert(service_id.to_string(), now_sec);
            true
        }
    }
}

/// Roll back a `try_begin` reservation (call when the send failed).
pub fn clear(service_id: &str) {
    if let Some(map) = LAST_ANCHOR
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_mut()
    {
        map.remove(service_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Provider;

    #[test]
    fn supported_only_claude_codex_zai() {
        assert!(supported(Provider::Claude));
        assert!(supported(Provider::Codex));
        assert!(supported(Provider::Zai));
        assert!(!supported(Provider::Copilot));
        assert!(!supported(Provider::Gemini));
        assert!(!supported(Provider::Cursor));
    }

    #[test]
    fn anchor_body_is_one_token_user_message() {
        let b = anchor_body("glm-4-flash");
        assert_eq!(b["model"], "glm-4-flash");
        assert_eq!(b["max_tokens"], 1);
        assert_eq!(b["messages"][0]["role"], "user");
        assert!(b["messages"][0]["content"].is_string());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn send_claude_posts_oauth_bearer_version_and_one_token() {
        use wiremock::matchers::{body_partial_json, header, header_exists, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("authorization", "Bearer oat-test"))
            .and(header("anthropic-version", "2023-06-01"))
            .and(header_exists("anthropic-beta"))
            .and(body_partial_json(serde_json::json!({"max_tokens": 1})))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"id":"msg_x"})),
            )
            .mount(&server)
            .await;
        let client = crate::http::build_client();
        let url = format!("{}/v1/messages", server.uri());
        send_claude(&client, "oat-test", &url).await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn send_zai_posts_bearer_and_one_token_body() {
        use wiremock::matchers::{body_partial_json, header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/paas/v4/chat/completions"))
            .and(header("authorization", "Bearer zk-test"))
            .and(body_partial_json(serde_json::json!({"max_tokens": 1})))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id":"x"})))
            .mount(&server)
            .await;
        let client = crate::http::build_client();
        let url = format!("{}/api/paas/v4/chat/completions", server.uri());
        send_zai(&client, "zk-test", &url).await.unwrap();
    }
}
