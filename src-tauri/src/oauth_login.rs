//! Browser OAuth login via a localhost callback server. The provider allows a
//! localhost redirect, so we run a local callback server and capture the
//! authorization code automatically. Used by Codex (OpenAI PKCE) and Gemini
//! (Google Authorization Code + loopback, matching gemini-cli's oauth2.ts).

//! Gemini CLI's public installed-app client (same values gemini-cli ships).
//! The "secret" is not actually secret for installed apps (RFC 6749 §2.1);
//! Google still requires it in the token exchange for this client type.
const GEMINI_CID: &str = "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com";
const GEMINI_CSEC: &str = "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";

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
use crate::model::{Provider, EVENT_LOGIN_COMPLETE};
use crate::store::{self, StoredCredential};

const LOCAL_REDIRECT_PATH: &str = "/auth/callback";
const TIMEOUT_SECS: u64 = 300;

#[derive(Clone)]
enum LoginMode {
    /// Localhost callback server; code captured automatically.
    LocalServer,
}

struct OAuthSpec {
    authorize_url: String,
    token_url: String,
    client_id: String,
    /// Optional client_secret for installed-app clients (Google). PKCE alone
    /// also works, but matching gemini-cli exactly (id+secret) is safest.
    client_secret: Option<String>,
    scope: String,
    extra_params: Vec<(&'static str, &'static str)>,
    mode: LoginMode,
}

fn spec_for(p: Provider) -> Option<OAuthSpec> {
    match p {
        Provider::Codex => Some(OAuthSpec {
            authorize_url: "https://auth.openai.com/oauth/authorize".into(),
            token_url: "https://auth.openai.com/oauth/token".into(),
            client_id: "app_EMoamEEZ73f0CkXaXp7hrann".into(),
            client_secret: None,
            scope: "openid profile email offline_access api.connectors.read api.connectors.invoke".into(),
            extra_params: vec![
                ("originator", "codex_cli_rs"),
                ("codex_cli_simplified_flow", "true"),
                ("id_token_add_organizations", "true"),
            ],
            mode: LoginMode::LocalServer,
        }),
        Provider::Gemini => Some(OAuthSpec {
            // Google Authorization Code + loopback redirect (gemini-cli pattern).
            // The Gemini client_id does NOT support the device-code grant —
            // googleapis.com/device/code returns "invalid_client: Invalid
            // client type" — so we use the same loopback flow gemini-cli uses.
            authorize_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            token_url: "https://oauth2.googleapis.com/token".into(),
            client_id: GEMINI_CID.into(),
            client_secret: Some(GEMINI_CSEC.into()),
            scope: "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile".into(),
            extra_params: vec![
                ("access_type", "offline"),
                ("prompt", "consent"),
            ],
            mode: LoginMode::LocalServer,
        }),
        _ => None,
    }
}

#[derive(Clone, Serialize)]
struct LoginResult {
    provider: Provider,
    ok: bool,
    label: Option<String>,
    error: Option<String>,
}

/// Cancel flag for the active LocalServer login.
static ACTIVE: LazyLock<Mutex<Option<std::sync::Arc<AtomicBool>>>> =
    LazyLock::new(|| Mutex::new(None));

pub fn cancel() {
    if let Ok(guard) = ACTIVE.lock() {
        if let Some(flag) = guard.as_ref() {
            flag.store(true, Ordering::SeqCst);
        }
    }
}

/// Install `new` as the active cancel flag, cancelling any prior in-flight login
/// first so its server thread exits promptly (freeing the callback port) instead
/// of lingering until its own timeout.
fn install_cancel_flag(new: std::sync::Arc<AtomicBool>) {
    if let Ok(mut g) = ACTIVE.lock() {
        if let Some(prev) = g.take() {
            prev.store(true, Ordering::SeqCst);
        }
        *g = Some(new);
    }
}

/// Clears `ACTIVE` when a login's server thread exits, but only if `ACTIVE` still
/// points at this login's flag (a newer login may have already replaced it).
struct ActiveGuard(std::sync::Arc<AtomicBool>);

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        if let Ok(mut g) = ACTIVE.lock() {
            if g.as_ref()
                .is_some_and(|cur| std::sync::Arc::ptr_eq(cur, &self.0))
            {
                *g = None;
            }
        }
    }
}

