//! Codex (ChatGPT) via Codex CLI. Reads `~/.codex/auth.json`, decodes the
//! `id_token` for plan/email display, and calls the SAME endpoint the official
//! codex CLI polls: `chatgpt.com/backend-api/wham/usage` with `Authorization:
//! Bearer <access_token>`, `ChatGPT-Account-Id`, and a `codex_cli_rs/…`
//! User-Agent (the prefix Cloudflare allow-lists for the CLI). Response gives
//! `rate_limit.primary_window` (5h) + `secondary_window` (weekly) `used_percent`.
//! Token-parsing only — no refresh.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::http;
use crate::jwt::jwt_payload;
use crate::model::{LimitWindow, Provider, ServiceUsage};
use crate::providers::ProviderError;
use crate::secrets;

/// The real endpoint, per openai/codex `backend-client` (`rate_limit_status_url`
/// with PathStyle::ChatGptApi). NOT `/codex/usage`.
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
    #[serde(default)] credits: Option<Credits>,
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
struct Credits {
    #[serde(default)] balance: Option<String>,
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
    a.tokens.ok_or_else(|| {
        ProviderError::NotLoggedIn("Codex not logged in (run `codex login`)".into())
    })
}

/// Pure: rate_limit windows → LimitWindows (5-hour + weekly), plus credits.
fn normalize(u: &WhamUsage) -> Vec<LimitWindow> {
    let mut ws = Vec::new();
    if let Some(rl) = &u.rate_limit {
        if let Some(w) = &rl.primary_window {
            ws.push(LimitWindow {
                label: "5-hour".into(),
                used_percent: w.used_percent.map(|x| x as f32),
                resets_at: w.reset_at,
                used: None,
                limit: None,
            });
        }
        if let Some(w) = &rl.secondary_window {
            ws.push(LimitWindow {
                label: "Weekly".into(),
                used_percent: w.used_percent.map(|x| x as f32),
                resets_at: w.reset_at,
                used: None,
                limit: None,
            });
        }
    }
    if let Some(c) = &u.credits {
        if let Some(bal) = &c.balance {
            if let Ok(b) = bal.parse::<f64>() {
                ws.push(LimitWindow {
                    label: "Credits balance".into(),
                    used_percent: None,
                    resets_at: None,
                    used: Some(b),
                    limit: None,
                });
            }
        }
    }
    ws
}

/// plan/email from the response (preferred) else the id_token JWT claims.
fn resolve_plan_email(u: &WhamUsage, id_token: &Option<String>) -> (Option<String>, Option<String>) {
    let plan = u.plan_type.clone().or_else(|| {
        id_token
            .as_deref()
            .and_then(|t| jwt_payload(t).ok())
            .and_then(|c| {
                c.get("https://api.openai.com/auth")
                    .and_then(|a| a.get("plan"))
                    .and_then(|p| p.as_str())
                    .map(String::from)
            })
    });
    let email = u.email.clone().or_else(|| {
        id_token
            .as_deref()
            .and_then(|t| jwt_payload(t).ok())
            .and_then(|c| c.get("email").and_then(|e| e.as_str()).map(String::from))
    });
    (plan, email)
}

#[async_trait]
impl crate::providers::ProviderApi for CodexProvider {
    fn key(&self) -> Provider {
        Provider::Codex
    }

    async fn fetch(&self) -> Result<ServiceUsage, ProviderError> {
        let t = read_tokens()?;
        let mut extra: Vec<(&str, &str)> = vec![("User-Agent", CODEX_UA)];
        let acc_holder;
        if let Some(acc) = &t.account_id {
            acc_holder = acc.clone();
            extra.push(("ChatGPT-Account-Id", acc_holder.as_str()));
        }
        let raw: Value = http::get_json(&self.http, &t.access_token, USAGE_URL, &extra).await?;
        let u: WhamUsage =
            serde_json::from_value(raw).map_err(|e| ProviderError::Parse(format!("codex usage: {e}")))?;
        let (plan, email) = resolve_plan_email(&u, &t.id_token);
        Ok(ServiceUsage {
            provider: Provider::Codex,
            connected: true,
            plan,
            account: email,
            error: None,
            windows: normalize(&u),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_wham_fixture() {
        let u: WhamUsage =
            serde_json::from_str(include_str!("../../tests/codex_wham_fixture.json")).unwrap();
        let ws = normalize(&u);
        let five = ws.iter().find(|w| w.label == "5-hour").unwrap();
        assert_eq!(five.used_percent, Some(1.0));
        assert_eq!(five.resets_at, Some(1781842264));
        let weekly = ws.iter().find(|w| w.label == "Weekly").unwrap();
        assert_eq!(weekly.used_percent, Some(1.0));
        let credits = ws.iter().find(|w| w.label == "Credits balance").unwrap();
        assert_eq!(credits.used, Some(9.99));
    }

    #[test]
    fn reads_tokens_from_fixture() {
        let v: Value =
            serde_json::from_str(include_str!("../../tests/codex_auth_fixture.json")).unwrap();
        let a: AuthDotJson = serde_json::from_value(v).unwrap();
        assert!(a.tokens.is_some());
    }
}
