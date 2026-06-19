//! Cursor (experimental) — token-parsing only. Reads the sign-in JWT Cursor
//! stores as a raw string at `ItemTable[cursorAuth/accessToken]` in its
//! `state.vscdb` SQLite DB, then POSTs to the Connect-RPC dashboard endpoint
//! (exactly like the ClearMeasureLabs/cursor-usage-status extension). Money in
//! the response is USD cents.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::http;
use crate::model::{LimitWindow, Provider, ServiceUsage};
use crate::providers::ProviderError;
use crate::secrets;

const USAGE_URL: &str =
    "https://api2.cursor.sh/aiserver.v1.DashboardService/GetCurrentPeriodUsage";
const ACCESS_KEY: &str = "cursorAuth/accessToken";
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CursorUsage {
    #[serde(default)] enabled: Option<bool>,
    #[serde(default)] plan_usage: Option<PlanUsage>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PlanUsage {
    #[serde(default)] included_spend: Option<f64>,
    #[serde(default)] total_spend: Option<f64>,
    #[serde(default)] remaining: Option<f64>,
    #[serde(default)] limit: Option<f64>,
    #[serde(default)] total_percent_used: Option<f64>,
}

pub struct CursorProvider {
    http: reqwest::Client,
}
impl Default for CursorProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CursorProvider {
    pub fn new() -> Self {
        Self {
            http: http::build_client(),
        }
    }
}

/// Read the Cursor access token from `state.vscdb` (raw string value).
fn read_cursor_token() -> Result<String, ProviderError> {
    let db = secrets::cursor_state_db()
        .ok_or_else(|| ProviderError::NotLoggedIn("Cursor not installed / no state.vscdb".into()))?;
    let conn = rusqlite::Connection::open_with_flags(
        &db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| ProviderError::NotLoggedIn(format!("open state.vscdb: {e}")))?;
    let mut stmt = conn
        .prepare("SELECT value FROM ItemTable WHERE key = ? LIMIT 1")
        .map_err(|e| ProviderError::Parse(format!("query state.vscdb: {e}")))?;
    let token: Option<String> = stmt
        .query_row([ACCESS_KEY], |r| r.get::<_, String>(0))
        .ok();
    token.filter(|t| !t.is_empty()).ok_or_else(|| {
        ProviderError::NotLoggedIn(
            "Cursor token not found in state.vscdb (sign in to Cursor)".into(),
        )
    })
}

/// Pure: planUsage → window. Money is cents → dollars.
fn normalize(u: &CursorUsage) -> Vec<LimitWindow> {
    if u.enabled == Some(false) {
        return vec![];
    }
    let Some(p) = &u.plan_usage else {
        return vec![];
    };
    let used_cents = p
        .included_spend
        .or(p.total_spend)
        .or(match (p.limit, p.remaining) {
            (Some(l), Some(r)) => Some(l - r),
            _ => None,
        });
    let limit_cents = p.limit;
    let used_percent = p.total_percent_used.or_else(|| match (used_cents, limit_cents) {
        (Some(u), Some(l)) if l > 0.0 => Some(u / l * 100.0),
        _ => None,
    });
    if used_cents.is_none() && limit_cents.is_none() && used_percent.is_none() {
        return vec![];
    }
    vec![LimitWindow {
        label: "Plan usage".into(),
        used_percent: used_percent.map(|x| x as f32),
        resets_at: None,
        used: used_cents.map(|c| c / 100.0),
        limit: limit_cents.map(|c| c / 100.0),
    }]
}

#[async_trait]
impl crate::providers::ProviderApi for CursorProvider {
    fn key(&self) -> Provider {
        Provider::Cursor
    }

    async fn fetch(&self) -> Result<ServiceUsage, ProviderError> {
        let token = read_cursor_token()?;
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
        let val: Value = http::send_for_json(resp, USAGE_URL).await?;
        let u: CursorUsage =
            serde_json::from_value(val).map_err(|e| ProviderError::Parse(e.to_string()))?;
        Ok(ServiceUsage {
            provider: Provider::Cursor,
            connected: true,
            plan: None,
            account: None,
            error: None,
            windows: normalize(&u),
            detail_windows: vec![],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_fixture_cents_to_dollars() {
        let u: CursorUsage =
            serde_json::from_str(include_str!("../../tests/cursor_usage_fixture.json")).unwrap();
        let w = &normalize(&u)[0];
        assert_eq!(w.used_percent, Some(15.48));
        assert_eq!(w.used, Some(232.22)); // 23222 cents -> $232.22
        assert_eq!(w.limit, Some(400.0)); // 40000 cents -> $400.00
    }

    #[test]
    fn normalize_empty_when_no_plan_usage() {
        assert!(normalize(&CursorUsage::default()).is_empty());
        assert!(normalize(&CursorUsage { enabled: Some(false), plan_usage: None }).is_empty());
    }
}