/// Start a login. Returns the authorize URL for the browser.
pub fn start(app: AppHandle, provider: Provider) -> Result<String, String> {
    let spec = spec_for(provider)
        .ok_or_else(|| "OAuth login not supported for this provider".to_string())?;
    let verifier = pkce_verifier();
    let challenge = pkce_challenge(&verifier);
    let state = random_b64(32);

    match spec.mode.clone() {
        LoginMode::LocalServer => {
            let server = Server::http("127.0.0.1:1455")
                .or_else(|_| Server::http("127.0.0.1:0"))
                .map_err(|e| format!("start local server: {e}"))?;
            let port = server.server_addr().to_ip().map(|a| a.port()).unwrap_or(0);
            let redirect_uri = format!("http://localhost:{port}{LOCAL_REDIRECT_PATH}");
            let auth_url = build_authorize_url(&spec, &redirect_uri, &challenge, &state);

            let cancelled = std::sync::Arc::new(AtomicBool::new(false));
            install_cancel_flag(cancelled.clone());
            let ctx = RunCtx {
                app: app.clone(),
                provider,
                client_id: spec.client_id.clone(),
                client_secret: spec.client_secret.clone(),
                token_url: spec.token_url.clone(),
                redirect_uri,
                verifier,
                expected_state: state,
                cancelled,
            };
            std::thread::spawn(move || run_server(server, ctx));
            Ok(auth_url)
        }
    }
}

struct RunCtx {
    app: AppHandle,
    provider: Provider,
    client_id: String,
    client_secret: Option<String>,
    token_url: String,
    redirect_uri: String,
    verifier: String,
    expected_state: String,
    cancelled: std::sync::Arc<AtomicBool>,
}

/// What to do with one inbound callback request, decided purely from its query
/// params. `state` is validated FIRST (X-3): any request whose state does not
/// match ours is `Ignore`d — NOT treated as an error — so an attacker-supplied
/// `?error=...` to the fixed callback port cannot abort the user's in-flight
/// login. Only a state-matched request can finish the login (success or error).
#[derive(Debug, PartialEq, Eq)]
enum CallbackAction {
    /// Wrong/missing state — not our callback. Reject and keep waiting.
    Ignore,
    /// State-matched genuine provider error → fail the login.
    Error(String),
    /// State-matched success → use this authorization code.
    Code(String),
    /// State-matched but no usable code → keep waiting.
    MissingCode,
}

fn classify_callback(
    params: &std::collections::HashMap<String, String>,
    expected_state: &str,
) -> CallbackAction {
    // X-3: validate `state` before acting on anything else. A non-matching
    // (or absent) state means the request is not the user's real callback.
    if params.get("state").map(|s| s.as_str()) != Some(expected_state) {
        return CallbackAction::Ignore;
    }
    if let Some(err) = params.get("error") {
        return CallbackAction::Error(err.clone());
    }
    match params.get("code") {
        Some(c) if !c.is_empty() => CallbackAction::Code(c.clone()),
        _ => CallbackAction::MissingCode,
    }
}

