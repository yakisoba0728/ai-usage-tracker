//! Gemini (via Gemini CLI) — token-parsing only.
//! Reads the OAuth token Gemini CLI stores at `~/.gemini/oauth_creds.json`,
//! checks expiry, and calls the internal Code Assist API (`loadCodeAssist` +
//! `retrieveUserQuota`). No OAuth of our own: the token is rotated by the
//! Gemini CLI; on expiry we surface a hint to run `gemini`.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http;
use crate::model::{LimitWindow, Provider, ServiceUsage};
use crate::providers::ProviderError;
use crate::secrets;

const CODE_ASSIST_BASE: &str = "https://cloudcode-pa.googleapis.com/v1internal";

#[derive(Deserialize)]
struct OauthCreds {
    access_token: String,
    #[serde(default)] expiry_date: i64,
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

/// Pure: buckets → LimitWindows.
fn normalize(resp: &QuotaResp) -> (Vec<LimitWindow>, Vec<LimitWindow>) {
    let all: Vec<LimitWindow> = resp
        .buckets
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
        .collect();
    if all.is_empty() {
        return (vec![], vec![]);
    }
    // Card headline = most-consumed bucket; every other bucket → modal.
    let mut idx = 0;
    let mut best = f32::MIN;
    for (i, w) in all.iter().enumerate() {
        let p = w.used_percent.unwrap_or(-1.0);
        if p > best {
            best = p;
            idx = i;
        }
    }
    let headline = all[idx].clone();
    let detail: Vec<LimitWindow> = all
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != idx)
        .map(|(_, w)| w.clone())
        .collect();
    (vec![headline], detail)
}

#[async_trait]
impl crate::providers::ProviderApi for GeminiProvider {
    fn key(&self) -> Provider {
        Provider::Gemini
    }

    async fn fetch(&self) -> Result<ServiceUsage, ProviderError> {
        let v = secrets::read_json_file(&secrets::gemini_creds_path())?;
        let creds: OauthCreds =
            serde_json::from_value(v).map_err(|e| ProviderError::Parse(format!("gemini creds: {e}")))?;
        let now_ms = chrono::Utc::now().timestamp_millis();
        if creds.expiry_date > 0 && creds.expiry_date < now_ms {
            return Err(ProviderError::Expired(
                "Gemini CLI token expired — run `gemini` once to refresh".into(),
            ));
        }
        let token = creds.access_token;

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
        let (windows, detail_windows) = normalize(&quota);
        Ok(ServiceUsage {
            provider: Provider::Gemini,
            connected: true,
            plan: tier,
            account: None,
            error: None,
            windows,
            detail_windows,
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
        let (ws, detail) = normalize(&q);
        // headline = most-consumed bucket (pro 35% > flash-lite ~0.07%)
        assert_eq!(ws.len(), 1);
        let pro = ws.iter().find(|w| w.label == "gemini-2.5-pro").unwrap();
        assert_eq!(pro.used_percent, Some(35.0));
        assert_eq!(pro.limit, Some(1000.0)); // 650 / 0.65
        let flash = detail.iter().find(|w| w.label == "gemini-2.5-flash-lite").unwrap();
        assert!(flash.limit.is_none());
    }
}
