//! Claude.ai web API client for manually-added session-key accounts (no OAuth /
//! CLI involved). Split out of the OAuth/CLI path in `super`.
//!
//! Two cookie-authenticated paths share the org lookup + cookie helpers:
//! - READ (`fetch_with_session_key`): GET organizations + usage → a `ServiceUsage`.
//! - WRITE (`send_claude_web`): the window anchor — orgs → create a throwaway
//!   conversation → POST a minimal completion, draining the SSE for a success
//!   frame. The anchor is the app's only write to claude.ai (FEAT-2 / BUG-1).

use serde::Deserialize;

use super::format_plan;
use crate::model::{auto_service_id, LimitWindow, Provider, ServiceSource, ServiceUsage};
use crate::providers::ProviderError;

/// Production base for every claude.ai API call (`{base}/organizations/...`).
/// A param on the request fns so the wiremock tests can point the whole 3-call
/// anchor sequence at a local mock (every `send_*` takes a URL for this reason).
pub(crate) const CLAUDE_WEB_API_BASE: &str = "https://claude.ai/api";

/// A realistic desktop User-Agent for the write path. claude.ai's bot protection
/// (Cloudflare) is sensitive to a non-browser UA; the read path doesn't write so
/// it can stay on the default client UA, but the completion POST mimics a browser.
const WEB_UA: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

/// The fixed root-message parent for the first turn of a fresh conversation
/// (claude.ai's "no parent" sentinel — a nil-ish v4 uuid).
const ROOT_PARENT_MESSAGE_UUID: &str = "00000000-0000-4000-8000-000000000000";

/// Shape verified against the live `/api/organizations` response: each org
/// carries `rate_limit_tier` directly and `memberships[].account.email_address`
/// inline, so no separate `/account` round-trip is needed (the shape that call
/// used to assume was wrong and always degraded to no email / no tier).
#[derive(Deserialize)]
struct WebOrg {
    uuid: String, // the identifier the usage endpoint expects
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    rate_limit_tier: Option<String>,
    #[serde(default)]
    memberships: Vec<WebMembership>,
}
#[derive(Deserialize)]
struct WebMembership {
    #[serde(default)]
    account: Option<WebMemberAccount>,
}
#[derive(Deserialize)]
struct WebMemberAccount {
    #[serde(default, rename = "email_address")]
    email_address: Option<String>,
}
#[derive(Deserialize)]
struct WebWindow {
    #[serde(default)]
    utilization: Option<f64>, // claude.ai returns int percent 0-100
    #[serde(default)]
    resets_at: Option<chrono::DateTime<chrono::Utc>>,
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "snake_case")]
struct WebUsage {
    #[serde(default)]
    five_hour: Option<WebWindow>,
    #[serde(default)]
    seven_day: Option<WebWindow>,
    #[serde(default)]
    seven_day_sonnet: Option<WebWindow>,
    #[serde(default)]
    seven_day_opus: Option<WebWindow>,
}

/// GET `{api_base}/organizations` with the given `Cookie` header and return the
/// FIRST organization (shared by the read + write paths). The read path needs
/// the whole `WebOrg` (email + tier come off it); the anchor needs only `.uuid`.
async fn fetch_first_org(
    http: &reqwest::Client,
    cookie: &str,
    api_base: &str,
) -> Result<WebOrg, ProviderError> {
    let resp = http
        .get(format!("{api_base}/organizations"))
        .header("Cookie", cookie)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    let v: serde_json::Value = crate::http::send_for_json(resp, "claude.ai/organizations").await?;
    let orgs: Vec<WebOrg> =
        serde_json::from_value(v).map_err(|e| ProviderError::Parse(format!("orgs: {e}")))?;
    orgs.into_iter()
        .next()
        .ok_or_else(|| ProviderError::Parse("no claude organization".into()))
}

