//! Gemini — usage fetch via stored OAuth accounts only.
//!
//! The Gemini CLI migrated off `~/.gemini/oauth_creds.json` (it deletes the
//! file) to an OS keychain / encrypted store we can't reliably read, so CLI
//! auto-detect is dropped. Gemini is supported exclusively via in-app OAuth
//! ("Add account") stored accounts; refresh uses the same Google OAuth endpoint
//! (`refresh_gemini_token`) and the CLI's public client_id/secret
//! (env override `GEMINI_OAUTH_CLIENT_ID`/`..._SECRET`).

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http;
use crate::model::{auto_service_id, LimitWindow, Provider, ServiceSource, ServiceUsage};
use crate::providers::ProviderError;

const CODE_ASSIST_BASE: &str = "https://cloudcode-pa.googleapis.com/v1internal";
const GEMINI_CLIENT_ID: &str =
    "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com";
const GEMINI_CLIENT_SECRET: &str = "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";
const GEMINI_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Bucket {
    #[serde(default)]
    model_id: Option<String>,
    #[serde(default)]
    remaining_fraction: Option<f64>,
    #[serde(default)]
    remaining_amount: Option<String>,
    #[serde(default)]
    reset_time: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Deserialize, Default)]
struct QuotaResp {
    #[serde(default)]
    buckets: Vec<Bucket>,
}

pub struct GeminiProvider;

impl Default for GeminiProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl GeminiProvider {
    pub fn new() -> Self {
        Self
    }
}

/// Pure: buckets → LimitWindows.
///
/// The live `retrieveUserQuota` response returns one bucket per MODEL, not
/// per time-window. Two distinct patterns appear:
/// - **Real quota windows**: future `resetTime` + `remainingFraction` ∈ [0,1].
///   These become LimitWindows; the highest-burn one is the card headline.
/// - **Tier-restricted models**: `resetTime` = epoch (1970-01-01) +
///   `remainingFraction` = 0. These are models NOT available on the user's
///   current tier (e.g. Pro models on Free). They are NOT "100% used" — we
///   surface them as labeled notes in `detail_windows` so the user can see
///   what they're missing, without polluting the usage math.
fn normalize(resp: &QuotaResp) -> (Vec<LimitWindow>, Vec<LimitWindow>) {
    let mut real: Vec<LimitWindow> = Vec::new();
    let mut restricted: Vec<String> = Vec::new();

    for b in &resp.buckets {
        // Epoch resetTime (1970-01-01) ⇒ tier-restricted model, not a real
        // quota window. Surface as a labeled note instead of a usage bar.
        let is_restricted = b.reset_time.map(|d| d.timestamp() <= 0).unwrap_or(false);
        if is_restricted {
            if let Some(model) = &b.model_id {
                restricted.push(model.clone());
            }
            continue;
        }
        let label = b.model_id.clone().unwrap_or_else(|| "Gemini".into());
        let remaining_fraction = b.remaining_fraction.map(|f| f.clamp(0.0, 1.0));
        let used_percent = remaining_fraction.map(|f| ((1.0 - f) * 100.0) as f32);
        // The live response carries no `remainingAmount`; the absolute limit
        // (e.g. 1500/day) isn't derivable from `remainingFraction` alone, so
        // `limit` stays `None`. Older hypothetical fixtures with
        // `remainingAmount` still compute it for back-compat.
        let limit = match (&b.remaining_amount, remaining_fraction) {
            (Some(amt), Some(f)) if f > 0.0 => amt.parse::<f64>().ok().map(|r| r / f),
            _ => None,
        };
        real.push(LimitWindow {
            label,
            used_percent,
            resets_at: b.reset_time.map(|d| d.timestamp()),
            used: None,
            limit,
        });
    }

    let restricted_notes: Vec<LimitWindow> = restricted
        .into_iter()
        .map(|model| LimitWindow {
            label: format!("{model} (not on your tier)"),
            used_percent: None,
            resets_at: None,
            used: None,
            limit: None,
        })
        .collect();
    if real.is_empty() {
        return (vec![], restricted_notes);
    }
    // Card headline = most-consumed real bucket (highest used_percent; ties
    // broken by first-seen so the order is stable).
    let mut idx = 0;
    let mut best = f32::MIN;
    for (i, w) in real.iter().enumerate() {
        let p = w.used_percent.unwrap_or(-1.0);
        if p > best {
            best = p;
            idx = i;
        }
    }
    let headline = real[idx].clone();
    let mut detail: Vec<LimitWindow> = real
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != idx)
        .map(|(_, w)| w.clone())
        .collect();
    // Tier-restricted models appended as labeled notes (no percent, no reset).
    detail.extend(restricted_notes);
    (vec![headline], detail)
}

