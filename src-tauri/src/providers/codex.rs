//! Codex (ChatGPT) via Codex CLI. Reads `~/.codex/auth.json`, decodes the
//! `id_token` for subscription/renewal info, and calls the SAME endpoint the
//! official codex CLI polls: `chatgpt.com/backend-api/wham/usage` with
//! `Authorization: Bearer <access_token>`, `ChatGPT-Account-Id`, and a
//! `codex_cli_rs/` User-Agent. Surfaces the main 5h/weekly rate limits, every
//! additional rate limit (e.g. `GPT-5.3-Codex-Spark`), credits, and available
//! rate-limit resets. Token-parsing only.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::http;
use crate::jwt::jwt_payload;
use crate::model::{LimitWindow, Provider, ServiceUsage};
use crate::providers::ProviderError;
use crate::secrets;

/// The real endpoint, per openai/codex `backend-client` (PathStyle::ChatGptApi).
const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
/// `codex_cli_rs/` is the UA prefix Cloudflare allow-lists for the CLI.
const CODEX_UA: &str = "codex_cli_rs/0.1.0 (ai-usage-tracker)";

#[derive(Deserialize)]
struct AuthDotJson {
    #[serde(default)] tokens: Option<Tokens>,
}
#[derive(Deserialize)]
struct Tokens {
    access_token: String,
    #[serde(default)] id_token: Option<String>,
    #[serde(default)] account_id: Option<String>,
}

#[derive(Deserialize, Default)]
struct WhamUsage {
    #[serde(default)] plan_type: Option<String>,
    #[serde(default)] email: Option<String>,
    #[serde(default)] rate_limit: Option<RateLimit>,
    #[serde(default)] additional_rate_limits: Vec<AdditionalLimit>,
    #[serde(default)] credits: Option<Credits>,
    #[serde(default)] rate_limit_reset_credits: Option<ResetCredits>,
    #[serde(default)] code_review_rate_limit: Option<RateLimit>,
}
#[derive(Deserialize, Default)]
struct RateLimit {
    #[serde(default)] primary_window: Option<RateWindow>,
    #[serde(default)] secondary_window: Option<RateWindow>,
}
#[derive(Deserialize, Default)]
struct RateWindow {
    #[serde(default)] used_percent: Option<f64>,
    #[serde(default)] reset_at: Option<i64>, // epoch seconds
}
#[derive(Deserialize, Default)]
struct AdditionalLimit {
    #[serde(default)] limit_name: Option<String>,
    #[serde(default)] rate_limit: Option<RateLimit>,
}
#[derive(Deserialize, Default)]
struct Credits {
    #[serde(default)] balance: Option<String>,
    #[serde(default)] has_credits: Option<bool>,
    #[serde(default)] unlimited: Option<bool>,
}
#[derive(Deserialize, Default)]
struct ResetCredits {
    #[serde(default)] available_count: Option<i64>,
}

pub struct CodexProvider {
    http: reqwest::Client,
}
impl CodexProvider {
    pub fn new() -> Self {
        Self {
            http: http::build_client(),
        }
    }
}

fn read_tokens() -> Result<Tokens, ProviderError> {
    let v = secrets::read_json_file(&secrets::codex_auth_path())?;
    let a: AuthDotJson =
        serde_json::from_value(v).map_err(|e| ProviderError::Parse(format!("codex auth: {e}")))?;
    a.tokens
        .ok_or_else(|| ProviderError::NotLoggedIn("Codex not logged in (run `codex login`)".into()))
}

fn push_window(ws: &mut Vec<LimitWindow>, label: &str, w: &Option<RateWindow>) {
    if let Some(w) = w {
        ws.push(LimitWindow {
            label: label.into(),
            used_percent: w.used_percent.map(|x| x as f32),
            resets_at: w.reset_at,
            used: None,
            limit: None,
        });
    }
}

