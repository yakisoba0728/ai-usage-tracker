//! Cursor response structs + the `normalize()` core (money is USD cents).

use serde::Deserialize;
use serde_json::Value;

use crate::model::LimitWindow;

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(super) struct CursorUsage {
    #[serde(default)]
    pub(super) enabled: Option<bool>,
    #[serde(default)]
    pub(super) billing_cycle_end: Option<Value>,
    #[serde(default)]
    pub(super) plan_usage: Option<PlanUsage>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(super) struct PlanUsage {
    #[serde(default)]
    pub(super) included_spend: Option<f64>,
    #[serde(default)]
    pub(super) total_spend: Option<f64>,
    #[serde(default)]
    pub(super) remaining: Option<f64>,
    #[serde(default)]
    pub(super) limit: Option<f64>,
    #[serde(default)]
    pub(super) total_percent_used: Option<f64>,
}

fn parse_epoch_seconds(v: &Value) -> Option<i64> {
    let n = v
        .as_i64()
        .or_else(|| v.as_f64().map(|f| f as i64))
        .or_else(|| v.as_str()?.trim().parse::<i64>().ok())?;
    if n.abs() > 1_000_000_000_000 {
        Some(n / 1000)
    } else {
        Some(n)
    }
}

/// Pure: planUsage → window. Money is cents → dollars.
pub(super) fn normalize(u: &CursorUsage) -> Vec<LimitWindow> {
    if u.enabled == Some(false) {
        return vec![];
    }
    let Some(p) = &u.plan_usage else {
        return vec![];
    };
    // Fallback order mirrors ClearMeasureLabs/cursor-usage-status
    // (src/usageModel.ts::parseDashboardPeriodUsage): includedSpend first,
    // then limit − remaining (clamped ≥ 0), then totalSpend as last resort.
    // totalSpend is last because it can include on-demand spend outside the
    // included allowance, which would overstate usage toward the limit.
    let used_cents = p
        .included_spend
        .or_else(|| match (p.limit, p.remaining) {
            (Some(l), Some(r)) => Some((l - r).max(0.0)),
            _ => None,
        })
        .or(p.total_spend);
    let limit_cents = p.limit;
    let used_percent = match (used_cents, limit_cents) {
        (Some(u), Some(l)) if l > 0.0 => Some(u / l * 100.0),
        _ => None,
    }
    .or(p.total_percent_used);
    if used_cents.is_none() && limit_cents.is_none() && used_percent.is_none() {
        return vec![];
    }
    let resets_at = u.billing_cycle_end.as_ref().and_then(parse_epoch_seconds);
    vec![LimitWindow {
        label: "Plan usage".into(),
        used_percent: used_percent.map(|x| x as f32),
        resets_at,
        used: used_cents.map(|c| c / 100.0),
        limit: limit_cents.map(|c| c / 100.0),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_fixture_cents_to_dollars_with_consistent_percent_and_reset() {
        let u: CursorUsage =
            serde_json::from_str(include_str!("../../../tests/cursor_usage_fixture.json")).unwrap();
        let w = &normalize(&u)[0];
        assert_eq!(w.used, Some(232.22)); // 23222 cents -> $232.22
        assert_eq!(w.limit, Some(400.0)); // 40000 cents -> $400.00
        let expected_pct = (232.22 / 400.0 * 100.0) as f32;
        assert!((w.used_percent.unwrap() - expected_pct).abs() < 0.001);
        assert_eq!(w.resets_at, Some(1_771_077_734)); // billingCycleEnd ms -> s
    }

    #[test]
    fn normalize_falls_back_to_limit_minus_remaining() {
        // No includedSpend/totalSpend: used must be derived as limit − remaining,
        // matching parseDashboardPeriodUsage in cursor-usage-status.
        let u = CursorUsage {
            enabled: Some(true),
            billing_cycle_end: None,
            plan_usage: Some(PlanUsage {
                limit: Some(40000.0),
                remaining: Some(10000.0),
                ..Default::default()
            }),
        };
        let w = &normalize(&u)[0];
        assert_eq!(w.used, Some(300.0)); // (40000 − 10000) cents -> $300.00
        assert_eq!(w.limit, Some(400.0));
    }

    #[test]
    fn normalize_clamps_negative_remaining_to_zero() {
        // remaining > limit must not produce a negative `used`.
        let u = CursorUsage {
            enabled: Some(true),
            billing_cycle_end: None,
            plan_usage: Some(PlanUsage {
                limit: Some(10000.0),
                remaining: Some(30000.0),
                ..Default::default()
            }),
        };
        let w = &normalize(&u)[0];
        assert_eq!(w.used, Some(0.0));
    }

    #[test]
    fn normalize_empty_when_no_plan_usage() {
        assert!(normalize(&CursorUsage::default()).is_empty());
        assert!(normalize(&CursorUsage {
            enabled: Some(false),
            billing_cycle_end: None,
            plan_usage: None
        })
        .is_empty());
    }
}