#[async_trait]
impl crate::providers::ProviderApi for GeminiProvider {
    fn key(&self) -> Provider {
        Provider::Gemini
    }

    async fn fetch(&self) -> Result<ServiceUsage, ProviderError> {
        // Gemini is supported via in-app OAuth (Add account → stored) ONLY. The
        // Gemini CLI migrated off ~/.gemini/oauth_creds.json (deletes it) to an OS
        // keychain / encrypted file we can't reliably read, so CLI auto-detect is
        // dropped. A connected Gemini comes only from a stored OAuth account.
        Err(ProviderError::NotLoggedIn(
            "Gemini auto-detect disabled — add via Add account (OAuth)".into(),
        ))
    }
}

#[derive(serde::Deserialize)]
struct Refreshed {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
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
    (
        GEMINI_CLIENT_ID.to_string(),
        GEMINI_CLIENT_SECRET.to_string(),
    )
}

async fn refresh_gemini_token(
    http: &reqwest::Client,
    rt: &str,
) -> Result<Refreshed, ProviderError> {
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
    let v = http::send_for_json(resp, "gemini refresh").await?;
    serde_json::from_value::<Refreshed>(v)
        .map_err(|e| ProviderError::Parse(format!("gemini refresh: {e}")))
}

async fn post_code_assist(
    http: &reqwest::Client,
    token: &str,
    method: &str,
    payload: Value,
) -> Result<Value, ProviderError> {
    http::post_json(
        http,
        token,
        &format!("{CODE_ASSIST_BASE}:{method}"),
        &payload,
    )
    .await
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
    let quota_val = post_code_assist(
        http,
        token,
        "retrieveUserQuota",
        json!({ "project": project }),
    )
    .await?;
    let raw_json = serde_json::to_string_pretty(&serde_json::json!({
        "loadCodeAssist": &load,
        "retrieveUserQuota": &quota_val,
    }))
    .ok();
    let quota: QuotaResp =
        serde_json::from_value(quota_val).map_err(|e| ProviderError::Parse(e.to_string()))?;
    let tier = load
        .get("paidTier")
        .and_then(|t| t.get("name"))
        .and_then(|n| n.as_str())
        .map(String::from);
    let (windows, detail_windows) = normalize(&quota);
    Ok(ServiceUsage {
        id: auto_service_id(Provider::Gemini),
        source: ServiceSource::Auto,
        provider: Provider::Gemini,
        connected: true,
        plan: tier,
        account: label_override.map(|s| s.to_string()),
        error: None,
        windows,
        detail_windows,
        raw_response: raw_json,
    })
}

/// Build a refreshed `StoredCredential` from a successful token refresh.
/// Pure: preserves `id`/`provider`/`label`/`id_token`/`account_id`, rotates
/// `access_token`/`refresh_token`/`expires_at`. Keeps the old refresh_token
/// when Google's response omits a fresh one (Google rotates them rarely).
fn build_refreshed_cred(
    cred: &crate::store::StoredCredential,
    fresh: &Refreshed,
    now_ms: i64,
) -> crate::store::StoredCredential {
    let expires_at = now_ms + (fresh.expires_in.unwrap_or(3600) as i64) * 1000;
    crate::store::rotate_credential(
        cred,
        fresh.access_token.clone(),
        fresh.refresh_token.clone(),
        None,
        expires_at,
    )
}