/// Fetch Claude usage via the claude.ai web API using a `sessionKey` cookie
/// (manually-added session-key account). No OAuth / CLI involved.
pub(crate) async fn fetch_with_session_key(
    http: &reqwest::Client,
    session_key: &str,
) -> Result<ServiceUsage, ProviderError> {
    let cookie = format!("sessionKey={}", session_key_cookie_value(session_key));

    let org = fetch_first_org(http, &cookie, CLAUDE_WEB_API_BASE).await?;

    let resp = http
        .get(format!(
            "{CLAUDE_WEB_API_BASE}/organizations/{}/usage",
            org.uuid
        ))
        .header("Cookie", &cookie)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    let v: serde_json::Value = crate::http::send_for_json(resp, "claude.ai/usage").await?;
    let raw_json = serde_json::to_string_pretty(&v).ok();
    let u: WebUsage =
        serde_json::from_value(v).map_err(|e| ProviderError::Parse(format!("usage: {e}")))?;

    // Email + plan tier come straight off the org object we already fetched.
    let email = org
        .memberships
        .iter()
        .find_map(|m| m.account.as_ref())
        .and_then(|a| a.email_address.clone());

    let win = |label: &str, w: &Option<WebWindow>| -> Option<LimitWindow> {
        w.as_ref().map(|w| LimitWindow {
            label: label.into(),
            used_percent: w.utilization.map(|v| v as f32),
            resets_at: w.resets_at.map(|d| d.timestamp()),
            used: None,
            limit: None,
        })
    };
    let mut windows = vec![];
    let mut detail = vec![];
    if let Some(x) = win("5-hour", &u.five_hour) {
        windows.push(x);
    }
    if let Some(x) = win("7-day", &u.seven_day) {
        windows.push(x);
    }
    if let Some(x) = win("7-day (Sonnet)", &u.seven_day_sonnet) {
        detail.push(x);
    }
    if let Some(x) = win("7-day (Opus)", &u.seven_day_opus) {
        detail.push(x);
    }
    Ok(ServiceUsage {
        id: auto_service_id(Provider::Claude),
        source: ServiceSource::Auto,
        provider: Provider::Claude,
        connected: true,
        plan: format_plan(&org.rate_limit_tier, &None).or(org.name),
        account: email,
        error: None,
        windows,
        detail_windows: detail,
        raw_response: raw_json,
    })
}

pub(super) fn session_key_cookie_value(input: &str) -> String {
    let trimmed = input.trim();
    let payload = trimmed
        .strip_prefix("Cookie:")
        .or_else(|| trimmed.strip_prefix("cookie:"))
        .unwrap_or(trimmed)
        .trim();
    for part in payload.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("sessionKey=") {
            return value.trim().to_string();
        }
    }
    payload.to_string()
}

/// Build the `Cookie:` header VALUE the WRITE path forwards, preserving any
/// Cloudflare cookies the user pasted.
///
/// claude.ai's completion (write) endpoint sits behind Cloudflare bot
/// protection that can require `cf_clearance` / `__cf_bm` alongside the
/// `sessionKey`. The read path only needs `sessionKey`, but for the write path
/// we must forward whatever Cloudflare cookies are present:
/// - If the user pasted a FULL `Cookie:` header (it carries `cf_clearance` or
///   `__cf_bm` next to `sessionKey`), forward the WHOLE cookie string verbatim.
/// - Otherwise (a bare session key, or a value with no CF cookies), send the
///   canonical `sessionKey=<value>`.
///
/// Distinct from `session_key_cookie_value` (which extracts the bare key for the
/// read path) — kept separate so the read path's extraction stays pinned.
pub(super) fn full_cookie_header(input: &str) -> String {
    let trimmed = input.trim();
    let payload = trimmed
        .strip_prefix("Cookie:")
        .or_else(|| trimmed.strip_prefix("cookie:"))
        .unwrap_or(trimmed)
        .trim();
    // A pasted multi-cookie header carrying Cloudflare clearance → forward as-is.
    let has_cf = payload
        .split(';')
        .any(|p| {
            let p = p.trim();
            p.starts_with("cf_clearance=") || p.starts_with("__cf_bm=")
        });
    if has_cf {
        return payload.to_string();
    }
    // Bare key (or no CF cookies present): canonical sessionKey=<value>.
    format!("sessionKey={}", session_key_cookie_value(input))
}

/// Generate a random RFC 4122 v4 UUID string (lowercased, hyphenated). Uses the
/// same `rand::rng().fill_bytes` idiom as `oauth::pkce` (rand 0.10), then sets
/// the version (4) and variant (10xx) bits. Used for the throwaway conversation
/// id and the per-install `device_id`.
pub(crate) fn uuid_v4() -> String {
    use rand::Rng;
    let mut b = [0u8; 16];
    rand::rng().fill_bytes(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // variant 10xx
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13],
        b[14], b[15]
    )
}

