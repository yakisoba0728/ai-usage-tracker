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
/// Codex is excluded: reset credits are finite and manual-only (see
/// `reset_codex_for`); the auto/message anchor path is SSE-only and fragile.
pub fn supported(provider: Provider) -> bool {
    matches!(provider, Provider::Claude | Provider::Zai)
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

/// Resolve the z.ai API key from the environment.
fn resolve_zai_auto() -> Result<String, ProviderError> {
    match std::env::var("ZAI_API_KEY") {
        Ok(k) if !k.trim().is_empty() => Ok(k.trim().to_string()),
        _ => Err(ProviderError::NotLoggedIn("z.ai API key not set".into())),
    }
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
        "auto:zai" => send_zai(&http, &resolve_zai_auto()?, ZAI_CHAT_URL).await,
        other => Err(ProviderError::NotLoggedIn(format!(
            "anchoring not implemented for {other}"
        ))),
    }
}

// ── Codex manual reset-credit ───────────────────────────────────────────────

const CODEX_RESET_URL: &str =
    "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits/consume";
const CODEX_UA: &str = "codex_cli_rs/0.141.0 (ai-usage-tracker)";

/// Consume one Codex rate-limit-reset credit (the official action the Codex CLI
/// uses). Returns the response `code` ("reset" | "nothing_to_reset" | "no_credit"
/// | "already_redeemed"). A fresh idempotency key per call so each deliberate
/// click attempts a reset.
pub async fn reset_codex(
    http: &reqwest::Client,
    access_token: &str,
    account_id: Option<&str>,
    url: &str,
) -> Result<String, ProviderError> {
    let redeem = format!(
        "ait-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    );
    let mut req = http
        .post(url)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("User-Agent", CODEX_UA)
        .header("Content-Type", "application/json");
    if let Some(acc) = account_id {
        req = req.header("ChatGPT-Account-Id", acc);
    }
    let resp = req
        .json(&serde_json::json!({ "redeem_request_id": redeem }))
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    let v = http::send_for_json(resp, "codex reset-credit").await?;
    Ok(v.get("code")
        .and_then(|c| c.as_str())
        .unwrap_or("unknown")
        .to_string())
}

fn resolve_codex_auto() -> Result<(String, Option<String>), ProviderError> {
    let v = crate::secrets::read_json_file(&crate::secrets::codex_auth_path())?;
    let tokens = v
        .get("tokens")
        .ok_or_else(|| ProviderError::NotLoggedIn("codex auth.json: no tokens".into()))?;
    let access = tokens
        .get("access_token")
        .and_then(|s| s.as_str())
        .ok_or_else(|| ProviderError::NotLoggedIn("codex: no access_token".into()))?;
    let account_id = tokens
        .get("account_id")
        .and_then(|s| s.as_str())
        .map(String::from);
    Ok((access.to_string(), account_id))
}

/// Manual Codex reset-credit for a UI service id (`auto:codex` or `stored:<id>`).
pub async fn reset_codex_for(service_id: &str) -> Result<String, ProviderError> {
    let http = http::shared();
    let (access, account_id) = if service_id.starts_with("stored:") {
        let cred = resolve_stored(service_id)
            .ok_or_else(|| ProviderError::NotLoggedIn(format!("no stored account {service_id}")))?;
        (cred.access_token, cred.account_id)
    } else {
        resolve_codex_auto()?
    };
    reset_codex(&http, &access, account_id.as_deref(), CODEX_RESET_URL).await
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
    fn supported_is_claude_and_zai_only() {
        assert!(supported(Provider::Claude));
        assert!(supported(Provider::Zai));
        // Codex is manual reset-credit only (finite credits) — no auto/message anchor.
        assert!(!supported(Provider::Codex));
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

    #[test]
    fn resolve_zai_auto_reads_env_or_errors() {
        std::env::set_var("ZAI_API_KEY", "zk-env-123");
        assert_eq!(resolve_zai_auto().unwrap(), "zk-env-123");
        std::env::remove_var("ZAI_API_KEY");
        assert!(resolve_zai_auto().is_err());
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reset_codex_posts_redeem_id_and_returns_code() {
        use wiremock::matchers::{body_partial_json, header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/backend-api/wham/rate-limit-reset-credits/consume"))
            .and(header("authorization", "Bearer cx-test"))
            .and(header("chatgpt-account-id", "acc-1"))
            .and(body_partial_json(serde_json::json!({})))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"code":"reset","windows_reset":2})),
            )
            .mount(&server)
            .await;
        let client = crate::http::build_client();
        let url = format!(
            "{}/backend-api/wham/rate-limit-reset-credits/consume",
            server.uri()
        );
        let code = reset_codex(&client, "cx-test", Some("acc-1"), &url)
            .await
            .unwrap();
        assert_eq!(code, "reset");
    }
}
