//! Browser + localhost-callback OAuth login (the codex-switcher pattern). We
//! spin up a tiny local HTTP server, build the provider's authorize URL with
//! PKCE + a localhost redirect, open it in the user's browser, capture the
//! authorization code on the callback, and exchange it for tokens. The browser
//! handles the provider's login (incl. any Cloudflare), so we never have to.
//! Works for Codex (proven) and Claude; Gemini/Copilot keep the device-code path.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

use base64::Engine;
use rand::Rng;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};
use tiny_http::{Response, Server};

use crate::jwt::jwt_payload;
use crate::model::Provider;
use crate::store::{self, StoredCredential};

const REDIRECT_PATH: &str = "/auth/callback";
const TIMEOUT_SECS: u64 = 300;

/// Per-provider OAuth config (public client_ids reused from the official CLIs).
struct OAuthSpec {
    authorize_url: String,
    token_url: String,
    client_id: String,
    scope: String,
    extra_params: Vec<(&'static str, &'static str)>,
}

fn spec_for(p: Provider) -> Option<OAuthSpec> {
    match p {
        Provider::Codex => Some(OAuthSpec {
            authorize_url: "https://auth.openai.com/oauth/authorize".into(),
            token_url: "https://auth.openai.com/oauth/token".into(),
            client_id: "app_EMoamEEZ73f0CkXaXp7hrann".into(),
            scope: "openid profile email offline_access".into(),
            // Required by OpenAI OAuth (per codex-switcher / codex CLI).
            extra_params: vec![
                ("originator", "codex_cli_rs"),
                ("codex_cli_simplified_flow", "true"),
                ("id_token_add_organizations", "true"),
            ],
        }),
        Provider::Claude => Some(OAuthSpec {
            authorize_url: "https://claude.com/cai/oauth/authorize".into(),
            token_url: "https://console.anthropic.com/v1/oauth/token".into(),
            client_id: "9d1c250a-e61b-44d9-88ed-5944d1962f5e".into(),
            scope:
                "org:create_api_key user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload"
                    .into(),
            extra_params: vec![("code", "true")],
        }),
        _ => None,
    }
}

pub fn supports(p: Provider) -> bool {
    spec_for(p).is_some()
}

#[derive(Clone, Serialize)]
struct LoginResult {
    provider: Provider,
    ok: bool,
    label: Option<String>,
    error: Option<String>,
}

/// Global cancel flag for the currently-active OAuth login.
static ACTIVE: LazyLock<Mutex<Option<std::sync::Arc<AtomicBool>>>> =
    LazyLock::new(|| Mutex::new(None));

pub fn cancel() {
    if let Ok(guard) = ACTIVE.lock() {
        if let Some(flag) = guard.as_ref() {
            flag.store(true, Ordering::SeqCst);
        }
    }
}

/// Start the OAuth login. Returns the authorize URL (for the frontend to open)
/// and runs the callback server in the background; emits `login-complete` when
/// done.
pub fn start(app: AppHandle, provider: Provider) -> Result<String, String> {
    let spec = spec_for(provider).ok_or_else(|| "OAuth login not supported for this provider".to_string())?;

    let verifier = pkce_verifier();
    let challenge = pkce_challenge(&verifier);
    let state = random_b64(32);

    // Bind a local callback server (prefer 1455 like the official CLIs; fall
    // back to an OS-assigned port).
    let server = Server::http("127.0.0.1:1455")
        .or_else(|_| Server::http("127.0.0.1:0"))
        .map_err(|e| format!("start local server: {e}"))?;
    let port = server
        .server_addr()
        .to_ip()
        .map(|a| a.port())
        .unwrap_or(0);
    let redirect_uri = format!("http://localhost:{port}{REDIRECT_PATH}");

    let auth_url = build_authorize_url(&spec, &redirect_uri, &challenge, &state);

    let cancelled = std::sync::Arc::new(AtomicBool::new(false));
    if let Ok(mut g) = ACTIVE.lock() {
        *g = Some(cancelled.clone());
    }

    let app2 = app.clone();
    let spec_client = spec.client_id.clone();
    let token_url = spec.token_url.clone();
    std::thread::spawn(move || {
        run_server(
            app2,
            provider,
            server,
            spec_client,
            token_url,
            redirect_uri,
            verifier,
            state,
            cancelled,
        );
    });

    Ok(auth_url)
}

