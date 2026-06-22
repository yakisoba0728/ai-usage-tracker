//! z.ai anchor send: post a 1-token message to the GLM Coding Plan window and
//! validate the 200 body actually carries a completion (BUG-3).

use crate::http;
use crate::providers::ProviderError;

use super::super::anchor_body;

const ZAI_MODEL: &str = "glm-4.6";

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