fn run_server(server: Server, ctx: RunCtx) {
    let RunCtx {
        app,
        provider,
        client_id,
        client_secret,
        token_url,
        redirect_uri,
        verifier,
        expected_state,
        cancelled,
    } = ctx;
    // Clear ACTIVE when this server thread exits (any return path), unless a
    // newer login has already taken over.
    let _active = ActiveGuard(cancelled.clone());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(TIMEOUT_SECS);
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
                        let _ =
                            req.respond(Response::from_string("Bad Request").with_status_code(400));
                        continue;
                    }
                };
                if parsed.path() != LOCAL_REDIRECT_PATH {
                    let _ = req.respond(Response::from_string("Not Found").with_status_code(404));
                    continue;
                }
                let params: std::collections::HashMap<String, String> =
                    parsed.query_pairs().into_owned().collect();
                match classify_callback(&params, &expected_state) {
                    // Not our callback (stale / cross-site / an attacker on the
                    // fixed, guessable port). Reject it but DO NOT end the in-flight
                    // login — keep waiting for the state-matched callback (X-3).
                    CallbackAction::Ignore => {
                        let _ = req
                            .respond(Response::from_string("State mismatch").with_status_code(400));
                        continue;
                    }
                    CallbackAction::Error(err) => {
                        let mut response =
                            Response::from_string(error_html(&format!("OAuth error: {err}")))
                                .with_status_code(400);
                        for header in html_security_headers() {
                            response = response.with_header(header);
                        }
                        let _ = req.respond(response);
                        return emit_err(&app, provider, format!("provider error: {err}"));
                    }
                    CallbackAction::MissingCode => {
                        let _ = req
                            .respond(Response::from_string("Missing code").with_status_code(400));
                        continue;
                    }
                    CallbackAction::Code(code) => {
                        let mut response = Response::from_string(success_html());
                        for header in html_security_headers() {
                            response = response.with_header(header);
                        }
                        let _ = req.respond(response);
                        got_code = Some(code);
                    }
                }
            }
            _ => continue,
        }
    }

    let Some(code) = got_code else {
        return emit_err(&app, provider, "no code".into());
    };
    match tauri::async_runtime::block_on(exchange(
        &token_url,
        &client_id,
        client_secret.as_deref(),
        &redirect_uri,
        &verifier,
        &code,
        None,
    )) {
        Ok(t) => {
            let cred = match build_credential(provider, &t) {
                Ok(cred) => cred,
                Err(e) => return emit_err(&app, provider, e),
            };
            let label = cred.label.clone();
            match store::add(cred) {
                Ok(_) => {
                    let _ = app.emit(
                        EVENT_LOGIN_COMPLETE,
                        LoginResult {
                            provider,
                            ok: true,
                            label: Some(label),
                            error: None,
                        },
                    );
                }
                Err(e) => emit_err(&app, provider, format!("failed to store credential: {e}")),
            }
        }
        Err(e) => emit_err(&app, provider, format!("token exchange: {e}")),
    }
}

async fn exchange(
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

fn build_credential(provider: Provider, tokens: &Value) -> Result<StoredCredential, String> {
    let access_token = tokens
        .get("access_token")
        .or_else(|| tokens.get("accessToken"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .ok_or_else(|| "OAuth token response missing access_token".to_string())?;
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
            let (email, acct) = crate::jwt::codex_identity(jwt);
            (email.unwrap_or_else(|| "Codex account".into()), acct)
        }
        (Provider::Gemini, Some(jwt)) => {
            // Google id_token JWT carries the user email at the top level.
            let email = jwt_payload(jwt)
                .ok()
                .and_then(|c| c.get("email").and_then(|v| v.as_str()).map(String::from));
            (email.unwrap_or_else(|| "Gemini account".into()), None)
        }
        _ => (format!("{provider:?} account"), None),
    };

    Ok(StoredCredential {
        id: String::new(),
        provider,
        label,
        access_token,
        refresh_token,
        expires_at,
        id_token,
        account_id,
    })
}

fn build_authorize_url(
    spec: &OAuthSpec,
    redirect_uri: &str,
    challenge: &str,
    state: &str,
) -> String {
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
    // RFC 7636 §4.1: 43–128 chars; 32 random bytes → 43 base64url chars.
    random_b64(32)
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
        EVENT_LOGIN_COMPLETE,
        LoginResult {
            provider,
            ok: false,
            label: None,
            error: Some(msg),
        },
    );
}

fn html_security_headers() -> Vec<tiny_http::Header> {
    [
        ("Content-Type", "text/html; charset=utf-8"),
        ("Cache-Control", "no-store"),
        ("Pragma", "no-cache"),
        ("Referrer-Policy", "no-referrer"),
    ]
    .into_iter()
    .map(|(name, value)| tiny_http::Header::from_bytes(name.as_bytes(), value.as_bytes()).unwrap())
    .collect()
}