fn run_server(
    app: AppHandle,
    provider: Provider,
    server: Server,
    client_id: String,
    token_url: String,
    redirect_uri: String,
    verifier: String,
    expected_state: String,
    cancelled: std::sync::Arc<AtomicBool>,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(TIMEOUT_SECS);
    let rt = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => return emit_err(&app, provider, format!("runtime: {e}")),
    };

    let mut got_code: Option<String> = None;
    while got_code.is_none() {
        if cancelled.load(Ordering::SeqCst) {
            return emit_err(&app, provider, "cancelled".into());
        }
        if std::time::Instant::now() > deadline {
            return emit_err(&app, provider, format!("timed out after {TIMEOUT_SECS}s"));
        }
        match server.recv_timeout(std::time::Duration::from_secs(1)) {
            Ok(Some(req)) => {
                let url_str = req.url().to_string();
                let parsed = match url::Url::parse(&format!("http://localhost{url_str}")) {
                    Ok(u) => u,
                    Err(_) => {
                        let _ = req.respond(Response::from_string("Bad Request").with_status_code(400));
                        continue;
                    }
                };
                if parsed.path() != REDIRECT_PATH {
                    let _ = req.respond(Response::from_string("Not Found").with_status_code(404));
                    continue;
                }
                let params: std::collections::HashMap<String, String> =
                    parsed.query_pairs().into_owned().collect();
                if let Some(err) = params.get("error") {
                    let _ = req.respond(
                        Response::from_string(format!("OAuth error: {err}")).with_status_code(400),
                    );
                    return emit_err(&app, provider, format!("provider error: {err}"));
                }
                if params.get("state").map(|s| s.as_str()) != Some(&expected_state) {
                    let _ = req.respond(Response::from_string("State mismatch").with_status_code(400));
                    return emit_err(&app, provider, "state mismatch".into());
                }
                let code = match params.get("code") {
                    Some(c) if !c.is_empty() => c.clone(),
                    _ => {
                        let _ = req.respond(
                            Response::from_string("Missing code").with_status_code(400),
                        );
                        continue;
                    }
                };
                let _ = req.respond(Response::from_string(SUCCESS_HTML).with_header(
                    tiny_http::Header::from_bytes(
                        b"Content-Type".as_ref(),
                        b"text/html; charset=utf-8".as_ref(),
                    )
                    .unwrap(),
                ));
                got_code = Some(code);
            }
            _ => continue,
        }
    }

    let Some(code) = got_code else {
        return emit_err(&app, provider, "no code".into());
    };

    let tokens = rt.block_on(exchange(&token_url, &client_id, &redirect_uri, &verifier, &code));
    match tokens {
        Ok(t) => {
            let cred = build_credential(provider, &t);
            let label = cred.label.clone();
            store::add(cred);
            let _ = app.emit(
                "login-complete",
                LoginResult { provider, ok: true, label: Some(label), error: None },
            );
        }
        Err(e) => emit_err(&app, provider, format!("token exchange: {e}")),
    }
}

async fn exchange(
    token_url: &str,
    client_id: &str,
    redirect_uri: &str,
    verifier: &str,
    code: &str,
) -> Result<Value, String> {
    let body = format!(
        "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
        urlencoding::encode(code),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(client_id),
        urlencoding::encode(verifier)
    );
    let resp = reqwest::Client::new()
        .post(token_url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("{token_url} ({status}): {}", &text[..text.len().min(200)]));
    }
    serde_json::from_str::<Value>(&text).map_err(|e| format!("parse: {e}"))
}

