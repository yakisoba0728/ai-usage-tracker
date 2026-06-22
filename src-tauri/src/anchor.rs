//! Window anchoring: send a minimal throwaway message so a provider's rolling
//! usage window starts at a predictable time. The app's only write path — kept
//! entirely in Rust (tokens never cross IPC). Supported: Claude, Codex, z.ai.
//! Claude & z.ai send a 1-token message; Codex sends a minimal turn via the
//! Responses API (no 1-token cap — reasoning models reject it).

use std::collections::HashMap;
use std::sync::Mutex;

use crate::http;
use crate::model::Provider;
use crate::providers::ProviderError;

// Coding-plan endpoint (not the general /api/paas/v4) so the message draws on the
// GLM Coding Plan window the tracker shows. `glm-4-flash` is rejected (code 1211);
// use a current coding model.
const ZAI_CHAT_URL: &str = "https://api.z.ai/api/coding/paas/v4/chat/completions";
const ZAI_MODEL: &str = "glm-4.6";

const CLAUDE_MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
const CLAUDE_VERSION: &str = "2023-06-01";
// Claude Code's OAuth tokens require the OAuth beta flag on the Messages API.
// Verify the exact tag with the guarded test-send (Step 8) and adjust if 401/400.
const CLAUDE_OAUTH_BETA: &str = "oauth-2025-04-20";
// claude-3-5-haiku is retired (404); use a current cheap model.
const CLAUDE_MODEL: &str = "claude-haiku-4-5-20251001";

const CODEX_RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
// ChatGPT-account Codex rejects `codex-mini-latest` ("model is not supported");
// it accepts the current Codex CLI models (e.g. gpt-5.5 — the CLI's own default).
const CODEX_MODEL: &str = "gpt-5.5";
const CODEX_UA: &str = "codex_cli_rs/0.141.0 (ai-usage-tracker)";

/// Per-service cooldown (seconds) so the auto-trigger sends at most once per this
/// interval — NOT once per usage-window (the window is hours long; this only
/// debounces repeat fires while the API still reports the window as empty).
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
    // HTTP 200 is necessary but NOT sufficient: z.ai returns 200 carrying an
    // in-band business error (codes like 1211/1308/1310, or a top-level `error`).
    // Validate the parsed body actually contains a completion before treating the
    // anchor as having consumed the window (BUG-3).
    let body: serde_json::Value =
        http::post_json(http, api_key, url, &anchor_body(ZAI_MODEL)).await?;
    zai_anchor_consumed_window(&body)
}

/// Did a 200 z.ai chat-completions response actually consume the usage window?
/// Requires a real completion (`choices[0].message` present) AND rejects an
/// in-band failure (a top-level `error`, or a non-success business `code`). On
/// rejection returns `ProviderError::Status { status: 200, .. }` with a truncated
/// snippet so a 200-but-failed never clears the cooldown as success (spec §6.3).
///
/// This mirrors the *intent* of Codex's `sse_has_failure` for a JSON (non-SSE)
/// body — the generalized "did the send consume the window" contract.
// reused by Claude web anchor (a later chunk routes the Claude `stored:` arm
// through its own SSE scanner with this same success contract; do not unify the
// two functions — z.ai is JSON, Claude web / Codex are SSE).
fn zai_anchor_consumed_window(body: &serde_json::Value) -> Result<(), ProviderError> {
    let reject = |body: &serde_json::Value| -> ProviderError {
        let snippet: String = body.to_string().chars().take(200).collect();
        ProviderError::Status {
            status: 200,
            body: format!("z.ai anchor: no completion in 200 response: {snippet}"),
        }
    };
    // Reject an explicit in-band error first (present even on some 200s).
    if !body.get("error").map(|e| e.is_null()).unwrap_or(true) {
        return Err(reject(body));
    }
    // A business `code` is success only when it is the 200 "ok" code (z.ai uses
    // HTTP-style 200, or omits `code` entirely on a normal completion).
    if let Some(code) = body.get("code").and_then(|c| c.as_i64()) {
        if code != 200 {
            return Err(reject(body));
        }
    }
    // Require a real completion frame: choices[0].message must be present.
    let has_message = body
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|c| c.get("message"))
        .is_some();
    if has_message {
        Ok(())
    } else {
        Err(reject(body))
    }
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

/// Detect an in-band failure frame in a drained Codex Responses SSE stream. The
/// endpoint can return HTTP 200 and then emit `event: response.failed`,
/// `event: response.incomplete`, or `event: error`, meaning the turn did NOT
/// complete — so the anchor never actually consumed the window (B-12). Matches
/// event names exactly to avoid false positives on response content.
fn sse_has_failure(body: &str) -> bool {
    body.lines().any(|line| {
        if let Some(ev) = line.trim().strip_prefix("event:") {
            matches!(
                ev.trim(),
                "response.failed" | "response.incomplete" | "error"
            )
        } else {
            false
        }
    })
}