/// Refresh a stored credential's access_token using its refresh_token via the
/// same Google OAuth endpoint + client metadata (env override > Gemini CLI's
/// public constants) as the CLI-path self-refresh. Returns `Some(updated_cred)`
/// when a refresh happened (caller persists); `None` if there is no
/// refresh_token or the refresh failed (caller falls back to the existing token).
pub(crate) async fn refresh_stored(
    http: &reqwest::Client,
    cred: &crate::store::StoredCredential,
) -> Option<crate::store::StoredCredential> {
    let rt = cred.refresh_token.as_ref()?;
    let fresh = refresh_gemini_token(http, rt).await.ok()?;
    let now_ms = chrono::Utc::now().timestamp_millis();
    Some(build_refreshed_cred(cred, &fresh, now_ms))
}

/// Fetch usage for a stored Gemini account (uniform stored-fetch adapter).
pub(crate) async fn fetch_stored(
    http: &reqwest::Client,
    cred: &crate::store::StoredCredential,
) -> Result<ServiceUsage, ProviderError> {
    fetch_with(http, &cred.access_token, Some(&cred.label)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_live_fixture_skips_restricted_and_picks_headline() {
        // Real response captured 2026-06-18 from a Free-tier account:
        // 5 available models all at 100% remaining + 3 Pro models locked
        // (epoch resetTime + remainingFraction 0).
        let q: QuotaResp =
            serde_json::from_str(include_str!("../../tests/gemini_quota_fixture.json")).unwrap();
        let (ws, detail) = normalize(&q);

        // One headline (highest-burn available bucket — all tie at 0% so the
        // first available model wins).
        assert_eq!(ws.len(), 1);
        let headline = &ws[0];
        assert_eq!(headline.label, "gemini-2.5-flash");
        assert_eq!(headline.used_percent, Some(0.0));
        assert!(headline.limit.is_none()); // no remainingAmount in live response
        assert_eq!(headline.resets_at, Some(1_781_970_996)); // 2026-06-20T15:56:36Z

        // Detail = 4 other available models + 3 tier-restricted notes (in order).
        let available: Vec<_> = detail.iter().filter(|w| w.used_percent.is_some()).collect();
        assert_eq!(available.len(), 4, "4 other available models in detail");
        assert!(detail.iter().any(|w| w.label == "gemini-2.5-flash-lite"));
        assert!(detail.iter().any(|w| w.label == "gemini-3-flash-preview"));

        // Tier-restricted models surface as labeled notes with no percent.
        let notes: Vec<_> = detail.iter().filter(|w| w.used_percent.is_none()).collect();
        assert_eq!(notes.len(), 3, "3 Pro models locked on Free tier");
        assert!(notes.iter().any(|w| w.label.contains("gemini-2.5-pro")));
        assert!(notes.iter().any(|w| w.label.contains("not on your tier")));
    }

    #[test]
    fn normalize_picks_highest_burn_as_headline() {
        // Synthetic: one model partially consumed must become the headline.
        let q = QuotaResp {
            buckets: vec![
                Bucket {
                    model_id: Some("gemini-2.5-flash".into()),
                    remaining_fraction: Some(1.0),
                    remaining_amount: None,
                    reset_time: Some(
                        chrono::DateTime::parse_from_rfc3339("2026-06-20T00:00:00Z")
                            .unwrap()
                            .with_timezone(&chrono::Utc),
                    ),
                },
                Bucket {
                    model_id: Some("gemini-2.5-pro".into()),
                    remaining_fraction: Some(0.25), // 75% used
                    remaining_amount: None,
                    reset_time: Some(
                        chrono::DateTime::parse_from_rfc3339("2026-06-20T00:00:00Z")
                            .unwrap()
                            .with_timezone(&chrono::Utc),
                    ),
                },
            ],
        };
        let (ws, detail) = normalize(&q);
        assert_eq!(ws[0].label, "gemini-2.5-pro");
        assert_eq!(ws[0].used_percent, Some(75.0));
        assert_eq!(detail.len(), 1);
        assert_eq!(detail[0].label, "gemini-2.5-flash");
    }

    #[test]
    fn normalize_clamps_remaining_fraction_to_usage_bounds() {
        let reset = chrono::DateTime::parse_from_rfc3339("2026-06-20T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let q = QuotaResp {
            buckets: vec![
                Bucket {
                    model_id: Some("over-remaining".into()),
                    remaining_fraction: Some(1.2),
                    remaining_amount: None,
                    reset_time: Some(reset),
                },
                Bucket {
                    model_id: Some("over-used".into()),
                    remaining_fraction: Some(-0.5),
                    remaining_amount: None,
                    reset_time: Some(reset),
                },
            ],
        };
        let (ws, detail) = normalize(&q);
        assert_eq!(ws[0].label, "over-used");
        assert_eq!(ws[0].used_percent, Some(100.0));
        assert_eq!(detail[0].label, "over-remaining");
        assert_eq!(detail[0].used_percent, Some(0.0));
    }

    #[test]
    fn normalize_preserves_restricted_only_buckets_as_detail_notes() {
        let restricted_reset = chrono::DateTime::parse_from_rfc3339("1970-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let q = QuotaResp {
            buckets: vec![Bucket {
                model_id: Some("gemini-2.5-pro".into()),
                remaining_fraction: Some(0.0),
                remaining_amount: None,
                reset_time: Some(restricted_reset),
            }],
        };
        let (ws, detail) = normalize(&q);
        assert!(ws.is_empty());
        assert_eq!(detail.len(), 1);
        assert_eq!(detail[0].label, "gemini-2.5-pro (not on your tier)");
        assert_eq!(detail[0].used_percent, None);
    }

    fn sample_stored(rt: Option<&str>) -> crate::store::StoredCredential {
        crate::store::StoredCredential {
            id: "abc".into(),
            provider: Provider::Gemini,
            label: "gemini-stored@example.invalid".into(),
            access_token: "old-at".into(),
            refresh_token: rt.map(str::to_string),
            expires_at: 1,
            id_token: Some("jwt-payload".into()),
            account_id: None,
        }
    }

    #[test]
    fn build_refreshed_cred_rotates_tokens_and_preserves_fields() {
        let cred = sample_stored(Some("old-rt"));
        let fresh = Refreshed {
            access_token: "new-at".into(),
            refresh_token: Some("new-rt".into()),
            expires_in: Some(3600),
        };
        let out = build_refreshed_cred(&cred, &fresh, 1_000_000);
        // Preserved verbatim.
        assert_eq!(out.id, "abc");
        assert_eq!(out.provider, Provider::Gemini);
        assert_eq!(out.label, "gemini-stored@example.invalid");
        assert_eq!(out.id_token.as_deref(), Some("jwt-payload"));
        assert!(out.account_id.is_none());
        // Rotated.
        assert_eq!(out.access_token, "new-at");
        assert_eq!(out.refresh_token.as_deref(), Some("new-rt"));
        assert_eq!(out.expires_at, 1_000_000 + 3600 * 1000);
    }

    #[test]
    fn build_refreshed_cred_keeps_old_rt_when_response_omits_one() {
        // Google rarely rotates refresh_tokens; the response may omit it.
        let cred = sample_stored(Some("old-rt"));
        let fresh = Refreshed {
            access_token: "new-at".into(),
            refresh_token: None,
            expires_in: None, // default 3600s fallback applies
        };
        let out = build_refreshed_cred(&cred, &fresh, 0);
        assert_eq!(out.access_token, "new-at");
        assert_eq!(out.refresh_token.as_deref(), Some("old-rt"));
        assert_eq!(out.expires_at, 3600 * 1000);
    }

    #[tokio::test]
    async fn auto_fetch_is_oauth_only_not_logged_in() {
        use crate::providers::ProviderApi;
        // Gemini auto-detect of the CLI is unsupported; only in-app OAuth (stored)
        // is. The auto provider must report not_logged_in without touching disk.
        let out = GeminiProvider::new().fetch().await;
        match out {
            Err(crate::providers::ProviderError::NotLoggedIn(_)) => {}
            other => panic!("expected NotLoggedIn, got {other:?}"),
        }
    }
}
