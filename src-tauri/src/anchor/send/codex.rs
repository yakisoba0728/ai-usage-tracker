//! Codex anchor send: a minimal streamed turn via the Responses API, draining
//! the SSE so the turn completes and scanning for an in-band failure (B-12).

use crate::providers::ProviderError;

// ChatGPT-account Codex rejects `codex-mini-latest` ("model is not supported");
// it accepts the current Codex CLI models (e.g. gpt-5.5 — the CLI's own default).
const CODEX_MODEL: &str = "gpt-5.5";
const CODEX_UA: &str = "codex_cli_rs/0.141.0 (ai-usage-tracker)";

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

#[cfg(test)]
mod tests {
    use super::*;

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