pub async fn send_codex(
    http: &reqwest::Client,
    access_token: &str,
    account_id: Option<&str>,
    url: &str,
) -> Result<(), ProviderError> {
    let body = serde_json::json!({
        "model": CODEX_MODEL,
        "instructions": "",
        "input": [{ "role": "user", "content": [{ "type": "input_text", "text": "." }] }],
        "tools": [],
        "tool_choice": "none",
        "parallel_tool_calls": false,
        "store": false,
        "stream": true
    });
    let mut req = http
        .post(url)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("User-Agent", CODEX_UA)
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream");
    if let Some(acc) = account_id {
        req = req.header("ChatGPT-Account-Id", acc);
    }
    let resp = req
        .json(&body)
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    let status = resp.status();
    // Drain the SSE stream so the turn completes (no token in the body).
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        let snippet: String = text.chars().take(200).collect();
        return Err(ProviderError::Status {
            status: status.as_u16(),
            body: format!("codex anchor: {snippet}"),
        });
    }
    // HTTP 200 is necessary but not sufficient: the stream can still carry an
    // in-band failure frame, in which case the turn never ran (B-12).
    if sse_has_failure(&text) {
        let snippet: String = text.chars().take(200).collect();
        return Err(ProviderError::Status {
            status: status.as_u16(),
            body: format!("codex anchor: stream reported failure: {snippet}"),
        });
    }
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

