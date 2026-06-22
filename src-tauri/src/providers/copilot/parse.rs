//! GitHub Copilot response structs + the `normalize()` core. The
//! `/copilot_internal/user` endpoint returns the `quota_snapshots` gauge (chat /
//! premium_interactions / completions) plus `quota_reset_date`.

use serde::Deserialize;

use crate::model::LimitWindow;

#[derive(Deserialize, Default)]
pub(super) struct CopilotUsageResp {
    #[serde(default)]
    pub(super) login: Option<String>,
    #[serde(default)]
    pub(super) copilot_plan: Option<String>,
    #[serde(default)]
    pub(super) quota_reset_date: Option<String>,
    #[serde(default)]
    pub(super) quota_reset_date_utc: Option<String>,
    #[serde(default)]
    pub(super) quota_snapshots: QuotaSnapshots,
}

#[derive(Deserialize, Default)]
pub(super) struct QuotaSnapshots {
    #[serde(default)]
    pub(super) chat: Option<QuotaSnapshot>,
    #[serde(default)]
    pub(super) premium_interactions: Option<QuotaSnapshot>,
    #[serde(default)]
    pub(super) completions: Option<QuotaSnapshot>,
}

#[derive(Deserialize, Default)]
pub(super) struct QuotaSnapshot {
    #[serde(default)]
    pub(super) entitlement: Option<f64>,
    /// Canonical field per `opencode-mystatus` (`used = entitlement - remaining`).
    #[serde(default)]
    pub(super) remaining: Option<f64>,
    /// Older payloads carry this alias; we accept either.
    #[serde(default)]
    pub(super) quota_remaining: Option<f64>,
    #[serde(default)]
    pub(super) percent_remaining: Option<f64>,
    #[serde(default)]
    pub(super) unlimited: Option<bool>,
}

/// Parse the `quota_reset_date` field. The real API returns a plain `YYYY-MM-DD`
/// date (e.g. `"2026-07-01"`); tolerate full RFC-3339 too.
fn parse_reset_date(s: &str) -> Option<i64> {
    if let Ok(d) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(d.timestamp());
    }
    chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0).map(|ndt| ndt.and_utc().timestamp()))
}

