//! OAuth token refresh using each CLI's PUBLIC client_id (we never register our
//! own OAuth app — these are the published client IDs the official CLIs use).
//! Source-verified values: see docs/oauth-credential-research.md.

use serde_json::Value;

use crate::providers::ProviderError;

pub const CLAUDE_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
pub const CLAUDE_TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";

pub const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const CODEX_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";

#[derive(Debug, Clone)]
pub struct RefreshedTokens {
    pub access_token: String,
    pub refresh_token: String, // rotated for Claude/Codex (single-use)
    pub expires_in: Option<u64>, // seconds
}

/// Parse an OAuth token response (tolerates snake_case / camelCase).
fn parse_tokens(v: &Value) -> Result<RefreshedTokens, ProviderError> {
    let at = v
        .get("access_token")
        .or_else(|| v.get("accessToken"))
        .and_then(|x| x.as_str());
    let rt = v
        .get("refresh_token")
        .or_else(|| v.get("refreshToken"))
        .and_then(|x| x.as_str());
    let exp = v.get("expires_in").and_then(|x| x.as_u64());
    match (at, rt) {
        (Some(a), Some(r)) => Ok(RefreshedTokens {
            access_token: a.into(),
            refresh_token: r.into(),
            expires_in: exp,
        }),
        _ => {
            // Surface the provider's error object/message if present.
            let msg = v
                .get("error")
                .map(|e| {
                    if let Some(s) = e.as_str() {
                        s.to_string()
                    } else {
                        e.get("message")
                            .and_then(|m| m.as_str())
                            .or_else(|| e.get("type").and_then(|t| t.as_str()))
                            .unwrap_or("unknown error")
                            .to_string()
                    }
                })
                .unwrap_or_else(|| format!("no access_token in refresh response"));
            Err(ProviderError::Network(format!("refresh failed: {msg}")))
        }
    }
}

async fn read_and_parse(resp: reqwest::Response, url: &str) -> Result<RefreshedTokens, ProviderError> {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    let v: Value = serde_json::from_str(&body).map_err(|e| {
        ProviderError::Network(format!("{url} ({status}): parse {e}; body {}", &body[..body.len().min(200)]))
    })?;
    if status.is_success() {
        parse_tokens(&v)
    } else {
        // Still try to parse the error body for a useful message.
        parse_tokens(&v)
            .map_err(|_| ProviderError::Network(format!("{url} ({status}): {}", &body[..body.len().min(200)])))
    }
}

/// Claude: form-encoded refresh.
pub async fn refresh_form(
    client: &reqwest::Client,
    url: &str,
    form: &[(&str, &str)],
) -> Result<RefreshedTokens, ProviderError> {
    let resp = client
        .post(url)
        .form(form)
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    read_and_parse(resp, url).await
}

/// Codex: JSON-body refresh.
pub async fn refresh_json(
    client: &reqwest::Client,
    url: &str,
    body: Value,
) -> Result<RefreshedTokens, ProviderError> {
    let resp = client
        .post(url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    read_and_parse(resp, url).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_snake_case_success() {
        let v = serde_json::json!({"access_token":"a","refresh_token":"r","expires_in":3600});
        let t = parse_tokens(&v).unwrap();
        assert_eq!(t.access_token, "a");
        assert_eq!(t.expires_in, Some(3600));
    }

    #[test]
    fn parses_camel_case_success() {
        let v = serde_json::json!({"accessToken":"a","refreshToken":"r"});
        assert!(parse_tokens(&v).is_ok());
    }

    #[test]
    fn surfaces_error_message() {
        let v = serde_json::json!({"error":{"type":"rate_limit_error","message":"Rate limited. Please try again later."}});
        let e = parse_tokens(&v).unwrap_err();
        assert!(e.to_string().contains("Rate limited"));
    }
}
