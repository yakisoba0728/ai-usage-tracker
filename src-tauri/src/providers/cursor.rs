//! Cursor (experimental). Reads the sign-in token Cursor stores in its
//! `state.vscdb` SQLite DB and POSTs to the Connect-RPC usage endpoint.
//! This endpoint is undocumented and may be WAF-protected; we use legitimate
//! headers only and degrade honestly if blocked. Marked "unstable" in the UI.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::http;
use crate::model::{LimitWindow, Provider, ServiceUsage};
use crate::providers::ProviderError;
use crate::secrets;

const USAGE_URL: &str =
    "https://api2.cursor.sh/aiserver.v1.DashboardService/GetCurrentPeriodUsage";

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CursorUsage {
    #[serde(default)] plan_usage: Option<f64>,
    #[serde(default)] max_plan_usage: Option<f64>,
}

pub struct CursorProvider {
    http: reqwest::Client,
}

impl CursorProvider {
    pub fn new() -> Self {
        Self {
            http: http::build_client(),
        }
    }
}

/// Read the Cursor auth token from the globalStorage SQLite DB. VS Code-family
/// stores global state in an `ItemTable(key, value)`; Cursor keeps its auth
/// token under a cursorAuth-ish key. We search plausible keys defensively.
fn read_cursor_token() -> Result<String, ProviderError> {
    let db = secrets::cursor_state_db().ok_or_else(|| {
        ProviderError::NotLoggedIn("Cursor not installed / no state.vscdb".into())
    })?;
    let conn = rusqlite::Connection::open_with_flags(
        &db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| ProviderError::NotLoggedIn(format!("open state.vscdb: {e}")))?;

    let mut stmt = conn
        .prepare("SELECT key, value FROM ItemTable WHERE key LIKE '%cursorAuth%' OR key LIKE '%authToken%' OR key LIKE '%accessToken%'")
        .map_err(|e| ProviderError::Parse(format!("query state.vscdb: {e}")))?;
    let rows: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| ProviderError::Parse(format!("read state.vscdb: {e}")))?
        .filter_map(|r| r.ok())
        .collect();

    for (_k, v) in &rows {
        // Cursor stores the token either as a bare string or a JSON object.
        if v.starts_with('{') {
            if let Ok(obj) = serde_json::from_str::<Value>(v) {
                for field in ["accessToken", "token", "value"] {
                    if let Some(s) = obj.get(field).and_then(|x| x.as_str()) {
                        if !s.is_empty() {
                            return Ok(s.to_string());
                        }
                    }
                }
            }
        } else if !v.is_empty() && v.contains('.') {
            // looks like a JWT-ish bearer token
            return Ok(v.clone());
        }
    }
    Err(ProviderError::NotLoggedIn(
        "Cursor token not found in state.vscdb (sign in to Cursor)".into(),
    ))
}

/// Pure: plan usage → window. plan_usage/max_plan_usage give a percentage when
/// both are present.
pub fn normalize(u: &CursorUsage) -> Vec<LimitWindow> {
    let used_percent = match (u.plan_usage, u.max_plan_usage) {
        (Some(used), Some(max)) if max > 0.0 => Some((used / max * 100.0) as f32),
        _ => None,
    };
    if u.plan_usage.is_none() && u.max_plan_usage.is_none() {
        return vec![];
    }
    vec![LimitWindow {
        label: "Plan usage".into(),
        used_percent,
        resets_at: None,
        used: u.plan_usage,
        limit: u.max_plan_usage,
    }]
}

#[async_trait]
impl crate::providers::ProviderApi for CursorProvider {
    fn key(&self) -> Provider {
        Provider::Cursor
    }

    async fn fetch(&self) -> Result<ServiceUsage, ProviderError> {
        let token = read_cursor_token()?;
        // Connect-RPC unary: method in URL, body is the JSON request, empty here.
        let resp = self
            .http
            .post(USAGE_URL)
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .header("Connect-Protocol-Version", "1")
            .body("{}")
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;
        let val: Value = http::send_for_json(resp, USAGE_URL).await.map_err(|e| {
            ProviderError::Network(format!("Cursor usage endpoint blocked/unavailable: {e}"))
        })?;
        let u: CursorUsage =
            serde_json::from_value(val).map_err(|e| ProviderError::Parse(e.to_string()))?;
        Ok(ServiceUsage {
            provider: Provider::Cursor,
            connected: true,
            plan: None,
            account: None,
            error: Some("experimental — values may be unstable".into()),
            windows: normalize(&u),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_fixture_percent() {
        let u: CursorUsage =
            serde_json::from_str(include_str!("../../tests/cursor_usage_fixture.json")).unwrap();
        let w = &normalize(&u)[0];
        assert_eq!(w.used_percent, Some(25.0));
        assert_eq!(w.limit, Some(5000.0));
        assert_eq!(w.used, Some(1250.0));
    }

    #[test]
    fn normalize_empty_when_no_data() {
        assert!(normalize(&CursorUsage::default()).is_empty());
    }
}
