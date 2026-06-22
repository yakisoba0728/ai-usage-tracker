//! Gemini response structs + the `normalize()` core.
//!
//! The live `retrieveUserQuota` response returns one bucket per MODEL, not per
//! time-window. Real quota windows (future resetTime + remainingFraction) become
//! LimitWindows; tier-restricted models (epoch resetTime + remainingFraction 0)
//! surface as labeled notes in `detail_windows`.

use serde::Deserialize;

use crate::model::LimitWindow;

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
pub(super) struct QuotaResp {
    #[serde(default)]
    buckets: Vec<Bucket>,
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
pub(super) fn normalize(resp: &QuotaResp) -> (Vec<LimitWindow>, Vec<LimitWindow>) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_live_fixture_skips_restricted_and_picks_headline() {
        // Real response captured 2026-06-18 from a Free-tier account:
        // 5 available models all at 100% remaining + 3 Pro models locked
        // (epoch resetTime + remainingFraction 0).
        let q: QuotaResp =
            serde_json::from_str(include_str!("../../../tests/gemini_quota_fixture.json")).unwrap();
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
}
