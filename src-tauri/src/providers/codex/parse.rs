//! Codex (`/wham/usage`) response structs + the `normalize()` core. Surfaces the
//! main 5h/weekly rate limits, every additional rate limit (e.g.
//! `GPT-5.3-Codex-Spark`), credits, and available rate-limit resets.

use serde::Deserialize;
use serde_json::Value;

use crate::model::LimitWindow;

#[derive(Deserialize, Default)]
pub(super) struct WhamUsage {
    #[serde(default)]
    pub(super) plan_type: Option<String>,
    #[serde(default)]
    pub(super) email: Option<String>,
    #[serde(default)]
    rate_limit: Option<RateLimit>,
    #[serde(default)]
    additional_rate_limits: Vec<AdditionalLimit>,
    #[serde(default)]
    credits: Option<Credits>,
    #[serde(default)]
    rate_limit_reset_credits: Option<ResetCredits>,
    #[serde(default)]
    code_review_rate_limit: Option<RateLimit>,
}
#[derive(Deserialize, Default)]
struct RateLimit {
    #[serde(default)]
    primary_window: Option<RateWindow>,
    #[serde(default)]
    secondary_window: Option<RateWindow>,
}
#[derive(Deserialize, Default)]
struct RateWindow {
    #[serde(default)]
    used_percent: Option<f64>,
    #[serde(default)]
    reset_at: Option<i64>, // epoch seconds
}
#[derive(Deserialize, Default)]
struct AdditionalLimit {
    #[serde(default)]
    limit_name: Option<String>,
    #[serde(default)]
    rate_limit: Option<RateLimit>,
}
#[derive(Deserialize, Default)]
struct Credits {
    #[serde(default)]
    balance: Option<Value>,
    #[serde(default)]
    has_credits: Option<bool>,
    #[serde(default)]
    unlimited: Option<bool>,
}
#[derive(Deserialize, Default)]
struct ResetCredits {
    #[serde(default)]
    available_count: Option<i64>,
}

fn push_window(ws: &mut Vec<LimitWindow>, label: &str, w: &Option<RateWindow>) {
    if let Some(w) = w {
        if w.used_percent.is_none() && w.reset_at.is_none() {
            return;
        }
        ws.push(LimitWindow {
            label: label.into(),
            used_percent: w.used_percent.map(|x| x as f32),
            resets_at: w.reset_at,
            used: None,
            limit: None,
        });
    }
}

fn parse_number(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str()?.trim().parse::<f64>().ok())
}

/// Pure: primary windows (5h/Weekly) + detail windows (Spark / code-review / credits / resets).
pub(super) fn normalize(u: &WhamUsage) -> (Vec<LimitWindow>, Vec<LimitWindow>) {
    let mut ws = Vec::new();
    let mut detail = Vec::new();
    if let Some(rl) = &u.rate_limit {
        push_window(&mut ws, "5-hour", &rl.primary_window);
        push_window(&mut ws, "Weekly", &rl.secondary_window);
    }
    for al in &u.additional_rate_limits {
        let Some(rl) = &al.rate_limit else { continue };
        let name = al.limit_name.clone().unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        push_window(&mut detail, &format!("{name} · 5-hour"), &rl.primary_window);
        push_window(
            &mut detail,
            &format!("{name} · Weekly"),
            &rl.secondary_window,
        );
    }
    if let Some(rl) = &u.code_review_rate_limit {
        push_window(&mut detail, "Code review · 5-hour", &rl.primary_window);
        push_window(&mut detail, "Code review · Weekly", &rl.secondary_window);
    }
    if let Some(c) = &u.credits {
        let bal = c.balance.as_ref().and_then(parse_number);
        if c.unlimited == Some(true) {
            detail.push(LimitWindow {
                label: "Credits (unlimited)".into(),
                used_percent: None,
                resets_at: None,
                used: None,
                limit: None,
            });
        } else if c.has_credits == Some(true) || matches!(bal, Some(v) if v > 0.0) {
            if let Some(v) = bal {
                // This is the *remaining* balance; keep it in the label rather
                // than `used` (which means consumption everywhere else) so the
                // semantics aren't inverted if the UI ever derives a percent or
                // `remaining = limit - used` (B-15).
                detail.push(LimitWindow {
                    label: format!("Credits balance: ${v:.2}"),
                    used_percent: None,
                    resets_at: None,
                    used: None,
                    limit: None,
                });
            }
        }
    }
    if let Some(r) = &u.rate_limit_reset_credits {
        if let Some(n) = r.available_count {
            detail.push(LimitWindow {
                label: "Available rate-limit resets".into(),
                used_percent: None,
                resets_at: None,
                used: Some(n as f64),
                limit: None,
            });
        }
    }
    (ws, detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_wham_fixture_includes_spark_and_credits() {
        let u: WhamUsage =
            serde_json::from_str(include_str!("../../../tests/codex_wham_fixture.json")).unwrap();
        let (ws, detail) = normalize(&u);
        let labels: Vec<&str> = ws.iter().map(|w| w.label.as_str()).collect();
        assert_eq!(labels, vec!["5-hour", "Weekly"]); // primary only
        let dlabels: Vec<&str> = detail.iter().map(|w| w.label.as_str()).collect();
        assert!(dlabels
            .iter()
            .any(|l| l.contains("Spark") && l.contains("5-hour")));
        // The *remaining* credits balance lives in the label, not the `used`
        // field (which everywhere else means consumption) — B-15.
        let credits = detail
            .iter()
            .find(|w| w.label.starts_with("Credits balance"))
            .unwrap();
        assert_eq!(credits.label, "Credits balance: $9.99");
        assert_eq!(credits.used, None);
        assert!(dlabels.contains(&"Available rate-limit resets"));
        let five = ws.iter().find(|w| w.label == "5-hour").unwrap();
        assert_eq!(five.used_percent, Some(1.0));
    }

    #[test]
    fn normalize_accepts_numeric_credit_balance() {
        let u: WhamUsage = serde_json::from_value(serde_json::json!({
            "credits": {
                "has_credits": true,
                "unlimited": false,
                "balance": 9.99
            }
        }))
        .unwrap();
        let (_ws, detail) = normalize(&u);
        let credits = detail
            .iter()
            .find(|w| w.label.starts_with("Credits balance"))
            .unwrap();
        assert_eq!(credits.label, "Credits balance: $9.99");
        assert_eq!(credits.used, None);
    }

    #[test]
    fn normalize_skips_blank_rate_limit_windows() {
        let u = WhamUsage {
            rate_limit: Some(RateLimit {
                primary_window: Some(RateWindow::default()),
                secondary_window: None,
            }),
            ..Default::default()
        };
        let (ws, detail) = normalize(&u);
        assert!(ws.is_empty());
        assert!(detail.is_empty());
    }
}
