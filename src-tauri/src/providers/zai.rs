//! z.ai GLM Coding Plan via API key. There is no z.ai CLI storing a token
//! locally, so the auto-detected path reads `ZAI_API_KEY` from the env; users
//! otherwise paste a key via Add account (stored accounts → `fetch_with`).
//!
//! Calls the SAME undocumented monitor endpoint the community tools poll:
//! `GET https://api.z.ai/api/monitor/usage/quota/limit` with
//! `Authorization: Bearer <api_key>`. It returns a `data.limits[]` array where
//! each entry is a quota window (5-hour / weekly token quotas, plus monthly
//! MCP-call quotas). The headline window is the higher-burn of the two token
//! windows; everything else (the other token window, MCP, per-model) drops
//! into `detail_windows`.
//!
//! When the plan is fully exhausted the server returns business `code` 1308
//! (short window) or 1310 (weekly/monthly) carrying a `data.next_flush_time`
//! reset timestamp — we surface that as a 100%-used window with `resets_at`.
//!
//! Source for the response shape: the `quotas` crate (`docs-usage/zai.md`,
//! `src/providers/zai.rs`) and the VS Code `vscode-zai-usage` extension, both
//! of which poll this same endpoint. Error codes per
//! https://docs.z.ai/api-reference/api-code.

use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use serde::Deserialize;
use serde_json::Value;

use crate::http;
use crate::model::{LimitWindow, Provider, ServiceUsage};
use crate::providers::ProviderError;

/// Undocumented monitor endpoint shared by every community z.ai usage tool.
const USAGE_URL: &str = "https://api.z.ai/api/monitor/usage/quota/limit";
/// Env var the auto-detected path reads (z.ai ships no local CLI token).
const ENV_KEY: &str = "ZAI_API_KEY";

/// Business codes the server returns (HTTP 200) when a quota window is
/// exhausted; both carry `data.next_flush_time`.
const CODE_USAGE_EXHAUSTED: i64 = 1308;
const CODE_PERIOD_EXHAUSTED: i64 = 1310;

#[derive(Deserialize, Default)]
struct ZaiResponse {
    #[serde(default)] code: Option<i64>,
    #[serde(default)] success: Option<bool>,
    #[serde(default)] msg: Option<String>,
    #[serde(default)] data: Option<ZaiData>,
}

#[derive(Deserialize, Default)]
struct ZaiData {
    /// Subscription tier: "lite" | "pro" | "max" | "unknown".
    #[serde(default)] level: Option<String>,
    #[serde(default)] limits: Vec<LimitEntry>,
}

#[derive(Deserialize, Default)]
struct LimitEntry {
    /// Human label, e.g. "5h Token", "Weekly Token", "MCP usage(1 Month)".
    #[serde(rename = "type", default)] limit_type: Option<String>,
    /// `TOKENS_LIMIT` (token quota) or `TIME_LIMIT` (MCP-call quota).
    #[serde(rename = "rawType", default)] raw_type: Option<String>,
    /// Period unit: 3 = hours, 5 = months, 6 = weeks.
    #[serde(default)] unit: Option<i64>,
    /// Total quota ceiling (tokens or call count).
    #[serde(default)] usage: Option<f64>,
    /// Alias for `usage` on some responses.
    #[serde(default)] total: Option<f64>,
    /// Amount consumed in the current period.
    #[serde(rename = "currentValue", default)] current_value: Option<f64>,
    #[serde(default)] remaining: Option<f64>,
    /// Consumption ratio 0–100 (authoritative when present).
    #[serde(default)] percentage: Option<f64>,
    /// Epoch milliseconds; absent for monthly TIME_LIMIT windows.
    #[serde(rename = "nextResetTime", default)] next_reset_time: Option<i64>,
    /// Per-MCP-tool breakdown (TIME_LIMIT entries only).
    #[serde(default, rename = "usageDetails")] usage_details: Vec<UsageDetail>,
}

