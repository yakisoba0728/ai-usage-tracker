//! Gemini (via Gemini CLI) — ported from gemini-cli-usage (MIT).
//! Reads `~/.gemini/oauth_creds.json`, self-refreshes the access token via
//! Google's OAuth endpoint (client metadata from env > creds file), then calls
//! the internal Code Assist API (`loadCodeAssist` + `retrieveUserQuota`).

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http;
use crate::model::{LimitWindow, Provider, ServiceUsage};
use crate::providers::ProviderError;
use crate::secrets;

const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const CODE_ASSIST_BASE: &str = "https://cloudcode-pa.googleapis.com/v1internal";

#[derive(Deserialize, Clone)]
struct OauthCreds {
    access_token: String,
    refresh_token: String,
    #[serde(default)] expiry_date: i64,
    #[serde(default)] client_id: Option<String>,
    #[serde(default)] client_secret: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Bucket {
    #[serde(default)] model_id: Option<String>,
    #[serde(default)] remaining_fraction: Option<f64>,
    #[serde(default)] remaining_amount: Option<String>,
    #[serde(default)] reset_time: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Deserialize, Default)]
struct QuotaResp {
    #[serde(default)] buckets: Vec<Bucket>,
}

pub struct GeminiProvider {
    http: reqwest::Client,
}

impl GeminiProvider {
    pub fn new() -> Self {
        Self {
            http: http::build_client(),
        }
    }
}

/// Resolve client metadata: env > creds file. We never register our own client.
fn client_metadata(creds: &OauthCreds) -> Result<(String, String), ProviderError> {
    if let (Some(id), Some(sec)) = (creds.client_id.clone(), creds.client_secret.clone()) {
        if !id.is_empty() && !sec.is_empty() {
            return Ok((id, sec));
        }
    }
    if let (Ok(id), Ok(sec)) = (
        std::env::var("GEMINI_OAUTH_CLIENT_ID"),
        std::env::var("GEMINI_OAUTH_CLIENT_SECRET"),
    ) {
        return Ok((id, sec));
    }
    Err(ProviderError::NotLoggedIn(
        "Gemini client metadata not found (set GEMINI_OAUTH_CLIENT_ID/SECRET or run `gemini`)".into(),
    ))
}

/// Build the refresh form (unit-testable, no network).
pub fn refresh_form(creds: &OauthCreds) -> Result<Vec<(String, String)>, ProviderError> {
    let (id, sec) = client_metadata(creds)?;
    Ok(vec![
        ("grant_type".into(), "refresh_token".into()),
        ("refresh_token".into(), creds.refresh_token.clone()),
        ("client_id".into(), id),
        ("client_secret".into(), sec),
    ])
}

/// Pure: buckets → LimitWindows.
pub fn normalize(resp: &QuotaResp) -> Vec<LimitWindow> {
    resp.buckets
        .iter()
        .filter_map(|b| {
            let label = b.model_id.clone().unwrap_or_else(|| "Gemini".into());
            let used_percent = b.remaining_fraction.map(|f| ((1.0 - f) * 100.0) as f32);
            let limit = match (&b.remaining_amount, b.remaining_fraction) {
                (Some(amt), Some(f)) if f > 0.0 => amt.parse::<f64>().ok().map(|r| r / f),
                _ => None,
            };
            Some(LimitWindow {
                label,
                used_percent,
                resets_at: b.reset_time.map(|d| d.timestamp()),
                used: None,
                limit,
            })
        })
        .collect()
}

impl GeminiProvider {
    async fn fresh_token(&self) -> Result<String, ProviderError> {
        let v = secrets::read_json_file(&secrets::gemini_creds_path())?;
        let creds: OauthCreds =
            serde_json::from_value(v).map_err(|e| ProviderError::Parse(format!("gemini creds: {e}")))?;
        let now_ms = chrono::Utc::now().timestamp_millis();
        if creds.expiry_date > 0 && now_ms < creds.expiry_date - 60_000 {
            return Ok(creds.access_token);
        }
        let form = refresh_form(&creds)?;
        let resp = self
            .http
            .post(TOKEN_URL)
            .form(&form)
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;
        let val: Value = http::send_for_json(resp, TOKEN_URL).await?;
        val["access_token"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| ProviderError::Parse("no access_token in refresh response".into()))
    }

    async fn post_internal(
        &self,
        token: &str,
        method: &str,
        payload: Value,
    ) -> Result<Value, ProviderError> {
        let url = format!("{CODE_ASSIST_BASE}:{method}");
        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;
        http::send_for_json(resp, &url).await
    }
}

#[async_trait]
impl crate::providers::ProviderApi for GeminiProvider {
    fn key(&self) -> Provider {
        Provider::Gemini
    }

    async fn fetch(&self) -> Result<ServiceUsage, ProviderError> {
        let token = self.fresh_token().await?;
        let load = self
            .post_internal(
                &token,
                "loadCodeAssist",
                json!({
                    "metadata": {
                        "ideType": "IDE_UNSPECIFIED",
                        "platform": "PLATFORM_UNSPECIFIED",
                        "pluginType": "GEMINI"
                    }
                }),
            )
            .await?;
        let project = load
            .get("cloudaicompanionProject")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| ProviderError::Parse("no cloudaicompanionProject".into()))?;
        let quota_val = self
            .post_internal(&token, "retrieveUserQuota", json!({ "project": project }))
            .await?;
        let quota: QuotaResp =
            serde_json::from_value(quota_val).map_err(|e| ProviderError::Parse(e.to_string()))?;
        let tier = load
            .get("paidTier")
            .and_then(|t| t.get("name"))
            .and_then(|n| n.as_str())
            .map(String::from);
        Ok(ServiceUsage {
            provider: Provider::Gemini,
            connected: true,
            plan: tier,
            account: None,
            error: None,
            windows: normalize(&quota),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_quota_fixture() {
        let q: QuotaResp =
            serde_json::from_str(include_str!("../../tests/gemini_quota_fixture.json")).unwrap();
        let ws = normalize(&q);
        assert_eq!(ws.len(), 2);
        let pro = ws.iter().find(|w| w.label == "gemini-2.5-pro").unwrap();
        assert_eq!(pro.used_percent, Some(35.0));
        assert_eq!(pro.limit, Some(1000.0)); // 650 / 0.65
        let flash = ws
            .iter()
            .find(|w| w.label == "gemini-2.5-flash-lite")
            .unwrap();
        assert!(flash.limit.is_none()); // no remainingAmount
    }

    #[test]
    fn refresh_form_uses_creds_client_metadata() {
        let c: OauthCreds =
            serde_json::from_str(include_str!("../../tests/gemini_oauth_fixture.json")).unwrap();
        let form = refresh_form(&c).unwrap();
        let map: std::collections::HashMap<String, String> = form.into_iter().collect();
        assert_eq!(map.get("client_id").unwrap(), "cid");
        assert_eq!(map.get("grant_type").unwrap(), "refresh_token");
        assert_eq!(map.get("client_secret").unwrap(), "csec");
    }
}