/// Pure: primary windows (5h/Weekly) + detail windows (Spark / code-review / credits / resets).
fn normalize(u: &WhamUsage) -> (Vec<LimitWindow>, Vec<LimitWindow>) {
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
        push_window(&mut detail, &format!("{name} · Weekly"), &rl.secondary_window);
    }
    if let Some(rl) = &u.code_review_rate_limit {
        push_window(&mut detail, "Code review · 5-hour", &rl.primary_window);
        push_window(&mut detail, "Code review · Weekly", &rl.secondary_window);
    }
    if let Some(c) = &u.credits {
        let bal = c.balance.as_deref().and_then(|b| b.parse::<f64>().ok());
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
                detail.push(LimitWindow {
                    label: "Credits balance".into(),
                    used_percent: None,
                    resets_at: None,
                    used: Some(v),
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

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Subscription renewal date (YYYY-MM-DD) from the id_token, if present.
fn renewal_date(id_token: &Option<String>) -> Option<String> {
    let claims = jwt_payload(id_token.as_deref()?).ok()?;
    let until = claims
        .get("https://api.openai.com/auth")
        .and_then(|a| a.get("chatgpt_subscription_active_until"))
        .and_then(|v| v.as_str())?;
    chrono::DateTime::parse_from_rfc3339(until)
        .ok()
        .map(|d| d.format("%Y-%m-%d").to_string())
}

#[async_trait]
impl crate::providers::ProviderApi for CodexProvider {
    fn key(&self) -> Provider {
        Provider::Codex
    }

    async fn fetch(&self) -> Result<ServiceUsage, ProviderError> {
        let t = read_tokens()?;
        fetch_with(&self.http, &t.access_token, t.account_id.as_deref(), &t.id_token, None).await
    }
}

/// Fetch Codex usage given an explicit token (used for manually-added accounts).
pub(crate) async fn fetch_with(
    http: &reqwest::Client,
    access_token: &str,
    account_id: Option<&str>,
    id_token: &Option<String>,
    label_override: Option<&str>,
) -> Result<ServiceUsage, ProviderError> {
    let mut extra: Vec<(&str, &str)> = vec![("User-Agent", CODEX_UA)];
    let acc_holder;
    if let Some(acc) = account_id {
        acc_holder = acc.to_string();
        extra.push(("ChatGPT-Account-Id", acc_holder.as_str()));
    }
    let raw: Value = http::get_json(http, access_token, USAGE_URL, &extra).await?;
    let u: WhamUsage =
        serde_json::from_value(raw).map_err(|e| ProviderError::Parse(format!("codex usage: {e}")))?;
    let plan = u.plan_type.as_deref().map(capitalize);
    let account = match label_override {
        Some(l) => Some(l.to_string()),
        None => match (u.email.as_ref(), renewal_date(id_token)) {
            (Some(email), Some(d)) => Some(format!("{email} · renews {d}")),
            (Some(email), None) => Some(email.clone()),
            (None, Some(d)) => Some(format!("renews {d}")),
            (None, None) => None,
        },
    };
    let (windows, detail_windows) = normalize(&u);
    Ok(ServiceUsage {
        provider: Provider::Codex,
        connected: true,
        plan,
        account,
        error: None,
        windows,
        detail_windows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_wham_fixture_includes_spark_and_credits() {
        let u: WhamUsage =
            serde_json::from_str(include_str!("../../tests/codex_wham_fixture.json")).unwrap();
        let (ws, detail) = normalize(&u);
        let labels: Vec<&str> = ws.iter().map(|w| w.label.as_str()).collect();
        assert_eq!(labels, vec!["5-hour", "Weekly"]); // primary only
        let dlabels: Vec<&str> = detail.iter().map(|w| w.label.as_str()).collect();
        assert!(dlabels.iter().any(|l| l.contains("Spark") && l.contains("5-hour")));
        assert!(dlabels.contains(&"Credits balance"));
        assert!(dlabels.contains(&"Available rate-limit resets"));
        let five = ws.iter().find(|w| w.label == "5-hour").unwrap();
        assert_eq!(five.used_percent, Some(1.0));
    }

    #[test]
    fn reads_tokens_from_fixture() {
        let v: Value =
            serde_json::from_str(include_str!("../../tests/codex_auth_fixture.json")).unwrap();
        let a: AuthDotJson = serde_json::from_value(v).unwrap();
        assert!(a.tokens.is_some());
    }
}
