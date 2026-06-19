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
const GEMINI_CLIENT_ID: &str = "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com";
const GEMINI_CLIENT_SECRET: &str = "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";
const GEMINI_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

#[derive(Deserialize)]
#[allow(dead_code)]
struct OauthCreds {
    access_token: String,
    #[serde(default)] refresh_token: Option<String>,
    #[serde(default)] token_type: Option<String>,
    #[serde(default)] scope: Option<String>,
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
        let blob = secrets::read_json_file(&secrets::gemini_creds_path())?;
        let mut creds: OauthCreds =
            serde_json::from_value(blob.clone()).map_err(|e| ProviderError::Parse(format!("gemini creds: {e}")))?;
        let now_ms = chrono::Utc::now().timestamp_millis();

        // Self-refresh on expiry (60s skew buffer) via Google OAuth, reusing the
        // Gemini CLI's public client_id/secret.
        if creds.expiry_date > 0 && creds.expiry_date < now_ms + 60_000 {
            match creds.refresh_token.clone() {
                Some(rt) => match refresh_gemini_token(&self.http, &rt).await {
                    Ok(fresh) => {
                        let new_exp = now_ms + (fresh.expires_in.unwrap_or(3600) as i64) * 1000;
                        let rt = fresh.refresh_token.as_ref().or(creds.refresh_token.as_ref());
                        let _ = write_back_creds(&blob, &fresh.access_token, rt, new_exp);
                        creds.access_token = fresh.access_token;
                        creds.expiry_date = new_exp;
                    }
                    Err(_) => {
                        return Err(ProviderError::Expired(
                            "Gemini token expired and refresh failed".into(),
                        ))
                    }
                },
                None => {
                    return Err(ProviderError::Expired(
                        "Gemini CLI token expired — run `gemini` once to refresh".into(),
                    ))
                }
            }
        }
        let token = &creds.access_token;

        // Project from env (Standard/Workspace accounts need it in the request).
        let env_project = std::env::var("GOOGLE_CLOUD_PROJECT")
            .ok()
            .or_else(|| std::env::var("GOOGLE_CLOUD_PROJECT_ID").ok());
        let load = self
            .post_internal(
                token,
                "loadCodeAssist",
                json!({
                    "cloudaicompanionProject": env_project,
                    "metadata": {
                        "ideType": "IDE_UNSPECIFIED",
                        "platform": "PLATFORM_UNSPECIFIED",
                        "pluginType": "GEMINI",
                        "duetProject": env_project,
                    }
                }),
            )
            .await?;
        // Project resolution: response → env, reject all-numeric.
        let project = load
            .get("cloudaicompanionProject")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or(env_project)
            .filter(|p| !p.chars().all(|c| c.is_ascii_digit()))
            .ok_or_else(|| ProviderError::Parse("no cloudaicompanionProject".into()))?;
        let quota_val = self
            .post_internal(token, "retrieveUserQuota", json!({ "project": project }))
            .await?;
        let quota: QuotaResp =
            serde_json::from_value(quota_val).map_err(|e| ProviderError::Parse(e.to_string()))?;
        // Tier: paidTier → currentTier fallback (free accounts have null paidTier).
        let tier = load
            .get("paidTier")
            .and_then(|t| t.get("name"))
            .and_then(|n| n.as_str())
            .or_else(|| load.get("currentTier").and_then(|t| t.get("name")).and_then(|n| n.as_str()))
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


#[derive(serde::Deserialize)]
struct Refreshed {
    access_token: String,
    #[serde(default)] refresh_token: Option<String>,
    #[serde(default)] expires_in: Option<u64>,
}

/// Resolve client metadata: env override > Gemini CLI's public constants.
fn gemini_client_metadata() -> (String, String) {
    if let (Ok(id), Ok(sec)) = (
        std::env::var("GEMINI_OAUTH_CLIENT_ID"),
        std::env::var("GEMINI_OAUTH_CLIENT_SECRET"),
    ) {
        if !id.is_empty() && !sec.is_empty() {
            return (id, sec);
        }
    }
    (GEMINI_CLIENT_ID.to_string(), GEMINI_CLIENT_SECRET.to_string())
}

async fn refresh_gemini_token(http: &reqwest::Client, rt: &str) -> Result<Refreshed, ProviderError> {
    let (id, sec) = gemini_client_metadata();
    let resp = http
        .post(GEMINI_TOKEN_URL)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", rt),
            ("client_id", id.as_str()),
            ("client_secret", sec.as_str()),
        ])
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(ProviderError::Status {
            status: status.as_u16(),
            body: text.chars().take(200).collect(),
        });
    }
    serde_json::from_str::<Refreshed>(&text).map_err(|e| ProviderError::Parse(format!("refresh: {e}")))
}

/// Write the refreshed access token back to oauth_creds.json (preserves all
/// other fields; keeps the old refresh_token if the response omits a new one).
fn write_back_creds(
    orig: &serde_json::Value,
    access_token: &str,
    refresh_token: Option<&String>,
    expiry_date: i64,
) -> Result<(), ProviderError> {
    let mut blob = orig.clone();
    if let Some(obj) = blob.as_object_mut() {
        obj.insert("access_token".into(), json!(access_token));
        if let Some(rt) = refresh_token {
            obj.insert("refresh_token".into(), json!(rt));
        }
        obj.insert("expiry_date".into(), json!(expiry_date));
        if let Ok(s) = serde_json::to_string_pretty(&blob) {
            let _ = std::fs::write(secrets::gemini_creds_path(), s);
        }
    }
    Ok(())
}
async fn post_code_assist(
    http: &reqwest::Client,
    token: &str,
    method: &str,
    payload: Value,
) -> Result<Value, ProviderError> {
    let url = format!("{CODE_ASSIST_BASE}:{method}");
    let resp = http
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    http::send_for_json(resp, &url).await
}

/// Fetch Gemini usage given an explicit token (manually-added accounts).
pub(crate) async fn fetch_with(
    http: &reqwest::Client,
    token: &str,
    label_override: Option<&str>,
) -> Result<ServiceUsage, ProviderError> {
    let load = post_code_assist(
        http,
        token,
        "loadCodeAssist",
        json!({ "metadata": { "ideType": "IDE_UNSPECIFIED", "platform": "PLATFORM_UNSPECIFIED", "pluginType": "GEMINI" } }),
    )
    .await?;
    let project = load
        .get("cloudaicompanionProject")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| ProviderError::Parse("no cloudaicompanionProject".into()))?;
    let quota_val = post_code_assist(http, token, "retrieveUserQuota", json!({ "project": project })).await?;
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
        account: label_override.map(|s| s.to_string()),
        error: None,
        windows,
        detail_windows,
    })
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
