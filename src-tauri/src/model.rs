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
    /// Pretty-printed raw API response JSON for the "Raw Response" tab in the
    /// detail modal. None if the provider didn't make an HTTP call (e.g.,
    /// file-based credential reads that failed before reaching the API) or
    /// the response couldn't be serialized back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
        let u = ServiceUsage {
            id: "auto:claude".into(),
            source: ServiceSource::Auto,
            provider: Provider::Claude,
            connected: true,
            plan: Some("Max".into()),
            account: Some("a@b.c".into()),
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
                "raw_response",
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

    /// `raw_response` is `skip_serializing_if = "Option::is_none"`, so it must be
    /// absent (not null) when there was no HTTP response.
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