#[derive(Deserialize, Default)]
struct UsageDetail {
    #[serde(default, rename = "modelCode")] model_code: Option<String>,
    #[serde(default)] usage: Option<f64>,
}

pub struct ZaiProvider {
    http: reqwest::Client,
}
impl Default for ZaiProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ZaiProvider {
    pub fn new() -> Self {
        Self {
            http: http::build_client(),
        }
    }
}

/// Where a limit entry lands in the unified model.
#[derive(PartialEq, Eq, Clone, Copy)]
enum Slot {
    FiveHour,
    Weekly,
    Detail,
}

fn classify(e: &LimitEntry) -> Slot {
    let raw = e.raw_type.as_deref().unwrap_or("");
    let typ = e.limit_type.as_deref().unwrap_or("");
    let tlow = typ.to_lowercase();
    let unit = e.unit.unwrap_or(0);

    if raw == "TOKENS_LIMIT" || raw.is_empty() {
        if unit == 3 || tlow.contains("5h") || tlow.contains("5 hour") || tlow.contains("5-hour") {
            return Slot::FiveHour;
        }
        if unit == 6 || tlow.contains("week") {
            return Slot::Weekly;
        }
        if raw == "TOKENS_LIMIT" {
            return Slot::Detail;
        }
    }
    Slot::Detail
}

/// Label for a window, or `None` when there is nothing showable (skip it).
fn window_label(e: &LimitEntry, slot: Slot) -> Option<String> {
    match slot {
        Slot::FiveHour => Some("5-hour".into()),
        Slot::Weekly => Some("Weekly".into()),
        Slot::Detail => {
            let typ = e.limit_type.as_deref().filter(|s| !s.is_empty());
            let raw = e.raw_type.as_deref().filter(|s| !s.is_empty());
            Some(typ.or(raw)?.to_string())
        }
    }
}

/// Pure: one limit entry → one normalized window (or None if unshowable).
fn entry_to_window(e: &LimitEntry, slot: Slot) -> Option<LimitWindow> {
    let label = window_label(e, slot)?;
    let limit = e.usage.or(e.total);
    let used = e.current_value.or_else(|| match (e.usage, e.remaining) {
        (Some(u), Some(r)) => Some((u - r).max(0.0)),
        _ => None,
    });
    let used_percent = e
        .percentage
        .map(|p| p as f32)
        .or_else(|| match (used, limit) {
            (Some(u), Some(l)) if l > 0.0 => Some((u / l * 100.0) as f32),
            _ => None,
        });
    let resets_at = e.next_reset_time.filter(|&ms| ms > 0).map(|ms| ms / 1000);
    Some(LimitWindow {
        label,
        used_percent,
        resets_at,
        used,
        limit,
    })
}

/// Pick the higher-burn token window as the headline; the other becomes a
/// detail window. `(headline, leftover)`.
fn pick_primary(
    a: Option<LimitWindow>,
    b: Option<LimitWindow>,
) -> (Option<LimitWindow>, Option<LimitWindow>) {
    match (a, b) {
        (None, None) => (None, None),
        (only @ Some(_), None) | (None, only @ Some(_)) => (only, None),
        (Some(x), Some(y)) => {
            let bx = x.used_percent.unwrap_or(0.0);
            let by = y.used_percent.unwrap_or(0.0);
            if by > bx {
                (Some(y), Some(x))
            } else {
                (Some(x), Some(y))
            }
        }
    }
}