/// Map a non-2xx response on the WRITE path to a `ProviderError::Status` with a
/// safe, truncated snippet. A 403 is almost always Cloudflare bot-protection
/// rejecting the write (the read path can still succeed), so its detail tells
/// the user to refresh their Claude cookie with a full Cookie header. Applied to
/// BOTH write POSTs (create-conversation AND completion) — the create call is the
/// FIRST write, so it's the most likely to draw the challenge. `Status` is
/// non-transient, so the cooldown is kept (`failure_is_transient` unchanged).
fn write_status_to_error(status: reqwest::StatusCode, body: &str, step: &str) -> ProviderError {
    let snippet: String = crate::util::scrub_sensitive_text(body)
        .chars()
        .take(200)
        .collect();
    let detail = if status.as_u16() == 403 {
        format!(
            "claude.ai anchor blocked (403 at {step}) — refresh your Claude cookie \
             (paste a fresh full Cookie header incl. cf_clearance): {snippet}"
        )
    } else {
        format!("claude.ai anchor ({step}): {snippet}")
    };
    ProviderError::Status {
        status: status.as_u16(),
        body: detail,
    }
}

/// Did a drained claude.ai completion SSE stream actually run the turn?
///
/// Unlike Codex's `sse_has_failure` (absence-of-failure), claude.ai's write path
/// needs a POSITIVE success signal: success = at least one assistant data frame
/// (`completion` / `message_start` / `message_delta`) AND no error frame
/// (`event: error` or a data payload carrying a top-level `"error"`). Kept a
/// distinct scanner from the z.ai JSON validator and the Codex SSE check (spec
/// §6.3 — three shapes, one intent).
fn web_sse_consumed_window(body: &str) -> bool {
    let mut saw_success = false;
    let mut saw_error = false;
    for line in body.lines() {
        let line = line.trim();
        if let Some(ev) = line.strip_prefix("event:") {
            let ev = ev.trim();
            if ev == "error" {
                saw_error = true;
            } else if matches!(ev, "completion" | "message_start" | "message_delta") {
                saw_success = true;
            }
        } else if let Some(data) = line.strip_prefix("data:") {
            let data = data.trim();
            // A data frame carrying a top-level error object → the turn failed
            // even on HTTP 200 (claude.ai sometimes streams `{"error":{...}}`).
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                if v.get("error").map(|e| !e.is_null()).unwrap_or(false) {
                    saw_error = true;
                }
                if let Some(t) = v.get("type").and_then(|t| t.as_str()) {
                    if matches!(t, "completion" | "message_start" | "message_delta") {
                        saw_success = true;
                    }
                }
            }
        }
    }
    saw_success && !saw_error
}

