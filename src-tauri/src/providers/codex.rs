//! Codex (via Codex CLI). Reads `~/.codex/auth.json`, decodes the `id_token` for
//! plan/account display, and attempts `chatgpt.com/backend-api/codex/usage` with
//! legitimate headers. That endpoint sits behind bot-management; we do NOT
//! impersonate browsers/TLS to defeat it — if the WAF blocks the request we
//! still report the account/plan (from the local token) with an honest note
//! rather than fabricating numbers.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::http;
use crate::jwt::jwt_payload;
use crate::model::{LimitWindow, Provider, ServiceUsage};
use crate::providers::ProviderError;
use crate::secrets;

const USAGE_URL: &str = "https://chatgpt.com/backend-api/codex/usage";

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

/// Permissive: the Codex usage shape is undocumented; capture common keys.
#[derive(Deserialize, Default)]
struct CodexUsage {
    #[serde(default)] remaining_credits: Option<f64>,
    #[serde(default)] used_credits: Option<f64>,
    #[serde(default)] reset_at: Option<chrono::DateTime<chrono::Utc>>,
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
    let a: AuthDotJson = serde_json::from_value(v)
        .map_err(|e| ProviderError::Parse(format!("codex auth: {e}")))?;
    a.tokens.ok_or_else(|| {
        ProviderError::NotLoggedIn("Codex not logged in (run `codex login`)".into())
    })
}

/// Pure: build a single credits window from a defensively-parsed usage blob.
pub fn normalize(u: &CodexUsage) -> Vec<LimitWindow> {
    let limit = match (u.used_credits, u.remaining_credits) {
        (Some(used), Some(rem)) => Some(used + rem),
        _ => None,
    };
    let used_percent = match (u.used_credits, limit) {
        (Some(used), Some(limit)) if limit > 0.0 => Some((used / limit * 100.0) as f32),
        _ => None,
    };
    if u.used_credits.is_none() && u.remaining_credits.is_none() && u.reset_at.is_none() {
        return vec![];
    }
    vec![LimitWindow {
        label: "Codex credits".into(),
        used_percent,
        resets_at: u.reset_at.map(|d| d.timestamp()),
        used: u.used_credits,
        limit,
    }]
}

/// Extract plan + email from the id_token JWT payload (metadata only).
fn id_token_identity(id_token: &Option<String>) -> (Option<String>, Option<String>) {
    let Some(tok) = id_token else {
        return (None, None);
    };
    let Ok(claims) = jwt_payload(tok) else {
        return (None, None);
    };
    let plan = claims
        .get("https://api.openai.com/auth")
        .and_then(|a| a.get("plan"))
        .and_then(|p| p.as_str())
        .or_else(|| claims.get("plan").and_then(|p| p.as_str()))
        .map(String::from);
    let email = claims
        .get("email")
        .and_then(|e| e.as_str())
        .map(String::from);
    (plan, email)
}

#[async_trait]
impl crate::providers::ProviderApi for CodexProvider {
    fn key(&self) -> Provider {
        Provider::Codex
    }

    async fn fetch(&self) -> Result<ServiceUsage, ProviderError> {
        let t = read_tokens()?;
        let (plan, email) = id_token_identity(&t.id_token);

        // Attempt the usage endpoint with legitimate headers. Cloudflare
        // bot-management may 403 this; we degrade honestly rather than bypass.
        let extra = match &t.account_id {
            Some(acc) => vec![
                ("User-Agent", "codex_cli_rs/0.1.0"),
                ("chatgpt-account-id", acc.as_str()),
            ],
            None => vec![("User-Agent", "codex_cli_rs/0.1.0")],
        };
        let result: Result<Value, ProviderError> =
            http::get_json(&self.http, &t.access_token, USAGE_URL, &extra).await;

        match result {
            Ok(raw) => {
                let inner = raw.get("usage").cloned().unwrap_or(raw);
                let u: CodexUsage = serde_json::from_value(inner)
                    .map_err(|e| ProviderError::Parse(format!("codex usage: {e}")))?;
                Ok(ServiceUsage {
                    provider: Provider::Codex,
                    connected: true,
                    plan,
                    account: email,
                    error: None,
                    windows: normalize(&u),
                })
            }
            Err(e) => {
                // Logged in (token readable) but usage fetch blocked/unavailable.
                Ok(ServiceUsage {
                    provider: Provider::Codex,
                    connected: true,
                    plan,
                    account: email,
                    error: Some(format!(
                        "usage endpoint unavailable ({e}); open ChatGPT → Codex settings"
                    )),
                    windows: vec![],
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_tokens_and_identity_from_fixture() {
        let v: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/codex_auth_fixture.json")).unwrap();
        let a: AuthDotJson = serde_json::from_value(v).unwrap();
        let (plan, email) = id_token_identity(&a.tokens.unwrap().id_token);
        assert_eq!(email.as_deref(), Some("me@ex.com"));
        assert_eq!(plan.as_deref(), Some("plus"));
    }

    #[test]
    fn normalize_computes_percent_and_limit() {
        let u = serde_json::from_str::<CodexUsage>(include_str!("../../tests/codex_usage_fixture.json"))
            .unwrap();
        let w = &normalize(&u)[0];
        assert_eq!(w.used_percent, Some(30.0));
        assert_eq!(w.limit, Some(100.0));
        assert_eq!(w.used, Some(30.0));
    }

    #[test]
    fn normalize_empty_when_no_data() {
        let u = CodexUsage::default();
        assert!(normalize(&u).is_empty());
    }
}
