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

impl Provider {
    pub fn display(&self) -> &'static str {
        match self {
            Provider::Claude => "Claude",
            Provider::Codex => "Codex",
            Provider::Gemini => "Gemini",
            Provider::Copilot => "GitHub Copilot",
            Provider::Cursor => "Cursor",
            Provider::Zai => "z.ai Coding Plan",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitWindow {
    pub label: String,
    pub used_percent: Option<f32>,
    pub resets_at: Option<i64>, // epoch seconds
    pub used: Option<f64>,
    pub limit: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceUsage {
    pub provider: Provider,
    pub connected: bool,
    pub plan: Option<String>,
    pub account: Option<String>,
    pub error: Option<String>,
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

impl UsageSnapshot {
    /// Highest used_percent across every window of every connected service.
    /// Used to label the tray icon.
    pub fn max_used_percent(&self) -> Option<f32> {
        self.services
            .iter()
            .filter(|s| s.connected)
            .flat_map(|s| s.windows.iter().chain(s.detail_windows.iter()))
            .filter_map(|w| w.used_percent)
            .fold(None, |acc, v| match acc {
                None => Some(v),
                Some(cur) => Some(cur.max(v)),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(label: &str, pct: Option<f32>) -> LimitWindow {
        LimitWindow {
            label: label.into(),
            used_percent: pct,
            resets_at: None,
            used: None,
            limit: None,
        }
    }

    #[test]
    fn snapshot_max_percent_picks_highest_connected() {
        let snap = UsageSnapshot {
            fetched_at: 0,
            services: vec![
                ServiceUsage {
                    provider: Provider::Claude,
                    connected: true,
                    plan: None,
                    account: None,
                    error: None,
                    windows: vec![win("5h", Some(40.0))],
                    detail_windows: vec![],
                    raw_response: None,
                },
                ServiceUsage {
                    provider: Provider::Gemini,
                    connected: true,
                    plan: None,
                    account: None,
                    error: None,
                    windows: vec![win("pro", Some(72.5))],
                    detail_windows: vec![],
                    raw_response: None,
                },
                ServiceUsage {
                    provider: Provider::Codex,
                    connected: false,
                    plan: None,
                    account: None,
                    error: Some("offline".into()),
                    windows: vec![],
                    detail_windows: vec![],
                    raw_response: None,
                },
            ],
        };
        assert_eq!(snap.max_used_percent(), Some(72.5));
    }

    #[test]
    fn provider_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&Provider::Claude).unwrap(), "\"claude\"");
        assert_eq!(serde_json::to_string(&Provider::Copilot).unwrap(), "\"copilot\"");
    }

    #[test]
    fn max_percent_none_when_all_offline() {
        let snap = UsageSnapshot {
            fetched_at: 0,
            services: vec![ServiceUsage {
                provider: Provider::Cursor,
                connected: false,
                plan: None,
                account: None,
                error: Some("x".into()),
                windows: vec![],
                detail_windows: vec![],
                raw_response: None,
            }],
        };
        assert_eq!(snap.max_used_percent(), None);
    }
}
