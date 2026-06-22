//! Unified usage model shared by every provider and the frontend (via IPC).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Claude,
    Codex,
    Gemini,
    Copilot,
    Cursor,
    Zai,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ServiceSource {
    #[default]
    Auto,
    Stored,
}

pub fn auto_service_id(provider: Provider) -> String {
    let key = match provider {
        Provider::Claude => "claude",
        Provider::Codex => "codex",
        Provider::Gemini => "gemini",
        Provider::Copilot => "copilot",
        Provider::Cursor => "cursor",
        Provider::Zai => "zai",
    };
    format!("auto:{key}")
}

pub fn stored_service_id(account_id: &str) -> String {
    format!("stored:{account_id}")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitWindow {
    pub label: String,
    pub used_percent: Option<f32>,
    pub resets_at: Option<i64>, // epoch seconds
    pub used: Option<f64>,
    pub limit: Option<f64>,
}

/// Structured, localizable error attached to a disconnected/failed service.
/// `code` is a stable machine key the frontend maps to a localized message
/// (`error.<code>`); `detail` is the English technical string (HTTP body,
/// network message) shown as a fallback when no key matches that code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceError {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl ServiceError {
    /// Code-only error (no technical detail). Test-only: production errors are
    /// minted solely via `From<ProviderError>`, which keeps `code` constrained
    /// to the known `ProviderError` taxonomy.
    #[cfg(test)]
    pub fn code(code: &str) -> Self {
        Self {
            code: code.to_string(),
            detail: None,
        }
    }
}

fn raw_response_should_skip(value: &Option<String>) -> bool {
    value.is_none() || !raw_response_debug_enabled()
}

fn serialize_raw_response<S>(value: &Option<String>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match value {
        Some(raw) => serializer.serialize_some(&redact_raw_response(raw)),
        None => serializer.serialize_none(),
    }
}

fn raw_response_debug_enabled() -> bool {
    std::env::var("AIT_DEBUG_RAW_RESPONSE")
        .map(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn redact_raw_response(raw: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(mut value) => {
            redact_json_value(&mut value);
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| "[redacted]".to_string())
        }
        Err(_) => crate::util::scrub_sensitive_text(raw)
            .chars()
            .take(200)
            .collect(),
    }
}

fn redact_json_value(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map.iter_mut() {
                if is_sensitive_json_key(key) {
                    *value = serde_json::Value::String("[redacted]".into());
                } else {
                    redact_json_value(value);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                redact_json_value(item);
            }
        }
        serde_json::Value::String(text) => {
            let scrubbed = crate::util::scrub_sensitive_text(text);
            if scrubbed != *text {
                *text = scrubbed;
            }
        }
        _ => {}
    }
}

fn is_sensitive_json_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|ch| *ch != '-' && *ch != '_')
        .flat_map(char::to_lowercase)
        .collect::<String>();
    normalized.contains("token")
        || normalized.contains("sessionkey")
        || normalized == "email"
        || normalized == "emailaddress"
        || normalized == "userid"
        || normalized == "accountid"
        || normalized == "cookie"
        || normalized == "authorization"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceUsage {
    /// Stable UI key for this reading. Auto-detected sessions use
    /// `auto:<provider>`; stored/manual credentials use `stored:<account-id>`.
    pub id: String,
    #[serde(default)]
    pub source: ServiceSource,
    pub provider: Provider,
    pub connected: bool,
    pub plan: Option<String>,
    pub account: Option<String>,
    pub error: Option<ServiceError>,
    pub windows: Vec<LimitWindow>,
    /// Modal-only windows (Spark / per-model / credits / resets / extra usage).
    /// Hidden on the card, shown when the card is opened.
    pub detail_windows: Vec<LimitWindow>,
    /// Pretty-printed raw API response JSON. It is omitted from IPC unless
    /// AIT_DEBUG_RAW_RESPONSE is explicitly enabled, then redacted during
    /// serialization before it can reach the webview.
    #[serde(
        default,
        skip_serializing_if = "raw_response_should_skip",
        serialize_with = "serialize_raw_response"
    )]
    pub raw_response: Option<String>,
}

/// Display-only projection of a stored credential — what `list_accounts`
/// returns to the frontend. Never carries the access/refresh tokens (P0 #1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountInfo {
    pub id: String,
    pub provider: Provider,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSnapshot {
    pub fetched_at: i64, // epoch seconds
    pub services: Vec<ServiceUsage>,
}

#[cfg(test)]
mod tests {
    use super::*;