/// Pure: `data` → (headline windows, detail windows). The headline is the
/// highest-burn of the 5-hour / weekly token windows; the other token window,
/// every MCP/time window, and per-model breakdowns populate `detail_windows`.
fn normalize(data: &ZaiData) -> (Vec<LimitWindow>, Vec<LimitWindow>) {
    let mut five_hour: Option<LimitWindow> = None;
    let mut weekly: Option<LimitWindow> = None;
    let mut detail: Vec<LimitWindow> = Vec::new();

    for e in &data.limits {
        let slot = classify(e);
        if let Some(lw) = entry_to_window(e, slot) {
            match slot {
                Slot::FiveHour if five_hour.is_none() => five_hour = Some(lw),
                Slot::Weekly if weekly.is_none() => weekly = Some(lw),
                _ => detail.push(lw),
            }
        }
        // Per-MCP-tool breakdown for TIME_LIMIT windows.
        if slot == Slot::Detail {
            for d in &e.usage_details {
                if let (Some(m), Some(u)) = (d.model_code.as_deref(), d.usage) {
                    detail.push(LimitWindow {
                        label: m.to_string(),
                        used_percent: None,
                        resets_at: None,
                        used: Some(u),
                        limit: None,
                    });
                }
            }
        }
    }

    let (headline, leftover) = pick_primary(five_hour, weekly);
    let mut windows = Vec::new();
    if let Some(h) = headline {
        windows.push(h);
    }
    if let Some(l) = leftover {
        detail.insert(0, l);
    }
    (windows, detail)
}

/// Parse the `next_flush_time` carried by exhausted (1308/1310) responses.
/// Accepts epoch ms (or s) as a number/numeric string, an RFC-3339 string, or
/// a `YYYY-MM-DD HH:MM:SS` string. Datetime strings are interpreted as UTC;
/// z.ai may emit UTC+8, so this is best-effort for the countdown.
fn parse_next_flush(v: &Value) -> Option<i64> {
    if let Some(n) = v.as_i64() {
        return Some(normalize_epoch_ms(n));
    }
    if let Some(f) = v.as_f64() {
        return Some(normalize_epoch_ms(f as i64));
    }
    let s = v.as_str()?.trim();
    if let Ok(n) = s.parse::<i64>() {
        return Some(normalize_epoch_ms(n));
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.timestamp());
    }
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Some(Utc.from_utc_datetime(&ndt).timestamp());
    }
    None
}

/// Epoch ms → seconds when the value is clearly milliseconds.
fn normalize_epoch_ms(n: i64) -> i64 {
    if n.abs() > 1_000_000_000_000 {
        n / 1000
    } else {
        n
    }
}

