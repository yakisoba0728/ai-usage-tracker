//! The authorization-code → token exchange: a form-encoded POST to the
//! provider's token endpoint, with an optional `client_secret` appended for
//! installed-app clients (Google) that require it even with PKCE.

use serde_json::Value;

pub(crate) async fn exchange(
    token_url: &str,
    client_id: &str,
    client_secret: Option<&str>,
    redirect_uri: &str,
    verifier: &str,
    code: &str,
    _state: Option<&str>,
) -> Result<Value, String> {
    let client = crate::http::shared();
    // Optional client_secret appended after the standard PKCE body (Google
    // installed-app clients require it even with PKCE).
    let mut body = format!(
        "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
        urlencoding::encode(code),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(client_id),
        urlencoding::encode(verifier),
    );
    if let Some(secret) = client_secret {
        body.push_str(&format!("&client_secret={}", urlencoding::encode(secret)));
    }
    let resp = client
        .post(token_url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "{token_url} ({status}): {}",
            crate::util::scrub_sensitive_text(&text)
                .chars()
                .take(200)
                .collect::<String>()
        ));
    }
    serde_json::from_str::<Value>(&text).map_err(|e| format!("parse: {e}"))
}