/// Send a window anchor through the claude.ai WEB chat completion endpoint
/// (session-key cookie auth; the app's only write to claude.ai — FEAT-2/BUG-1).
///
/// Three cookie-authenticated steps:
///   a. GET `{api_base}/organizations` → first org `uuid`.
///   b. POST `{api_base}/organizations/{uuid}/chat_conversations`
///      `{"uuid": <client v4>, "name": ""}` → conversation uuid.
///   c. POST `.../chat_conversations/{conv}/completion` with a minimal body,
///      then drain the `text/event-stream` for a success frame.
///
/// `cookie_value` is the user's pasted session key OR full Cookie header (the
/// full-cookie forward in `full_cookie_header` preserves Cloudflare cookies on
/// the write path). `device_id` is the stable per-install `anthropic-device-id`.
///
/// On a non-2xx response returns `ProviderError::Status` with a truncated
/// snippet; a 403 (Cloudflare bot-protection) is surfaced with a "refresh your
/// Claude cookie" hint. `Status` is already non-transient, so the cooldown is
/// kept (`failure_is_transient` unchanged).
pub(crate) async fn send_claude_web(
    http: &reqwest::Client,
    cookie_value: &str,
    device_id: &str,
    api_base: &str,
) -> Result<(), ProviderError> {
    let cookie = full_cookie_header(cookie_value);

    // a. Organization uuid (reuses the shared org fetch).
    let org_uuid = fetch_first_org(http, &cookie, api_base).await?.uuid;

    // b. Create a throwaway conversation. claude.ai expects a client-generated
    //    v4 uuid + a name (empty is accepted for a "new chat").
    let conv_uuid = uuid_v4();
    let create_url = format!("{api_base}/organizations/{org_uuid}/chat_conversations");
    let create_resp = http
        .post(&create_url)
        .header("Cookie", &cookie)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .header("anthropic-client-platform", "web_claude_ai")
        .header("anthropic-device-id", device_id)
        .header("Origin", "https://claude.ai")
        .header("Referer", "https://claude.ai/new")
        .header("User-Agent", WEB_UA)
        .json(&serde_json::json!({ "uuid": conv_uuid, "name": "" }))
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    // The create POST is the FIRST write to claude.ai → the call most likely to
    // draw a Cloudflare 403. Classify the status with the SAME write-path helper
    // the completion uses (so a 403 here also says "refresh your cookie", not the
    // generic send_for_json "access denied"). We generate the conversation uuid
    // client-side, so a 2xx body is just drained, not parsed.
    let create_status = create_resp.status();
    let create_body = create_resp.text().await.unwrap_or_default();
    if !create_status.is_success() {
        return Err(write_status_to_error(
            create_status,
            &create_body,
            "create conversation",
        ));
    }

    // c. POST the minimal completion and drain the SSE.
    // TUNE: minimal claude.ai completion body — adjust against a live capture if 4xx.
    let completion_body = serde_json::json!({
        "prompt": ".",
        "parent_message_uuid": ROOT_PARENT_MESSAGE_UUID,
        "timezone": "UTC",
        "attachments": [],
        "files": [],
        "sync_sources": [],
        "rendering_mode": "messages"
    });
    let completion_url =
        format!("{api_base}/organizations/{org_uuid}/chat_conversations/{conv_uuid}/completion");
    let resp = http
        .post(&completion_url)
        .header("Cookie", &cookie)
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream")
        .header("anthropic-client-platform", "web_claude_ai")
        .header("anthropic-device-id", device_id)
        .header("Origin", "https://claude.ai")
        .header("Referer", "https://claude.ai/new")
        .header("User-Agent", WEB_UA)
        .json(&completion_body)
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))?;

    let status = resp.status();
    // Drain the stream so the turn completes (the body itself carries the result).
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(write_status_to_error(status, &text, "completion"));
    }
    // HTTP 200 is necessary but not sufficient: require a positive assistant
    // frame and no in-band error frame (the turn must actually have run).
    if !web_sse_consumed_window(&text) {
        let snippet: String = crate::util::scrub_sensitive_text(&text)
            .chars()
            .take(200)
            .collect();
        return Err(ProviderError::Status {
            status: status.as_u16(),
            body: format!("claude.ai anchor: no completion frame in stream: {snippet}"),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_pasted_session_key_cookie_values() {
        assert_eq!(
            session_key_cookie_value("sk-ant-sid01-raw"),
            "sk-ant-sid01-raw"
        );
        assert_eq!(
            session_key_cookie_value("sessionKey=sk-ant-sid01-direct"),
            "sk-ant-sid01-direct"
        );
        assert_eq!(
            session_key_cookie_value("Cookie: other=1; sessionKey=sk-ant-sid01-cookie; x=2"),
            "sk-ant-sid01-cookie"
        );
    }

    #[test]
    fn web_orgs_parse_tier_and_email() {
        let v: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/claude_web_orgs.json")).unwrap();
        let orgs: Vec<WebOrg> = serde_json::from_value(v).unwrap();
        let org = orgs.into_iter().next().unwrap();
        assert_eq!(org.uuid, "11111111-2222-3333-4444-555555555555");
        assert_eq!(
            org.rate_limit_tier.as_deref(),
            Some("default_claude_max_20x")
        );
        let email = org
            .memberships
            .iter()
            .find_map(|m| m.account.as_ref())
            .and_then(|a| a.email_address.clone());
        assert_eq!(email.as_deref(), Some("claude-user@example.invalid"));
    }

    // ── WRITE path: claude.ai web-chat anchor (FEAT-2 / BUG-1) ──────────────

    #[test]
    fn full_cookie_header_forwards_cloudflare_else_bare_session_key() {
        // Bare key → canonical sessionKey=<value> (no CF cookies to preserve).
        assert_eq!(
            full_cookie_header("sk-ant-sid01-raw"),
            "sessionKey=sk-ant-sid01-raw"
        );
        assert_eq!(
            full_cookie_header("sessionKey=sk-ant-sid01-direct"),
            "sessionKey=sk-ant-sid01-direct"
        );
        // A full Cookie header carrying cf_clearance / __cf_bm → forward VERBATIM
        // (the whole string, so Cloudflare clearance reaches the write path).
        let full = "Cookie: __cf_bm=abc; sessionKey=sk-ant-sid01-x; cf_clearance=zzz";
        assert_eq!(
            full_cookie_header(full),
            "__cf_bm=abc; sessionKey=sk-ant-sid01-x; cf_clearance=zzz"
        );
        // cf_clearance alone (lowercase `cookie:` prefix) is also preserved whole.
        assert_eq!(
            full_cookie_header("cookie: sessionKey=sk-ant-sid01-y; cf_clearance=q"),
            "sessionKey=sk-ant-sid01-y; cf_clearance=q"
        );
    }

    #[test]
    fn uuid_v4_is_well_formed_and_random() {
        let a = uuid_v4();
        let b = uuid_v4();
        assert_ne!(a, b, "two generated uuids must differ");
        // 8-4-4-4-12 hyphenation, all-hex, version nibble 4, variant in [8,b].
        let parts: Vec<&str> = a.split('-').collect();
        assert_eq!(
            parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
            vec![8, 4, 4, 4, 12]
        );
        assert!(a.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
        assert_eq!(parts[2].chars().next(), Some('4'), "version 4");
        assert!(
            matches!(parts[3].chars().next(), Some('8' | '9' | 'a' | 'b')),
            "variant 10xx, got {}",
            parts[3]
        );
    }

    #[test]
    fn web_sse_scanner_requires_positive_frame_and_rejects_errors() {
        // Positive assistant frames (event: form and data: {"type":...} form).
        assert!(web_sse_consumed_window(
            "event: completion\ndata: {\"completion\":\"x\"}\n\n"
        ));
        assert!(web_sse_consumed_window(
            "event: message_start\ndata: {}\n\nevent: message_delta\ndata: {}\n\n"
        ));
        assert!(web_sse_consumed_window(
            "data: {\"type\":\"completion\",\"completion\":\"hi\"}\n\n"
        ));
        // No assistant frame at all → not consumed.
        assert!(!web_sse_consumed_window("event: ping\ndata: {}\n\n"));
        assert!(!web_sse_consumed_window(""));
        // An explicit error frame poisons the stream even with a success frame.
        assert!(!web_sse_consumed_window(
            "event: message_start\ndata: {}\n\nevent: error\ndata: {\"error\":{\"type\":\"overloaded\"}}\n\n"
        ));
        // A data frame carrying a top-level error object → failed.
        assert!(!web_sse_consumed_window(
            "data: {\"error\":{\"message\":\"blocked\"}}\n\n"
        ));
    }

    /// D2 request-SHAPE gate for the claude.ai web anchor. Drives the full
    /// 3-call sequence against a wiremock base and asserts each call's
    /// auth/path/body/headers, then SSE success vs an `event: error` failure.
    /// This is the new behavior-change test (replaces the old `/v1/messages`
    /// OAuth wiremock test in `anchor.rs`). It proves request SHAPE only — the
    /// live claude.ai behavior is verifiable only with a real session cookie.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn send_claude_web_drives_orgs_conversation_and_completion() {
        use wiremock::matchers::{body_partial_json, header, method, path, path_regex};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let cookie = "sessionKey=sk-ant-sid01-test";

        // a. GET organizations — cookie-authenticated; returns one org uuid.
        Mock::given(method("GET"))
            .and(path("/api/organizations"))
            .and(header("cookie", cookie))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "uuid": "org-uuid-1", "name": "Personal", "memberships": [] }
            ])))
            .mount(&server)
            .await;

        // b. POST chat_conversations — cookie + the device/platform headers; body
        //    carries a client-generated uuid + an (empty) name.
        Mock::given(method("POST"))
            .and(path("/api/organizations/org-uuid-1/chat_conversations"))
            .and(header("cookie", cookie))
            .and(header("anthropic-client-platform", "web_claude_ai"))
            .and(header("anthropic-device-id", "dev-uuid-xyz"))
            .and(body_partial_json(serde_json::json!({ "name": "" })))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "uuid": "conv-uuid-2" })),
            )
            .mount(&server)
            .await;

        // c. POST completion — correct nested path, accept: text/event-stream, the
        //    minimal body, and an SSE success stream (a positive completion frame).
        Mock::given(method("POST"))
            .and(path_regex(
                r"^/api/organizations/org-uuid-1/chat_conversations/[0-9a-f-]+/completion$",
            ))
            .and(header("cookie", cookie))
            .and(header("accept", "text/event-stream"))
            .and(header("anthropic-client-platform", "web_claude_ai"))
            .and(body_partial_json(
                serde_json::json!({ "prompt": ".", "parent_message_uuid": ROOT_PARENT_MESSAGE_UUID }),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                "event: message_start\ndata: {\"type\":\"message_start\"}\n\n\
                 event: completion\ndata: {\"completion\":\".\"}\n\n",
            ))
            .mount(&server)
            .await;

        let client = crate::http::build_client();
        let api_base = format!("{}/api", server.uri());
        send_claude_web(&client, cookie, "dev-uuid-xyz", &api_base)
            .await
            .expect("a 200 SSE with a completion frame is a successful anchor");
    }

    /// Failure half of the gate: a 200 completion whose stream is an
    /// `event: error` frame must NOT read as a successful anchor (no window
    /// consumed). Returns a `Status` error (kept non-transient → cooldown holds).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn send_claude_web_treats_sse_error_frame_as_failure() {
        use wiremock::matchers::{method, path, path_regex};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let cookie = "sessionKey=sk-ant-sid01-test";

        Mock::given(method("GET"))
            .and(path("/api/organizations"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "uuid": "org-uuid-1", "memberships": [] }
            ])))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/organizations/org-uuid-1/chat_conversations"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "uuid": "conv-uuid-2" })),
            )
            .mount(&server)
            .await;
        // HTTP 200 but the stream is an error frame → the turn never ran.
        Mock::given(method("POST"))
            .and(path_regex(
                r"^/api/organizations/org-uuid-1/chat_conversations/[0-9a-f-]+/completion$",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                "event: error\ndata: {\"error\":{\"type\":\"overloaded_error\"}}\n\n",
            ))
            .mount(&server)
            .await;

        let client = crate::http::build_client();
        let api_base = format!("{}/api", server.uri());
        let err = send_claude_web(&client, cookie, "dev-uuid-xyz", &api_base)
            .await
            .expect_err("an SSE error frame must be a failed anchor");
        assert!(
            matches!(err, ProviderError::Status { status: 200, .. }),
            "got: {err:?}"
        );
    }

    /// A 403 on the completion (Cloudflare bot-protection on the write path) is a
    /// `Status` error whose detail tells the user to refresh their Claude cookie.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn send_claude_web_403_hints_to_refresh_cookie() {
        use wiremock::matchers::{method, path, path_regex};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/organizations"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "uuid": "org-uuid-1", "memberships": [] }
            ])))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/organizations/org-uuid-1/chat_conversations"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "uuid": "conv-uuid-2" })),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path_regex(
                r"^/api/organizations/org-uuid-1/chat_conversations/[0-9a-f-]+/completion$",
            ))
            .respond_with(ResponseTemplate::new(403).set_body_string("Just a moment... Cloudflare"))
            .mount(&server)
            .await;

        let client = crate::http::build_client();
        let api_base = format!("{}/api", server.uri());
        let err = send_claude_web(&client, "sk-ant-sid01-test", "dev", &api_base)
            .await
            .expect_err("403 on the write path is a failure");
        match err {
            ProviderError::Status { status, body } => {
                assert_eq!(status, 403);
                assert!(
                    body.contains("refresh your Claude cookie"),
                    "403 detail must hint a cookie refresh: {body}"
                );
            }
            other => panic!("expected Status 403, got {other:?}"),
        }
    }

    /// The create-conversation POST is the FIRST write — a 403 there (Cloudflare)
    /// must ALSO produce the "refresh your Claude cookie" hint, NOT the generic
    /// `send_for_json` "access denied" message (else the user is told their key
    /// is bad when they actually need a full cookie with cf_clearance).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn send_claude_web_create_conversation_403_hints_to_refresh_cookie() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/organizations"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "uuid": "org-uuid-1", "memberships": [] }
            ])))
            .mount(&server)
            .await;
        // 403 (Cloudflare HTML) on the create call — the first write.
        Mock::given(method("POST"))
            .and(path("/api/organizations/org-uuid-1/chat_conversations"))
            .respond_with(
                ResponseTemplate::new(403)
                    .set_body_string("<!DOCTYPE html>Just a moment... Cloudflare"),
            )
            .mount(&server)
            .await;

        let client = crate::http::build_client();
        let api_base = format!("{}/api", server.uri());
        let err = send_claude_web(&client, "sk-ant-sid01-test", "dev", &api_base)
            .await
            .expect_err("403 on the create-conversation write is a failure");
        match err {
            ProviderError::Status { status, body } => {
                assert_eq!(status, 403);
                assert!(
                    body.contains("refresh your Claude cookie"),
                    "create-conversation 403 must hint a cookie refresh, not 'access denied': {body}"
                );
            }
            other => panic!("expected Status 403, got {other:?}"),
        }
    }
}
