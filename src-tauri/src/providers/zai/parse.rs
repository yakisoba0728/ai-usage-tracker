//! z.ai response structs + the `normalize()` core. The monitor endpoint returns
//! a `data.limits[]` array where each entry is a quota window (5-hour / weekly
//! token quotas, plus monthly MCP-call quotas). When the plan is exhausted the
//! server returns business `code` 1308/1310 carrying a `next_flush_time`.

use chrono::{TimeZone, Utc};
use serde::Deserialize;
use serde_json::Value;

use crate::model::LimitWindow;

/// Business codes the server returns (HTTP 200) when a quota window is
/// exhausted; both carry `data.next_flush_time`.
pub(super) const CODE_USAGE_EXHAUSTED: i64 = 1308;
pub(super) const CODE_PERIOD_EXHAUSTED: i64 = 1310;

#[derive(Deserialize, Default)]
pub(super) struct ZaiResponse {
    #[serde(default)]
    pub(super) code: Option<i64>,
    #[serde(default)]
    pub(super) success: Option<bool>,
    #[serde(default)]
    pub(super) msg: Option<String>,
    #[serde(default)]
    pub(super) data: Option<ZaiData>,
}

#[derive(Deserialize, Default)]
pub(super) struct ZaiData {
    /// Subscription tier: "lite" | "pro" | "max" | "unknown".
    #[serde(default)]
    pub(super) level: Option<String>,
    #[serde(default)]
    limits: Vec<LimitEntry>,
}

#[derive(Deserialize, Default)]
struct LimitEntry {
    /// Human label, e.g. "5h Token", "Weekly Token", "MCP usage(1 Month)".
    #[serde(rename = "type", default)]
    limit_type: Option<String>,
    /// `TOKENS_LIMIT` (token quota) or `TIME_LIMIT` (MCP-call quota).
    #[serde(rename = "rawType", default)]
    raw_type: Option<String>,
    /// Period unit: 3 = hours, 5 = months, 6 = weeks.
    #[serde(default)]
    unit: Option<i64>,
    /// Total quota ceiling (tokens or call count).
    #[serde(default)]
    usage: Option<f64>,
    /// Alias for `usage` on some responses.
    #[serde(default)]
    total: Option<f64>,
    /// Amount consumed in the current period.
    #[serde(rename = "currentValue", default)]
    current_value: Option<f64>,
    #[serde(default)]
    remaining: Option<f64>,
    /// Consumption ratio 0–100 (authoritative when present).
    #[serde(default)]
    percentage: Option<f64>,
    /// Epoch milliseconds; absent for monthly TIME_LIMIT windows.
    #[serde(rename = "nextResetTime", default)]
    next_reset_time: Option<i64>,
    /// Per-MCP-tool breakdown (TIME_LIMIT entries only).
    #[serde(default, rename = "usageDetails")]
    usage_details: Vec<UsageDetail>,
}

#[derive(Deserialize, Default)]
struct UsageDetail {
    #[serde(default, rename = "modelCode")]
    model_code: Option<String>,
    #[serde(default)]
    usage: Option<f64>,
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
            // The live API emits raw enum values (`TIME_LIMIT`, `TOKENS_LIMIT`)
            // rather than human labels — derive a friendly name from the period
            // unit when possible, else fall back to the type string.
            match e.unit.unwrap_or(0) {
                3 => Some("5-hour".into()),
                5 => Some("Monthly".into()),
                6 => Some("Weekly".into()),
                _ => {
                    let typ = e.limit_type.as_deref().filter(|s| !s.is_empty());
                    let raw = e.raw_type.as_deref().filter(|s| !s.is_empty());
                    Some(typ.or(raw)?.to_string())
                }
            }
        }
    }
}