/// Pure: build the 100%-used window surfaced when the plan is exhausted.
fn exhausted_window(code: i64, data: Option<&Value>) -> LimitWindow {
    let label = if code == CODE_PERIOD_EXHAUSTED {
        "Weekly"
    } else {
        "5-hour"
    };
    let resets_at = data
        .and_then(|d| d.get("next_flush_time"))
        .and_then(parse_next_flush);
    LimitWindow {
        label: label.into(),
        used_percent: Some(100.0),
        resets_at,
        used: None,
        limit: None,
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Subscription tier → display plan, dropping empty/"unknown".
fn plan_from_level_str(level: Option<&str>) -> Option<String> {
    let level = level?.trim();
    if level.is_empty() || level.eq_ignore_ascii_case("unknown") {
        return None;
    }
    Some(capitalize(level))
}

#[async_trait]
impl crate::providers::ProviderApi for ZaiProvider {
    fn key(&self) -> Provider {
        Provider::Zai
    }

    async fn fetch(&self) -> Result<ServiceUsage, ProviderError> {
        let key = std::env::var(ENV_KEY).map_err(|_| {
            ProviderError::NotLoggedIn(
                "z.ai API key not set; add it via Add account or export ZAI_API_KEY".into(),
            )
        })?;
        fetch_with(&self.http, &key, None).await
    }
}

/// Fetch z.ai usage given an explicit API key (used for stored accounts and
/// the env-var auto-detected path). API keys do not expire.
pub(crate) async fn fetch_with(
    http: &reqwest::Client,
    api_key: &str,
    label_override: Option<&str>,
) -> Result<ServiceUsage, ProviderError> {
    let resp = http
        .get(USAGE_URL)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    // Capture as Value first — the endpoint is undocumented and the exhausted
    // path needs `code` / `data.next_flush_time` before struct conversion.
    let raw: Value = http::send_for_json(resp, USAGE_URL).await?;

    let code = raw.get("code").and_then(|v| v.as_i64());
    let exhausted_code = code.filter(|&c| c == CODE_USAGE_EXHAUSTED || c == CODE_PERIOD_EXHAUSTED);
    if let Some(c) = exhausted_code {
        let plan = plan_from_level_str(
            raw.get("data")
                .and_then(|d| d.get("level"))
                .and_then(|v| v.as_str()),
        );
        return Ok(ServiceUsage {
            provider: Provider::Zai,
            connected: true,
            plan,
            account: label_override.map(String::from),
            error: None,
            windows: vec![exhausted_window(c, raw.get("data"))],
            detail_windows: vec![],
        });
    }

    let u: ZaiResponse = serde_json::from_value(raw)
        .map_err(|e| ProviderError::Parse(format!("zai usage: {e}")))?;
    let ok = u.success.unwrap_or(false) || u.code == Some(200);
    if !ok {
        let msg = u
            .msg
            .unwrap_or_else(|| "z.ai quota request failed".to_string());
        return Err(ProviderError::Status {
            status: 200,
            body: format!("{USAGE_URL}: {msg}"),
        });
    }

    let plan = plan_from_level_str(u.data.as_ref().and_then(|d| d.level.as_deref()));
    let (windows, detail_windows) = u.data.as_ref().map(normalize).unwrap_or_default();
    Ok(ServiceUsage {
        provider: Provider::Zai,
        connected: true,
        plan,
        account: label_override.map(String::from),
        error: None,
        windows,
        detail_windows,
    })
}

/// API keys do not expire, so there is nothing to refresh.
pub(crate) async fn refresh_stored(
    _: &reqwest::Client,
    _: &crate::store::StoredCredential,
) -> Option<crate::store::StoredCredential> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_fixture_headline_is_weekly() {
        let v: ZaiResponse =
            serde_json::from_str(include_str!("../../tests/zai_quota_fixture.json")).unwrap();
        let data = v.data.expect("fixture has data");
        let (ws, _detail) = normalize(&data);

        // Higher-burn token window (53% > 7%) is the sole headline.
        assert_eq!(ws.len(), 1);
        let headline = &ws[0];
        assert_eq!(headline.label, "Weekly");
        assert_eq!(headline.used_percent, Some(53.0));
        assert_eq!(headline.used, Some(2_650_000.0));
        assert_eq!(headline.limit, Some(5_000_000.0));
        assert_eq!(headline.resets_at, Some(1_713_388_800)); // ms → s
    }

    #[test]
    fn normalize_fixture_detail_has_5h_mcp_and_models() {
        let v: ZaiResponse =
            serde_json::from_str(include_str!("../../tests/zai_quota_fixture.json")).unwrap();
        let (_, detail) = normalize(v.data.as_ref().unwrap());

        let five = detail.iter().find(|w| w.label == "5-hour").unwrap();
        assert_eq!(five.used_percent, Some(7.0));
        assert_eq!(five.used, Some(72_000.0));
        assert_eq!(five.limit, Some(1_000_000.0));
        assert_eq!(five.resets_at, Some(1_712_956_800));

        let mcp = detail
            .iter()
            .find(|w| w.label == "MCP usage(1 Month)")
            .unwrap();
        assert_eq!(mcp.used_percent, Some(4.0));
        assert_eq!(mcp.used, Some(42.0));
        assert_eq!(mcp.limit, Some(1000.0));

        // Per-MCP-tool breakdown.
        let search = detail.iter().find(|w| w.label == "search-prime").unwrap();
        assert_eq!(search.used, Some(20.0));
        assert!(detail.iter().any(|w| w.label == "web-reader" && w.used == Some(15.0)));
        assert!(detail.iter().any(|w| w.label == "zread" && w.used == Some(7.0)));
    }

    #[test]
    fn percentage_falls_back_to_used_over_limit() {
        let e = LimitEntry {
            limit_type: Some("Weekly Token".into()),
            raw_type: Some("TOKENS_LIMIT".into()),
            unit: Some(6),
            usage: Some(2000.0),
            current_value: Some(500.0),
            percentage: None,
            ..Default::default()
        };
        let lw = entry_to_window(&e, classify(&e)).unwrap();
        assert_eq!(lw.used_percent, Some(25.0)); // 500 / 2000 * 100
        assert_eq!(lw.used, Some(500.0));
        assert_eq!(lw.limit, Some(2000.0));
    }

    #[test]
    fn used_derives_from_usage_minus_remaining_when_current_value_absent() {
        let e = LimitEntry {
            limit_type: Some("5h Token".into()),
            raw_type: Some("TOKENS_LIMIT".into()),
            unit: Some(3),
            usage: Some(1000.0),
            current_value: None,
            remaining: Some(250.0),
            percentage: None,
            ..Default::default()
        };
        let lw = entry_to_window(&e, classify(&e)).unwrap();
        assert_eq!(lw.used, Some(750.0));
        assert_eq!(lw.used_percent, Some(75.0));
    }

    #[test]
    fn exhausted_1308_window_carries_next_flush_time() {
        let flush_ms: i64 = 1_712_956_800_000;
        let raw = serde_json::json!({
            "code": 1308,
            "data": { "next_flush_time": flush_ms }
        });
        let lw = exhausted_window(1308, raw.get("data"));
        assert_eq!(lw.label, "5-hour");
        assert_eq!(lw.used_percent, Some(100.0));
        assert_eq!(lw.resets_at, Some(1_712_956_800)); // ms → s
    }

    #[test]
    fn exhausted_1310_window_parses_datetime_flush_time() {
        let raw = serde_json::json!({
            "code": 1310,
            "data": { "next_flush_time": "2024-04-12 00:00:00" }
        });
        let lw = exhausted_window(1310, raw.get("data"));
        assert_eq!(lw.label, "Weekly");
        assert_eq!(lw.used_percent, Some(100.0));
        assert_eq!(lw.resets_at, Some(1_712_880_000)); // 2024-04-12 00:00:00 UTC
    }

    #[test]
    fn empty_response_does_not_panic() {
        let v: ZaiResponse = serde_json::from_str(
            r#"{"code":200,"success":true,"data":{}}"#,
        )
        .unwrap();
        let (ws, detail) = normalize(v.data.as_ref().unwrap());
        assert!(ws.is_empty());
        assert!(detail.is_empty());
    }

    #[test]
    fn missing_data_does_not_panic() {
        let v: ZaiResponse = serde_json::from_str(r#"{"code":200,"success":true}"#).unwrap();
        assert!(v.data.is_none());
        assert_eq!(plan_from_level_str(None), None);
    }

    #[test]
    fn plan_level_drops_unknown_and_capitalizes() {
        assert_eq!(plan_from_level_str(Some("pro")), Some("Pro".into()));
        assert_eq!(plan_from_level_str(Some("unknown")), None);
        assert_eq!(plan_from_level_str(Some("")), None);
        assert_eq!(plan_from_level_str(Some("  max  ")), Some("Max".into()));
    }

    #[tokio::test]
    async fn refresh_stored_is_none() {
        // API keys do not expire — nothing to refresh.
        let cred = crate::store::StoredCredential {
            id: "x".into(),
            provider: Provider::Zai,
            label: "x".into(),
            access_token: "k".into(),
            refresh_token: None,
            expires_at: 0,
            id_token: None,
            account_id: None,
        };
        assert!(refresh_stored(&http::build_client(), &cred).await.is_none());
    }
}