/// Pure: quota_snapshots → (headline windows, detail notes).
///
/// Metered categories (unlimited=false) become real usage windows sorted by
/// highest burn; the first one is the card headline. Unlimited categories
/// (unlimited=true — typically `chat` and `completions` on individual plans)
/// become labeled notes in `detail_windows` so the user can see what their
/// plan includes without polluting the usage math.
pub(super) fn normalize(resp: &CopilotUsageResp) -> (Vec<LimitWindow>, Vec<LimitWindow>) {
    let reset = resp
        .quota_reset_date
        .as_deref()
        .or(resp.quota_reset_date_utc.as_deref())
        .and_then(parse_reset_date);
    let categories: [(&str, &Option<QuotaSnapshot>); 3] = [
        ("Chat", &resp.quota_snapshots.chat),
        (
            "Premium requests",
            &resp.quota_snapshots.premium_interactions,
        ),
        ("Completions", &resp.quota_snapshots.completions),
    ];
    let mut windows: Vec<LimitWindow> = Vec::new();
    let mut detail: Vec<LimitWindow> = Vec::new();
    for (label, q) in categories {
        let Some(q) = q.as_ref() else { continue };
        if q.unlimited.unwrap_or(false) {
            // Unlimited category — surface as a labeled note in the modal.
            detail.push(LimitWindow {
                label: format!("{label} (unlimited)"),
                used_percent: None,
                resets_at: reset,
                used: None,
                limit: None,
            });
            continue;
        }
        let pct = q
            .percent_remaining
            .map(|r| (100.0 - r).clamp(0.0, 100.0) as f32);
        // Reference uses `remaining`; older payloads carry `quota_remaining`.
        let remaining = q.remaining.or(q.quota_remaining);
        // Nothing displayable → don't emit a blank window that could sort to the
        // card headline as a zero bar (B-16; mirrors cursor::normalize's guard).
        if pct.is_none() && q.entitlement.is_none() && remaining.is_none() {
            continue;
        }
        let used = match (q.entitlement, remaining) {
            (Some(e), Some(r)) if e >= 0.0 => Some((e - r).clamp(0.0, e)),
            (Some(_), Some(r)) => Some((-r).max(0.0)),
            _ => None,
        };
        windows.push(LimitWindow {
            label: (*label).into(),
            used_percent: pct,
            resets_at: reset,
            used,
            limit: q.entitlement,
        });
    }
    windows.sort_by(|a, b| {
        b.used_percent
            .unwrap_or(0.0)
            .partial_cmp(&a.used_percent.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    (windows, detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn normalize_live_fixture_shows_metered_and_unlimited_notes() {
        // Real response captured 2026-06-19 from an individual-plan account:
        // chat + completions are unlimited; premium_interactions is metered
        // (200 of 200 remaining → 0% used).
        let raw = include_str!("../../../tests/copilot_internal_fixture.json");
        let v: Value = serde_json::from_str(raw).unwrap();
        let u: CopilotUsageResp = serde_json::from_value(v).unwrap();
        assert_eq!(u.login.as_deref(), Some("yakisoba0728"));
        assert_eq!(u.copilot_plan.as_deref(), Some("individual"));
        assert_eq!(u.quota_reset_date.as_deref(), Some("2026-07-01"));

        let (ws, detail) = normalize(&u);
        // Only the metered category surfaces as a usage window.
        assert_eq!(ws.len(), 1);
        assert_eq!(ws[0].label, "Premium requests");
        assert_eq!(ws[0].used_percent, Some(0.0)); // percent_remaining=100 → 0% used
        assert_eq!(ws[0].used, Some(0.0)); // entitlement(200) - remaining(200)
        assert_eq!(ws[0].limit, Some(200.0));
        assert!(ws[0].resets_at.is_some()); // 2026-07-01 parsed

        // Unlimited categories appear as labeled notes in detail (no percent).
        assert_eq!(detail.len(), 2);
        assert!(detail.iter().any(|w| w.label == "Chat (unlimited)"));
        assert!(detail.iter().any(|w| w.label == "Completions (unlimited)"));
        assert!(detail.iter().all(|w| w.used_percent.is_none()));
    }

    #[test]
    fn normalize_sorts_metered_categories_by_usage_desc() {
        // Synthetic: two metered categories, one more consumed than the other.
        let resp = CopilotUsageResp {
            login: None,
            copilot_plan: Some("pro".into()),
            quota_reset_date: Some("2026-07-01T00:00:00Z".into()),
            quota_reset_date_utc: None,
            quota_snapshots: QuotaSnapshots {
                chat: Some(QuotaSnapshot {
                    entitlement: Some(300.0),
                    remaining: Some(250.0),
                    quota_remaining: Some(250.0),
                    percent_remaining: Some(83.33),
                    unlimited: Some(false),
                }),
                premium_interactions: Some(QuotaSnapshot {
                    entitlement: Some(300.0),
                    remaining: Some(50.0),
                    quota_remaining: Some(50.0),
                    percent_remaining: Some(16.67),
                    unlimited: Some(false),
                }),
                completions: None,
            },
        };
        let (ws, detail) = normalize(&resp);
        // Both metered categories in windows; none unlimited → detail empty.
        assert_eq!(ws.len(), 2);
        assert!(detail.is_empty());
        // Higher usage (lower percent_remaining) first.
        assert_eq!(ws[0].label, "Premium requests");
        assert_eq!(ws[0].used_percent, Some(83.33));
        assert_eq!(ws[0].used, Some(250.0));
        assert_eq!(ws[0].limit, Some(300.0));
        // Both windows share the parsed reset date.
        assert!(ws[0].resets_at.is_some());
        assert!(ws[1].resets_at.is_some());
    }

    #[test]
    fn normalize_skips_metered_category_with_no_displayable_fields() {
        // A metered (unlimited absent → false) category with no
        // entitlement/remaining/percent must NOT emit a blank zero-bar window
        // that could sort to the card headline (B-16).
        let resp = CopilotUsageResp {
            login: None,
            copilot_plan: None,
            quota_reset_date: None,
            quota_reset_date_utc: None,
            quota_snapshots: QuotaSnapshots {
                chat: Some(QuotaSnapshot {
                    entitlement: None,
                    remaining: None,
                    quota_remaining: None,
                    percent_remaining: None,
                    unlimited: None,
                }),
                premium_interactions: None,
                completions: None,
            },
        };
        let (ws, detail) = normalize(&resp);
        assert!(ws.is_empty(), "no displayable fields → no window");
        assert!(detail.is_empty());
    }

    #[test]
    fn normalize_uses_quota_reset_date_utc_fallback() {
        let v = serde_json::json!({
            "quota_reset_date_utc": "2026-07-01T00:00:00.000Z",
            "quota_snapshots": {
                "premium_interactions": {
                    "entitlement": 200,
                    "remaining": 100,
                    "percent_remaining": 50,
                    "unlimited": false
                }
            }
        });
        let resp: CopilotUsageResp = serde_json::from_value(v).unwrap();
        let (ws, _detail) = normalize(&resp);
        assert_eq!(ws.len(), 1);
        assert_eq!(ws[0].resets_at, Some(1_782_864_000));
    }

    #[test]
    fn normalize_clamps_metered_used_and_percent() {
        let resp = CopilotUsageResp {
            login: None,
            copilot_plan: None,
            quota_reset_date: None,
            quota_reset_date_utc: None,
            quota_snapshots: QuotaSnapshots {
                chat: Some(QuotaSnapshot {
                    entitlement: Some(200.0),
                    remaining: Some(250.0),
                    quota_remaining: None,
                    percent_remaining: Some(150.0),
                    unlimited: Some(false),
                }),
                premium_interactions: Some(QuotaSnapshot {
                    entitlement: Some(200.0),
                    remaining: Some(-10.0),
                    quota_remaining: None,
                    percent_remaining: Some(-50.0),
                    unlimited: Some(false),
                }),
                completions: None,
            },
        };
        let (ws, detail) = normalize(&resp);
        assert!(detail.is_empty());

        let premium = ws.iter().find(|w| w.label == "Premium requests").unwrap();
        assert_eq!(premium.used_percent, Some(100.0));
        assert_eq!(premium.used, Some(200.0));

        let chat = ws.iter().find(|w| w.label == "Chat").unwrap();
        assert_eq!(chat.used_percent, Some(0.0));
        assert_eq!(chat.used, Some(0.0));
    }

    #[test]
    fn parse_reset_date_accepts_plain_and_rfc3339() {
        // Real API format: plain YYYY-MM-DD.
        assert!(parse_reset_date("2026-07-01").is_some());
        // Tolerate full RFC-3339 too.
        assert!(parse_reset_date("2026-07-01T00:00:00Z").is_some());
        assert!(parse_reset_date("nonsense").is_none());
    }
}