/// Resolve the stored credential whose UI id is `service_id` (`stored:<id>`).
fn resolve_stored(service_id: &str) -> Option<crate::store::StoredCredential> {
    crate::store::find_by_service_id(service_id)
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
        // Refresh an expired token before sending, so a manual "Anchor now" is
        // never weaker than the poll path (which refreshes via fetch_credential)
        // and doesn't 401 on a stale bearer (B-11).
        let cred = crate::providers::refresh_if_expired(&http, &cred).await?;
        return match cred.provider {
            Provider::Zai => send_zai(&http, &cred.access_token, ZAI_CHAT_URL).await,
            Provider::Claude => send_claude(&http, &cred.access_token, CLAUDE_MESSAGES_URL).await,
            Provider::Codex => {
                send_codex(
                    &http,
                    &cred.access_token,
                    cred.account_id.as_deref(),
                    CODEX_RESPONSES_URL,
                )
                .await
            }
            other => Err(ProviderError::NotLoggedIn(format!(
                "anchoring for stored {other:?} not implemented yet"
            ))),
        };
    }
    match service_id {
        "auto:claude" => send_claude(&http, &resolve_claude_auto()?, CLAUDE_MESSAGES_URL).await,
        "auto:zai" => send_zai(&http, &resolve_zai_auto()?, ZAI_CHAT_URL).await,
        "auto:codex" => {
            let (access, account_id) = resolve_codex_auto()?;
            send_codex(&http, &access, account_id.as_deref(), CODEX_RESPONSES_URL).await
        }
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

/// Whether an anchor-send failure is transient (worth an immediate retry) vs a
/// durable rejection (a retired model → 4xx, a missing credential, a parse
/// error). Only transient failures should roll back the cooldown; a durable one
/// must keep it so the auto-anchor doesn't retry-storm a failing provider every
/// poll (B-3 / B-13).
pub fn failure_is_transient(e: &ProviderError) -> bool {
    matches!(e, ProviderError::Network(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Provider;

    #[test]
    fn cooldown_allows_first_blocks_within_window_and_clears() {
        let id = "test:cooldown:unique-xyz";
        clear(id); // no prior state
        let t0 = 1_000_000i64;
        assert!(try_begin(id, t0), "first send allowed");
        assert!(!try_begin(id, t0 + 1), "within cooldown blocked");
        assert!(
            !try_begin(id, t0 + COOLDOWN_SECS - 1),
            "still within cooldown"
        );
        assert!(
            try_begin(id, t0 + COOLDOWN_SECS),
            "at/after cooldown allowed"
        );
        clear(id);
        assert!(try_begin(id, t0 + 2), "after clear, allowed again");
        clear(id); // cleanup
    }

    #[test]
    fn only_network_failures_are_transient() {
        assert!(failure_is_transient(&ProviderError::Network(
            "timeout".into()
        )));
        // Durable rejections must NOT clear the cooldown (else retry-storm).
        assert!(!failure_is_transient(&ProviderError::Status {
            status: 400,
            body: "model is not supported".into(),
        }));
        assert!(!failure_is_transient(&ProviderError::NotLoggedIn(
            "x".into()
        )));
        assert!(!failure_is_transient(&ProviderError::Parse("x".into())));
        assert!(!failure_is_transient(&ProviderError::Expired("x".into())));
    }

    #[test]
    fn supported_is_claude_codex_zai() {
        assert!(supported(Provider::Claude));
        assert!(supported(Provider::Codex));
        assert!(supported(Provider::Zai));
        assert!(!supported(Provider::Copilot));
        assert!(!supported(Provider::Gemini));
        assert!(!supported(Provider::Cursor));
    }

    #[test]
    fn sse_has_failure_detects_in_band_failure_frames_only() {
        // Success / non-failure streams → false.
        assert!(!sse_has_failure("event: response.completed\ndata: {}\n\n"));
        assert!(!sse_has_failure("event: done\ndata: {}\n\n")); // the existing test body
        assert!(!sse_has_failure("data: {\"text\":\"hi\"}\n\n"));
        // In-band failure frames (HTTP 200 but the turn failed) → true.
        assert!(sse_has_failure(
            "event: response.failed\ndata: {\"error\":\"x\"}\n\n"
        ));
        assert!(sse_has_failure("event: error\ndata: {}\n\n"));
        assert!(sse_has_failure("event: response.incomplete\ndata: {}\n\n"));
        // Tolerates leading whitespace on the event line.
        assert!(sse_has_failure("  event: response.failed\n"));
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
            .and(path("/api/coding/paas/v4/chat/completions"))
            .and(header("authorization", "Bearer zk-test"))
            .and(body_partial_json(serde_json::json!({"max_tokens": 1})))
            // A real completion shape (choices[0].message present) — a bare
            // {"id":"x"} is NO LONGER a success per the body-check validator (BUG-3).
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "x",
                "choices": [{ "message": { "role": "assistant", "content": "" } }]
            })))
            .mount(&server)
            .await;
        let client = crate::http::build_client();
        let url = format!("{}/api/coding/paas/v4/chat/completions", server.uri());
        send_zai(&client, "zk-test", &url).await.unwrap();
    }

    // ── BUG-3: z.ai 200-with-in-band-error must NOT read as a successful anchor ──

    #[test]
    fn zai_body_check_accepts_a_real_completion() {
        // A 200 carrying a genuine completion (choices[0].message) consumed the
        // window → Ok.
        let ok = serde_json::json!({
            "id": "chatcmpl-1",
            "choices": [{ "message": { "role": "assistant", "content": "." } }]
        });
        assert!(zai_anchor_consumed_window(&ok).is_ok());
    }

    #[test]
    fn zai_body_check_rejects_in_band_error_code() {
        // z.ai returns HTTP 200 + a business error code (1211/1308/1310) when the
        // request was rejected — the window was never consumed.
        for code in [1211_i64, 1308, 1310] {
            let v = serde_json::json!({ "code": code, "msg": "model not supported" });
            let err = zai_anchor_consumed_window(&v).expect_err("in-band error code must fail");
            assert!(matches!(err, ProviderError::Status { status: 200, .. }));
        }
    }

    #[test]
    fn zai_body_check_rejects_top_level_error_object() {
        let v = serde_json::json!({ "error": { "message": "quota exceeded" } });
        let err = zai_anchor_consumed_window(&v).expect_err("top-level error must fail");
        assert!(matches!(err, ProviderError::Status { status: 200, .. }));
    }

    #[test]
    fn zai_body_check_rejects_completion_without_message() {
        // No choices / no message → did not complete.
        let v = serde_json::json!({ "id": "x" });
        assert!(zai_anchor_consumed_window(&v).is_err());
        let v2 = serde_json::json!({ "choices": [] });
        assert!(zai_anchor_consumed_window(&v2).is_err());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn send_zai_rejects_200_with_in_band_error() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        // HTTP 200 but a business error code — must NOT clear the cooldown as a
        // success (the window was never consumed).
        Mock::given(method("POST"))
            .and(path("/api/coding/paas/v4/chat/completions"))
            .and(header("authorization", "Bearer zk-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 1211,
                "msg": "glm-4-flash is not supported"
            })))
            .mount(&server)
            .await;
        let client = crate::http::build_client();
        let url = format!("{}/api/coding/paas/v4/chat/completions", server.uri());
        let err = send_zai(&client, "zk-test", &url)
            .await
            .expect_err("200 + in-band error must be Err");
        assert!(matches!(err, ProviderError::Status { status: 200, .. }));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn send_codex_posts_bearer_account_and_streams() {
        use wiremock::matchers::{body_partial_json, header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/backend-api/codex/responses"))
            .and(header("authorization", "Bearer cx"))
            .and(header("chatgpt-account-id", "acc"))
            .and(header("accept", "text/event-stream"))
            .and(body_partial_json(serde_json::json!({"stream": true})))
            .respond_with(ResponseTemplate::new(200).set_body_string("event: done\ndata: {}\n\n"))
            .mount(&server)
            .await;
        let client = crate::http::build_client();
        let url = format!("{}/backend-api/codex/responses", server.uri());
        send_codex(&client, "cx", Some("acc"), &url).await.unwrap();
    }
}
