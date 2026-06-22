//! Cursor (experimental) — token-parsing only. Reads the sign-in JWT Cursor
//! stores as a raw string at `ItemTable[cursorAuth/accessToken]` in its
//! `state.vscdb` SQLite DB, then POSTs to the Connect-RPC dashboard endpoint
//! (mirrors the ClearMeasureLabs/cursor-usage-status VS Code extension). Money
//! in the response is USD cents. No token refresh: Cursor JWTs have no public
//! refresh path, so `refresh_stored` always returns None.

mod creds;
mod parse;

use async_trait::async_trait;
use serde_json::Value;

use crate::http;
use crate::model::{auto_service_id, Provider, ServiceSource, ServiceUsage};
use crate::providers::ProviderError;

use creds::read_cursor_token;
use parse::{normalize, CursorUsage};

const USAGE_URL: &str = "https://api2.cursor.sh/aiserver.v1.DashboardService/GetCurrentPeriodUsage";

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
            http: http::shared(),
        }
    }
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
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("Connect-Protocol-Version", "1")
            .body("{}")
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;
        let val: Value = http::send_for_json(resp, USAGE_URL).await?;
        let raw_json = serde_json::to_string_pretty(&val).ok();
        let u: CursorUsage =
            serde_json::from_value(val).map_err(|e| ProviderError::Parse(e.to_string()))?;
        Ok(ServiceUsage {
            id: auto_service_id(Provider::Cursor),
            source: ServiceSource::Auto,
            provider: Provider::Cursor,
            connected: true,
            plan: None,
            account: None,
            error: None,
            windows: normalize(&u),
            detail_windows: vec![],
            raw_response: raw_json,
        })
    }
}

/// Cursor JWTs have no public refresh path (the sign-in token is reissued only
/// by signing in through the Cursor app itself), so refresh is a no-op. Always
/// returns None — the caller falls back to the existing stored token.
#[allow(clippy::unused_async)]
pub(crate) async fn refresh_stored(
    _: &reqwest::Client,
    _: &crate::store::StoredCredential,
) -> Option<crate::store::StoredCredential> {
    None
}