fn success_html() -> String {
    r#"<!DOCTYPE html><html><head><meta charset="utf-8"><meta name="referrer" content="no-referrer"><title>Logged in</title>
<style>body{font-family:system-ui;display:flex;align-items:center;justify-content:center;height:100vh;margin:0;background:#111827;color:#e5e7eb}
.c{text-align:center}.ck{font-size:48px}</style><script>history.replaceState(null,"",location.pathname);</script></head>
<body><div class="c"><div class="ck">✓</div><h1>Logged in</h1><p>You can close this tab and return to AI Usage Tracker.</p></div></body></html>"#.to_string()
}

fn error_html(message: &str) -> String {
    let escaped = escape_html(message);
    format!(
        r#"<!DOCTYPE html><html><head><meta charset="utf-8"><meta name="referrer" content="no-referrer"><title>Login failed</title>
<style>body{{font-family:system-ui;display:flex;align-items:center;justify-content:center;height:100vh;margin:0;background:#111827;color:#e5e7eb}}.c{{max-width:640px;text-align:center}}</style><script>history.replaceState(null,"",location.pathname);</script></head>
<body><div class="c"><h1>Login failed</h1><p>{escaped}</p></div></body></html>"#
    )
}

fn escape_html(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            '&' => "&amp;".chars().collect::<Vec<_>>(),
            '<' => "&lt;".chars().collect::<Vec<_>>(),
            '>' => "&gt;".chars().collect::<Vec<_>>(),
            '"' => "&quot;".chars().collect::<Vec<_>>(),
            '\'' => "&#39;".chars().collect::<Vec<_>>(),
            _ => vec![c],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn callback_with_wrong_or_missing_state_is_ignored_not_errored() {
        // The security property (X-3): an attacker hitting the fixed callback
        // port with ?error=... and a wrong/absent state must NOT abort the login.
        assert_eq!(
            classify_callback(&params(&[("error", "access_denied")]), "S"),
            CallbackAction::Ignore,
            "error with NO state must be ignored, not treated as a provider error"
        );
        assert_eq!(
            classify_callback(&params(&[("error", "x"), ("state", "WRONG")]), "S"),
            CallbackAction::Ignore
        );
        assert_eq!(
            classify_callback(&params(&[("code", "c"), ("state", "WRONG")]), "S"),
            CallbackAction::Ignore,
            "even a code with the wrong state is ignored"
        );
    }

    #[test]
    fn state_matched_callback_resolves_error_or_code() {
        assert_eq!(
            classify_callback(&params(&[("error", "denied"), ("state", "S")]), "S"),
            CallbackAction::Error("denied".into())
        );
        assert_eq!(
            classify_callback(&params(&[("code", "abc"), ("state", "S")]), "S"),
            CallbackAction::Code("abc".into())
        );
        // State-matched but empty/absent code → keep waiting, don't fail.
        assert_eq!(
            classify_callback(&params(&[("code", ""), ("state", "S")]), "S"),
            CallbackAction::MissingCode
        );
        assert_eq!(
            classify_callback(&params(&[("state", "S")]), "S"),
            CallbackAction::MissingCode
        );
    }

    #[test]
    fn oauth_success_html_scrubs_query_and_blocks_referrers() {
        let html = success_html();
        assert!(
            html.contains("history.replaceState"),
            "query scrub missing: {html}"
        );
        assert!(html.contains("referrer") && html.contains("no-referrer"));
        assert!(html.contains("Logged in"));
    }

    #[test]
    fn oauth_error_html_escapes_provider_error() {
        let html = error_html(r#"bad <script>alert("x")</script> & retry"#);
        assert!(html.contains("&lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt;"));
        assert!(html.contains("&amp; retry"));
        assert!(!html.contains("<script>alert"));
    }

    #[test]
    fn oauth_html_security_headers_disable_storage_and_referrers() {
        let headers = html_security_headers();
        let joined = headers
            .iter()
            .map(|h| format!("{}: {}", h.field, h.value))
            .collect::<Vec<_>>()
            .join("\n")
            .to_ascii_lowercase();
        assert!(joined.contains("cache-control: no-store"));
        assert!(joined.contains("pragma: no-cache"));
        assert!(joined.contains("referrer-policy: no-referrer"));
        assert!(joined.contains("content-type: text/html; charset=utf-8"));
    }

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
        assert!(url.contains("api.connectors.read"));
        assert!(url.contains("api.connectors.invoke"));
    }

    #[test]
    fn gemini_spec_uses_loopback_oauth_with_secret() {
        // Gemini must NOT use device-code (google rejects it with
        // `invalid_client: Invalid client type`); it uses Authorization Code +
        // loopback redirect like gemini-cli.
        let spec = spec_for(Provider::Gemini).expect("gemini must support OAuth");
        assert!(
            spec.client_secret.is_some(),
            "gemini client_secret required"
        );
        assert!(spec.authorize_url.contains("accounts.google.com"));
        assert!(spec.token_url.contains("oauth2.googleapis.com"));
        assert!(spec.scope.contains("cloud-platform"));
        assert!(spec.scope.contains("userinfo.email"));
        // access_type=offline + prompt=consent forces a refresh_token.
        assert!(spec
            .extra_params
            .iter()
            .any(|(k, v)| *k == "access_type" && *v == "offline"));
        assert!(spec
            .extra_params
            .iter()
            .any(|(k, v)| *k == "prompt" && *v == "consent"));
    }

    /// Encode a fake JWT (`hdr.<payload>.sig`) whose middle segment is the given
    /// claims, so `build_credential`'s id_token parsing can be exercised offline.
    fn fake_jwt(claims: serde_json::Value) -> String {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
        format!("hdr.{payload}.sig")
    }

    #[test]
    fn build_credential_extracts_codex_email_and_account_id() {
        let id_token = fake_jwt(serde_json::json!({
            "email": "codex-oauth@example.invalid",
            "https://api.openai.com/auth": { "chatgpt_account_id": "acct-123" },
        }));
        let tokens = serde_json::json!({
            "access_token": "at",
            "refresh_token": "rt",
            "id_token": id_token,
            "expires_in": 3600,
        });
        let cred = build_credential(Provider::Codex, &tokens).unwrap();
        assert_eq!(cred.access_token, "at");
        assert_eq!(cred.refresh_token.as_deref(), Some("rt"));
        assert_eq!(cred.label, "codex-oauth@example.invalid");
        assert_eq!(cred.account_id.as_deref(), Some("acct-123"));
        assert!(cred.expires_at > 0, "expires_in should set expires_at");
    }

    #[test]
    fn build_credential_rejects_missing_access_token() {
        let tokens = serde_json::json!({
            "refresh_token": "rt",
            "expires_in": 3600,
        });

        let err = build_credential(Provider::Codex, &tokens).unwrap_err();

        assert!(err.contains("access_token"));
    }

    #[test]
    fn build_credential_uses_gemini_email_and_no_account_id() {
        let id_token = fake_jwt(serde_json::json!({ "email": "gemini-oauth@example.invalid" }));
        let tokens = serde_json::json!({ "access_token": "g-at", "id_token": id_token });
        let cred = build_credential(Provider::Gemini, &tokens).unwrap();
        assert_eq!(cred.label, "gemini-oauth@example.invalid");
        assert_eq!(cred.account_id, None);
        assert_eq!(cred.expires_at, 0, "no expires_in → unknown expiry");
    }

    #[test]
    fn build_credential_falls_back_to_generic_label_without_id_token() {
        let tokens = serde_json::json!({ "access_token": "at" });
        let cred = build_credential(Provider::Codex, &tokens).unwrap();
        assert_eq!(cred.access_token, "at");
        assert_eq!(cred.label, "Codex account");
        assert_eq!(cred.account_id, None);
    }

    // Serializes the ACTIVE-static tests against each other (other oauth tests
    // don't touch ACTIVE). Reset ACTIVE at both ends to avoid cross-run bleed.
    static ACTIVE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn cancel_flag_lifecycle() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let _g = ACTIVE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        if let Ok(mut a) = ACTIVE.lock() {
            *a = None;
        }

        // Installing a second login cancels the first and installs itself.
        let first = Arc::new(AtomicBool::new(false));
        install_cancel_flag(first.clone());
        let second = Arc::new(AtomicBool::new(false));
        install_cancel_flag(second.clone());
        assert!(
            first.load(Ordering::SeqCst),
            "a new login must cancel the previous one"
        );
        assert!(!second.load(Ordering::SeqCst));

        // cancel() targets the current (second) flag.
        cancel();
        assert!(second.load(Ordering::SeqCst));

        // An ActiveGuard clears ACTIVE iff it owns the current flag.
        let owned = Arc::new(AtomicBool::new(false));
        install_cancel_flag(owned.clone());
        drop(ActiveGuard(owned.clone()));
        assert!(
            ACTIVE.lock().unwrap().is_none(),
            "guard owning the current flag clears ACTIVE on drop"
        );

        // A stale guard must NOT clear a newer login's flag.
        let newer = Arc::new(AtomicBool::new(false));
        install_cancel_flag(newer.clone());
        drop(ActiveGuard(Arc::new(AtomicBool::new(false))));
        assert!(
            ACTIVE.lock().unwrap().is_some(),
            "a stale guard must not clear the newer login's flag"
        );

        if let Ok(mut a) = ACTIVE.lock() {
            *a = None;
        }
    }
}
