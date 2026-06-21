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
            // Claude/Codex stored sends are added in later tasks.
            other => Err(ProviderError::NotLoggedIn(format!(
                "anchoring for stored {other:?} not implemented yet"
            ))),
        };
    }
    Err(ProviderError::NotLoggedIn(format!(
        "anchoring not implemented for {service_id}"
    )))
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