/// Pure: one limit entry → one normalized window (or None if unshowable).
fn entry_to_window(e: &LimitEntry, slot: Slot) -> Option<LimitWindow> {
    let label = window_label(e, slot)?;
    let limit = e.usage.or(e.total);
    let used = e.current_value.or_else(|| match (limit, e.remaining) {
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
    let resets_at = e.next_reset_time.filter(|&t| t > 0).map(normalize_epoch_ms);
    if used_percent.is_none() && resets_at.is_none() && used.is_none() && limit.is_none() {
        return None;
    }
    Some(LimitWindow {
        label,
        used_percent,
        resets_at,
        used,
        limit,
    })
}

/// Pure: `data` → (card windows, detail windows). BOTH token windows go on the
/// card with the **5-hour first** (so it's the headline, consistent with Claude/
/// Codex where the short window leads), the weekly second. Every MCP/time window
/// and per-model breakdown populates `detail_windows`.
pub(super) fn normalize(data: &ZaiData) -> (Vec<LimitWindow>, Vec<LimitWindow>) {
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

    // Both token windows go on the card, 5-hour first (headline), then weekly —
    // consistent with Claude/Codex (short window leads), regardless of burn.
    let mut windows = Vec::new();
    if let Some(w) = five_hour {
        windows.push(w);
    }
    if let Some(w) = weekly {
        windows.push(w);
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
pub(super) fn exhausted_window(code: i64, data: Option<&Value>) -> LimitWindow {
    let label = if code == CODE_PERIOD_EXHAUSTED {
        "Period"
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

/// Subscription tier → display plan, dropping empty/"unknown".
pub(super) fn plan_from_level_str(level: Option<&str>) -> Option<String> {
    let level = level?.trim();
    if level.is_empty() || level.eq_ignore_ascii_case("unknown") {
        return None;
    }
    Some(crate::util::capitalize(level))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_live_fixture_card_shows_5h_then_weekly() {
        // Real response captured 2026-06-18 from GET /api/monitor/usage/quota/limit.
        let v: ZaiResponse =
            serde_json::from_str(include_str!("../../../tests/zai_quota_fixture.json")).unwrap();
        let data = v.data.expect("fixture has data");
        let (ws, _detail) = normalize(&data);

        // BOTH token windows on the card, 5-hour FIRST (headline) then weekly —
        // regardless of burn (consistent with Claude/Codex short-window-leads).
        assert_eq!(ws.len(), 2);
        assert_eq!(ws[0].label, "5-hour");
        assert_eq!(ws[0].used_percent, Some(9.0));
        assert_eq!(ws[0].resets_at, Some(1_781_897_705));
        assert_eq!(ws[1].label, "Weekly");
        assert_eq!(ws[1].used_percent, Some(60.0));
        // Live TOKENS_LIMIT entries carry percentage only — no usage/currentValue.
        assert_eq!(ws[1].used, None);
        assert_eq!(ws[1].limit, None);
        assert_eq!(ws[1].resets_at, Some(1_782_210_628)); // 1782210628998 ms → s
    }

    #[test]
    fn normalize_live_fixture_detail_has_monthly_and_models_not_5h() {
        let v: ZaiResponse =
            serde_json::from_str(include_str!("../../../tests/zai_quota_fixture.json")).unwrap();
        let (ws, detail) = normalize(v.data.as_ref().unwrap());

        // 5-hour is now a CARD window, not a detail window.
        assert!(ws.iter().any(|w| w.label == "5-hour"));
        assert!(!detail.iter().any(|w| w.label == "5-hour"));

        // Monthly window (TIME_LIMIT unit 5) — full detail present.
        let monthly = detail.iter().find(|w| w.label == "Monthly").unwrap();
        assert_eq!(monthly.used_percent, Some(1.0));
        assert_eq!(monthly.used, Some(7.0));
        assert_eq!(monthly.limit, Some(4000.0));
        assert_eq!(monthly.resets_at, Some(1_783_852_228));

        // Per-tool breakdown.
        assert!(detail
            .iter()
            .any(|w| w.label == "search-prime" && w.used == Some(1.0)));
        assert!(detail
            .iter()
            .any(|w| w.label == "web-reader" && w.used == Some(0.0)));
        assert!(detail
            .iter()
            .any(|w| w.label == "zread" && w.used == Some(6.0)));
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
    fn used_derives_from_total_minus_remaining_when_usage_absent() {
        let e = LimitEntry {
            limit_type: Some("Weekly Token".into()),
            raw_type: Some("TOKENS_LIMIT".into()),
            unit: Some(6),
            usage: None,
            total: Some(1000.0),
            current_value: None,
            remaining: Some(250.0),
            percentage: None,
            ..Default::default()
        };
        let lw = entry_to_window(&e, classify(&e)).unwrap();
        assert_eq!(lw.limit, Some(1000.0));
        assert_eq!(lw.used, Some(750.0));
        assert_eq!(lw.used_percent, Some(75.0));
    }

    #[test]
    fn next_reset_time_accepts_seconds_and_milliseconds() {
        let mut e = LimitEntry {
            limit_type: Some("5h Token".into()),
            raw_type: Some("TOKENS_LIMIT".into()),
            unit: Some(3),
            usage: Some(100.0),
            current_value: Some(25.0),
            next_reset_time: Some(1_781_897_705),
            ..Default::default()
        };
        let seconds = entry_to_window(&e, classify(&e)).unwrap();
        assert_eq!(seconds.resets_at, Some(1_781_897_705));

        e.next_reset_time = Some(1_781_897_705_337);
        let millis = entry_to_window(&e, classify(&e)).unwrap();
        assert_eq!(millis.resets_at, Some(1_781_897_705));
    }

    #[test]
    fn blank_limit_entry_is_not_emitted() {
        let e = LimitEntry {
            limit_type: Some("5h Token".into()),
            raw_type: Some("TOKENS_LIMIT".into()),
            unit: Some(3),
            ..Default::default()
        };
        assert!(entry_to_window(&e, classify(&e)).is_none());
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
        assert_eq!(lw.label, "Period");
        assert_eq!(lw.used_percent, Some(100.0));
        assert_eq!(lw.resets_at, Some(1_712_880_000)); // 2024-04-12 00:00:00 UTC
    }

    #[test]
    fn empty_response_does_not_panic() {
        let v: ZaiResponse =
            serde_json::from_str(r#"{"code":200,"success":true,"data":{}}"#).unwrap();
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
}