    static RAW_RESPONSE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn provider_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&Provider::Claude).unwrap(),
            "\"claude\""
        );
        assert_eq!(
            serde_json::to_string(&Provider::Copilot).unwrap(),
            "\"copilot\""
        );
    }

    /// Guards the IPC contract: the serialized field set must match
    /// `src/lib/types.ts` (`ServiceUsage` / `LimitWindow`). A Rust-side rename or
    /// serde-attr change that diverges from the frontend types fails here.
    #[test]
    fn service_usage_json_shape_matches_ts_contract() {
        let _guard = RAW_RESPONSE_ENV_LOCK.lock().unwrap();
        std::env::remove_var("AIT_DEBUG_RAW_RESPONSE");

        let u = ServiceUsage {
            id: "auto:claude".into(),
            source: ServiceSource::Auto,
            provider: Provider::Claude,
            connected: true,
            plan: Some("Max".into()),
            account: Some("person@example.invalid".into()),
            error: None,
            windows: vec![LimitWindow {
                label: "5-hour".into(),
                used_percent: Some(92.0),
                resets_at: Some(123),
                used: Some(184.0),
                limit: Some(200.0),
            }],
            detail_windows: vec![],
            raw_response: Some("{}".into()),
        };
        let v = serde_json::to_value(&u).unwrap();
        let obj = v.as_object().unwrap();
        let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "account",
                "connected",
                "detail_windows",
                "error",
                "id",
                "plan",
                "provider",
                "source",
                "windows",
            ]
        );
        assert_eq!(obj["source"], "auto");
        assert_eq!(obj["provider"], "claude");

        let mut wkeys: Vec<&str> = obj["windows"][0]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        wkeys.sort_unstable();
        assert_eq!(
            wkeys,
            vec!["label", "limit", "resets_at", "used", "used_percent"]
        );
    }

    /// Guards the nested `ServiceError` IPC contract (mirrors TS `ServiceError`).
    /// `code` is always serialized; `detail` is `skip_serializing_if = None`, so it
    /// must be ABSENT (not null) when there's no detail — a Rust-side rename of
    /// either field, or dropping the skip attr, fails here.
    #[test]
    fn service_error_json_shape_matches_ts_contract() {
        let with_detail = serde_json::to_value(ServiceError {
            code: "server_error".into(),
            detail: Some("unexpected response (429): rate limited".into()),
        })
        .unwrap();
        let mut keys: Vec<&str> = with_detail
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["code", "detail"]);
        assert_eq!(with_detail["code"], "server_error");

        // detail = None → the key is omitted entirely (skip_serializing_if).
        let no_detail = serde_json::to_value(ServiceError::code("offline")).unwrap();
        let keys: Vec<&str> = no_detail
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, vec!["code"]);
    }

    /// `raw_response` is omitted from IPC by default even when a provider
    /// captured one; it can contain account identifiers or provider-specific
    /// metadata that the webview does not need during normal operation.
    #[test]
    fn raw_response_is_omitted_by_default_even_when_present() {
        let _guard = RAW_RESPONSE_ENV_LOCK.lock().unwrap();
        std::env::remove_var("AIT_DEBUG_RAW_RESPONSE");

        let u = ServiceUsage {
            id: "auto:claude".into(),
            source: ServiceSource::Auto,
            provider: Provider::Claude,
            connected: true,
            plan: Some("Max".into()),
            account: Some("person@example.invalid".into()),
            error: None,
            windows: vec![],
            detail_windows: vec![],
            raw_response: Some(
                r#"{"email":"person@example.invalid","access_token":"sk-ant-secret"}"#.into(),
            ),
        };
        let v = serde_json::to_value(&u).unwrap();
        assert!(!v.as_object().unwrap().contains_key("raw_response"));
    }

    /// Debug raw responses are opt-in and scrubbed before serialization.
    #[test]
    fn raw_response_debug_opt_in_redacts_sensitive_values() {
        let _guard = RAW_RESPONSE_ENV_LOCK.lock().unwrap();
        std::env::set_var("AIT_DEBUG_RAW_RESPONSE", "1");

        let u = ServiceUsage {
            id: "auto:claude".into(),
            source: ServiceSource::Auto,
            provider: Provider::Claude,
            connected: true,
            plan: Some("Max".into()),
            account: Some("person@example.invalid".into()),
            error: None,
            windows: vec![],
            detail_windows: vec![],
            raw_response: Some(
                r#"{"email":"person@example.invalid","nested":{"sessionKey":"sk-ant-session","refresh_token":"rt-secret"},"ok":true}"#.into(),
            ),
        };
        let v = serde_json::to_value(&u).unwrap();
        let raw = v
            .as_object()
            .unwrap()
            .get("raw_response")
            .and_then(|v| v.as_str())
            .expect("debug opt-in should serialize a scrubbed raw response");
        assert!(
            raw.contains("\"ok\": true"),
            "non-sensitive fields stay visible: {raw}"
        );
        assert!(
            !raw.contains("person@example.invalid"),
            "email leaked: {raw}"
        );
        assert!(!raw.contains("sk-ant-session"), "session key leaked: {raw}");
        assert!(!raw.contains("rt-secret"), "refresh token leaked: {raw}");

        std::env::remove_var("AIT_DEBUG_RAW_RESPONSE");
    }

    /// `raw_response` is absent (not null) when there was no HTTP response.
    #[test]
    fn raw_response_is_omitted_when_none() {
        let u = ServiceUsage {
            id: "auto:cursor".into(),
            source: ServiceSource::Auto,
            provider: Provider::Cursor,
            connected: false,
            plan: None,
            account: None,
            error: Some(ServiceError::code("offline")),
            windows: vec![],
            detail_windows: vec![],
            raw_response: None,
        };
        let v = serde_json::to_value(&u).unwrap();
        assert!(!v.as_object().unwrap().contains_key("raw_response"));
    }
}