fn build_credential(provider: Provider, tokens: &Value) -> StoredCredential {
    let access_token = tokens
        .get("access_token")
        .or_else(|| tokens.get("accessToken"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let refresh_token = tokens
        .get("refresh_token")
        .or_else(|| tokens.get("refreshToken"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let id_token = tokens
        .get("id_token")
        .or_else(|| tokens.get("idToken"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let expires_in = tokens.get("expires_in").and_then(|v| v.as_u64());
    let expires_at = expires_in
        .map(|s| chrono::Utc::now().timestamp_millis() + (s as i64) * 1000)
        .unwrap_or(0);

    let (label, account_id) = match (provider, &id_token) {
        (Provider::Codex, Some(jwt)) => {
            let claims = jwt_payload(jwt).ok();
            let email = claims
                .as_ref()
                .and_then(|c| c.get("email"))
                .and_then(|v| v.as_str())
                .map(String::from);
            let acct = claims
                .as_ref()
                .and_then(|c| c.get("https://api.openai.com/auth"))
                .and_then(|a| a.get("chatgpt_account_id"))
                .and_then(|v| v.as_str())
                .map(String::from);
            (email.unwrap_or_else(|| "Codex account".into()), acct)
        }
        (Provider::Claude, Some(jwt)) => {
            let claims = jwt_payload(jwt).ok();
            let email = claims
                .as_ref()
                .and_then(|c| c.get("email"))
                .and_then(|v| v.as_str())
                .map(String::from);
            (email.unwrap_or_else(|| "Claude account".into()), None)
        }
        _ => (format!("{provider:?} account"), None),
    };

    StoredCredential {
        id: String::new(),
        provider,
        label,
        access_token,
        refresh_token,
        expires_at,
        id_token,
        account_id,
    }
}

fn build_authorize_url(spec: &OAuthSpec, redirect_uri: &str, challenge: &str, state: &str) -> String {
    let mut params: Vec<(&str, String)> = vec![
        ("response_type", "code".into()),
        ("client_id", spec.client_id.clone()),
        ("redirect_uri", redirect_uri.into()),
        ("scope", spec.scope.clone()),
        ("code_challenge", challenge.into()),
        ("code_challenge_method", "S256".into()),
        ("state", state.into()),
    ];
    for (k, v) in &spec.extra_params {
        params.push((k, (*v).into()));
    }
    let query = params
        .iter()
        .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{}?{query}", spec.authorize_url)
}

fn pkce_verifier() -> String {
    let mut bytes = [0u8; 64];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

fn random_b64(n: usize) -> String {
    let mut bytes = vec![0u8; n];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn emit_err(app: &AppHandle, provider: Provider, msg: String) {
    let _ = app.emit(
        "login-complete",
        LoginResult { provider, ok: false, label: None, error: Some(msg) },
    );
}

const SUCCESS_HTML: &str = r#"<!DOCTYPE html><html><head><meta charset="utf-8"><title>Logged in</title>
<style>body{font-family:system-ui;display:flex;align-items:center;justify-content:center;height:100vh;margin:0;background:#111827;color:#e5e7eb}
.c{text-align:center}.ck{font-size:48px}</style></head>
<body><div class="c"><div class="ck">✓</div><h1>Logged in</h1><p>You can close this tab and return to AI Usage Tracker.</p></div></body></html>"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_roundtrip() {
        let v = pkce_verifier();
        let c = pkce_challenge(&v);
        assert!(c.len() > 20);
        assert_ne!(v, c);
    }

    #[test]
    fn codex_authorize_url_has_required_params() {
        let spec = spec_for(Provider::Codex).unwrap();
        let url = build_authorize_url(&spec, "http://localhost:1455/auth/callback", "chk", "st");
        assert!(url.contains("originator=codex_cli_rs"));
        assert!(url.contains("codex_cli_simplified_flow=true"));
        assert!(url.contains("code_challenge_method=S256"));
    }
}
