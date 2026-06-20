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
        let snippet = sanitize_error_body(status.as_u16(), &body);
        return Err(ProviderError::Status {
            status: status.as_u16(),
            body: format!("{url}: {snippet}"),
        });
    }
    serde_json::from_str::<T>(&body).map_err(|e| ProviderError::Parse(format!("{url}: {e}")))
}

/// Build a safe error snippet for a non-2xx response body. Raw HTML (Cloudflare
/// challenges, provider error pages) is redacted to a short status-specific
/// message so it never reaches the UI; any other body passes through truncated
/// to 200 chars.
fn sanitize_error_body(status: u16, body: &str) -> String {
    if body.trim_start().starts_with('<') {
        match status {
            401 | 403 => "access denied — token expired or invalid".to_string(),
            404 => "endpoint not found".to_string(),
            429 => "rate limited — try again later".to_string(),
            500..=599 => "server error".to_string(),
            _ => "unexpected response".to_string(),
        }
    } else {
        body.chars().take(200).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_html_by_status() {
        let html = "<!DOCTYPE html><html>cloudflare challenge</html>";
        assert_eq!(
            sanitize_error_body(401, html),
            "access denied — token expired or invalid"
        );
        assert_eq!(
            sanitize_error_body(403, html),
            "access denied — token expired or invalid"
        );
        assert_eq!(sanitize_error_body(404, html), "endpoint not found");
        assert_eq!(
            sanitize_error_body(429, html),
            "rate limited — try again later"
        );
        assert_eq!(sanitize_error_body(503, html), "server error");
        assert_eq!(sanitize_error_body(418, html), "unexpected response");
    }

    #[test]
    fn detects_html_after_leading_whitespace() {
        assert_eq!(
            sanitize_error_body(403, "   \n<html></html>"),
            "access denied — token expired or invalid"
        );
    }

    #[test]
    fn passes_non_html_through_truncated_to_200() {
        assert_eq!(
            sanitize_error_body(400, r#"{"error":"bad request"}"#),
            r#"{"error":"bad request"}"#
        );
        let long = "x".repeat(500);
        assert_eq!(sanitize_error_body(400, &long).chars().count(), 200);
    }

    // ── Live HTTP behavior against a local mock server (shared by every
    //    provider through get_json / send_for_json). ──────────────────────────

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_json_decodes_200_and_sends_bearer_auth() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/usage"))
            .and(header("authorization", "Bearer tok-123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"k": 42})))
            .mount(&server)
            .await;

        #[derive(serde::Deserialize)]
        struct Resp {
            k: i64,
        }
        let client = build_client();
        // If the Authorization header were missing/wrong the mock wouldn't match
        // and this would error instead of decoding — so success proves the header.
        let r: Resp = get_json(&client, "tok-123", &format!("{}/usage", server.uri()), &[])
            .await
            .unwrap();
        assert_eq!(r.k, 42);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_json_redacts_html_error_body() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/usage"))
            .respond_with(
                ResponseTemplate::new(401)
                    .set_body_string("<!DOCTYPE html><html>blocked by cloudflare</html>"),
            )
            .mount(&server)
            .await;

        let client = build_client();
        let err =
            get_json::<serde_json::Value>(&client, "t", &format!("{}/usage", server.uri()), &[])
                .await
                .unwrap_err();
        match err {
            ProviderError::Status { status, body } => {
                assert_eq!(status, 401);
                assert!(body.contains("access denied"), "got: {body}");
                assert!(!body.contains("DOCTYPE"), "raw HTML leaked: {body}");
            }
            other => panic!("expected Status, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_json_maps_invalid_json_to_parse_error() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;

        let client = build_client();
        let err =
            get_json::<serde_json::Value>(&client, "t", &format!("{}/usage", server.uri()), &[])
                .await
                .unwrap_err();
        assert!(matches!(err, ProviderError::Parse(_)), "got: {err:?}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn send_for_json_decodes_a_post_response() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
            .mount(&server)
            .await;

        let client = build_client();
        let resp = client
            .post(format!("{}/x", server.uri()))
            .send()
            .await
            .unwrap();
        let v = send_for_json(resp, "test").await.unwrap();
        assert_eq!(v["ok"], true);
    }
}
