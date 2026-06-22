//! Device-code OAuth login using each official CLI's PUBLIC client_id, so a
//! manually-added account is treated as "logged in via that CLI". The backend
//! requests the device code, the UI opens the verification URL in the user's
//! browser, and this module polls to completion then persists the credential.
//! Feasible for Codex / Copilot. Gemini uses loopback OAuth (see the `oauth`
//! module); its Google installed-app client_id does not support the
//! device-code grant (`invalid_client: Invalid client type`). Claude
//! (Cloudflare-blocked) and Cursor (no public client_id) are not supported
//! here either.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tauri::AppHandle;

use crate::auth::cancel_token::CancelToken;
use crate::auth::finish::finish;
use crate::model::Provider;
use crate::store::StoredCredential;

const CODEX_ISSUER: &str = "https://auth.openai.com";
const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

const GH_CLIENT_ID: &str = "178c6fc778ccc68e1d6a";

const POLL_MAX_SECS: u64 = 15 * 60;

#[derive(Clone, Serialize)]
pub struct LoginInfo {
    pub provider: Provider,
    pub verification_url: String,
    pub user_code: String,
    pub expires_in: u64,
}

/// Cancel flag for the active device-code login. Independent of the OAuth
/// callback flow's token — `cancel_login` cancels both deliberately.
static DEVICE_ACTIVE: CancelToken = CancelToken::new();

pub fn cancel() {
    DEVICE_ACTIVE.cancel();
}

/// Request a device code and spawn the background poll. Returns what the UI
/// needs to show the code + open the browser.
pub async fn start(app: AppHandle, provider: Provider) -> Result<LoginInfo, String> {
    let http = crate::http::shared();
    match provider {
        Provider::Codex => start_codex(app, http, provider).await,
        Provider::Copilot => start_github(app, http, provider).await,
        _ => Err(format!(
            "{provider:?} does not support device-code login; Gemini uses browser OAuth"
        )),
    }
}

async fn post_json<T: DeserializeOwned>(
    http: &reqwest::Client,
    url: &str,
    body: serde_json::Value,
) -> Result<T, String> {
    let resp = http
        .post(url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    read::<T>(resp, url).await
}

async fn post_form<T: DeserializeOwned>(
    http: &reqwest::Client,
    url: &str,
    form: &[(&str, &str)],
) -> Result<T, String> {
    let resp = http
        .post(url)
        .header("Accept", "application/json")
        .form(form)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    read::<T>(resp, url).await
}

async fn read<T: DeserializeOwned>(resp: reqwest::Response, url: &str) -> Result<T, String> {
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "{url} ({status}): {}",
            crate::util::scrub_sensitive_text(&text)
                .chars()
                .take(200)
                .collect::<String>()
        ));
    }
    serde_json::from_str(&text).map_err(|e| {
        format!(
            "parse {url}: {e} (body was: {})",
            crate::util::scrub_sensitive_text(&text)
                .chars()
                .take(200)
                .collect::<String>()
        )
    })
}

// ---- Codex (OpenAI) ----

#[derive(Deserialize)]
struct CodexUserCode {
    device_auth_id: String,
    user_code: String,
    #[serde(default)]
    interval: serde_json::Value,
}
#[derive(Deserialize)]
struct CodexPoll {
    authorization_code: String,
    code_verifier: String,
}
#[derive(Deserialize)]
struct CodexTokens {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
}

async fn start_codex(
    app: AppHandle,
    http: reqwest::Client,
    provider: Provider,
) -> Result<LoginInfo, String> {
    let api = format!("{CODEX_ISSUER}/api/accounts");
    let r: CodexUserCode = post_json(
        &http,
        &format!("{api}/deviceauth/usercode"),
        serde_json::json!({"client_id":CODEX_CLIENT_ID}),
    )
    .await?;
    let interval = interval_secs(&r.interval, 5);
    let info = LoginInfo {
        provider,
        verification_url: format!("{CODEX_ISSUER}/codex/device"),
        user_code: r.user_code.clone(),
        expires_in: 900,
    };
    let app2 = app.clone();
    let cancelled = Arc::new(AtomicBool::new(false));
    DEVICE_ACTIVE.install(cancelled.clone());
    tauri::async_runtime::spawn(async move {
        let _active = DEVICE_ACTIVE.guard(cancelled.clone());
        let res = poll_codex(http, r.device_auth_id, r.user_code, interval, cancelled).await;
        finish(&app2, provider, res);
    });
    Ok(info)
}

async fn poll_codex(
    http: reqwest::Client,
    device_auth_id: String,
    user_code: String,
    interval: u64,
    cancelled: Arc<AtomicBool>,
) -> Result<StoredCredential, String> {
    let api = format!("{CODEX_ISSUER}/api/accounts");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(POLL_MAX_SECS);
    let auth = loop {
        check_cancelled(&cancelled)?;
        let body = serde_json::json!({ "device_auth_id": device_auth_id, "user_code": user_code });
        let resp = http
            .post(format!("{api}/deviceauth/token"))
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = resp.status();
        if status.is_success() {
            let p: CodexPoll = resp.json().await.map_err(|e| e.to_string())?;
            break p;
        }
        if (status.as_u16() == 403 || status.as_u16() == 404)
            && std::time::Instant::now() < deadline
        {
            sleep_or_cancel(interval, &cancelled).await?;
            continue;
        }
        return Err(format!("device auth ended ({status})"));
    };

    let token_url = format!("{CODEX_ISSUER}/oauth/token");
    let resp = http
        .post(&token_url)
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(codex_token_exchange_body(
            &auth.authorization_code,
            &auth.code_verifier,
        ))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let tokens: CodexTokens = read(resp, &token_url).await?;

    let access_token = tokens.access_token.ok_or("no access_token")?;
    Ok(codex_credential(
        access_token,
        tokens.refresh_token,
        tokens.id_token,
    ))
}

/// Build a fresh device-code Codex credential, deriving `expires_at` (epoch ms)
/// from the access token's JWT `exp`, falling back to the id_token's `exp`, then
/// 0 (unknown). Critical: without a real expiry the proactive-refresh gate in
/// `fetch_credential` (`expires_at > 0 && < now`) never fires, so the stored
/// refresh token is never used and the account silently dies once the access JWT
/// expires (B-4). Mirrors `codex::build_refreshed_cred`'s exp logic; consistent
/// with the loopback-OAuth path, which already sets `expires_at`.
fn codex_credential(
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
) -> StoredCredential {
    let (label, account_id) = id_token
        .as_deref()
        .map(crate::jwt::codex_identity)
        .unwrap_or((None, None));
    let expires_at = crate::jwt::jwt_exp(&access_token)
        .or_else(|| id_token.as_deref().and_then(crate::jwt::jwt_exp))
        .map(|s| s * 1000)
        .unwrap_or(0);
    StoredCredential {
        id: String::new(),
        provider: Provider::Codex,
        label: label.unwrap_or_else(|| "Codex account".into()),
        access_token,
        refresh_token,
        expires_at,
        id_token,
        account_id,
    }
}

fn codex_token_exchange_body(code: &str, code_verifier: &str) -> String {
    format!(
        "grant_type=authorization_code&code={}&client_id={}&redirect_uri={}&code_verifier={}",
        urlencoding::encode(code),
        urlencoding::encode(CODEX_CLIENT_ID),
        urlencoding::encode(&format!("{CODEX_ISSUER}/deviceauth/callback")),
        urlencoding::encode(code_verifier),
    )
}

// ---- GitHub Copilot ----

#[derive(Deserialize)]
struct GhDeviceCode {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default)]
    interval: u64,
}

async fn start_github(
    app: AppHandle,
    http: reqwest::Client,
    provider: Provider,
) -> Result<LoginInfo, String> {
    let r: GhDeviceCode = post_form(
        &http,
        "https://github.com/login/device/code",
        &[("client_id", GH_CLIENT_ID), ("scope", "read:user")],
    )
    .await?;
    let interval = r.interval.max(5);
    let info = LoginInfo {
        provider,
        verification_url: r.verification_uri.clone(),
        user_code: r.user_code.clone(),
        expires_in: 900,
    };
    let app2 = app.clone();
    let cancelled = Arc::new(AtomicBool::new(false));
    DEVICE_ACTIVE.install(cancelled.clone());
    tauri::async_runtime::spawn(async move {
        let _active = DEVICE_ACTIVE.guard(cancelled.clone());
        let res = poll_github(http, r.device_code, r.user_code, interval, cancelled).await;
        finish(&app2, provider, res);
    });
    Ok(info)
}

async fn poll_github(
    http: reqwest::Client,
    device_code: String,
    _user_code: String,
    interval: u64,
    cancelled: Arc<AtomicBool>,
) -> Result<StoredCredential, String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(POLL_MAX_SECS);
    let token = loop {
        check_cancelled(&cancelled)?;
        let resp = http
            .post("https://github.com/login/oauth/access_token")
            .form(&[
                ("client_id", GH_CLIENT_ID),
                ("device_code", &device_code),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| e.to_string())?;
        #[derive(Deserialize)]
        struct GhTok {
            #[serde(default)]
            access_token: Option<String>,
            #[serde(default)]
            error: Option<String>,
        }
        let t: GhTok = resp.json().await.map_err(|e| e.to_string())?;
        if let Some(tok) = t.access_token {
            break tok;
        }
        match t.error.as_deref() {
            Some("authorization_pending") | Some("slow_down")
                if std::time::Instant::now() < deadline =>
            {
                let extra = if t.error.as_deref() == Some("slow_down") {
                    5
                } else {
                    0
                };
                sleep_or_cancel(interval + extra, &cancelled).await?;
                continue;
            }
            Some(e) => return Err(format!("github device: {e}")),
            None => return Err("github device: no token, no error".into()),
        }
    };

    // Resolve username for the label.
    let label = resolve_github_label(&http, &token, "https://api.github.com/user")
        .await
        .unwrap_or_else(|| "GitHub account".into());

    Ok(StoredCredential {
        id: String::new(),
        provider: Provider::Copilot,
        label,
        access_token: token,
        refresh_token: None,
        expires_at: 0,
        id_token: None,
        account_id: None,
    })
}

/// Resolve a GitHub `login` name for the account label. Copilot CLI `gho_`
/// user-OAuth tokens use the legacy `Authorization: token <t>` scheme — NOT
/// `Bearer` (which 401s and degrades the label to a generic fallback, BUG-5).
/// Matches `copilot.rs::fetch_with`. `user_url` is injectable for testing.
async fn resolve_github_label(
    http: &reqwest::Client,
    token: &str,
    user_url: &str,
) -> Option<String> {
    match http
        .get(user_url)
        .header("Authorization", format!("token {token}"))
        .header("User-Agent", "ai-usage-tracker")
        .send()
        .await
    {
        Ok(r) => r
            .json::<serde_json::Value>()
            .await
            .ok()
            .and_then(|v| v.get("login").and_then(|x| x.as_str()).map(String::from)),
        Err(_) => None,
    }
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::SeqCst) {
        Err("cancelled".into())
    } else {
        Ok(())
    }
}

async fn sleep_or_cancel(secs: u64, cancelled: &AtomicBool) -> Result<(), String> {
    for _ in 0..secs.max(1) {
        check_cancelled(cancelled)?;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    check_cancelled(cancelled)
}

// Gemini used to live here as a device-code flow, but Google's installed-app
// client_id (the one gemini-cli ships) rejects the device-code grant with
// `invalid_client: Invalid client type`. Gemini now uses loopback OAuth in the
// `oauth` module (Authorization Code + PKCE + client_secret, matching
// gemini-cli's `oauth2.ts`).

fn interval_secs(v: &serde_json::Value, default: u64) -> u64 {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        .unwrap_or(default)
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_jwt(claims: serde_json::Value) -> String {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
        format!("hdr.{payload}.sig")
    }

    #[test]
    fn codex_credential_sets_expiry_from_access_jwt_and_extracts_identity() {
        let access = fake_jwt(serde_json::json!({ "exp": 1_700_000_000_i64 }));
        let id_token = fake_jwt(serde_json::json!({
            "email": "codex-login@example.invalid",
            "https://api.openai.com/auth": { "chatgpt_account_id": "acct-9" },
        }));
        let cred = codex_credential(access, Some("rt".into()), Some(id_token));
        // exp seconds → epoch ms (re-enables the proactive-refresh gate — B-4).
        assert_eq!(cred.expires_at, 1_700_000_000_000);
        assert_eq!(cred.label, "codex-login@example.invalid");
        assert_eq!(cred.account_id.as_deref(), Some("acct-9"));
        assert_eq!(cred.refresh_token.as_deref(), Some("rt"));
        assert_eq!(cred.provider, Provider::Codex);
    }

    #[test]
    fn codex_credential_falls_back_to_id_token_exp_then_zero() {
        // Opaque access token, but the id_token carries exp → use it.
        let id_token = fake_jwt(serde_json::json!({ "exp": 2_000_000_000_i64 }));
        let cred = codex_credential("opaque".into(), None, Some(id_token));
        assert_eq!(cred.expires_at, 2_000_000_000_000);
        // Opaque access + no id_token → unknown (0), with the generic label.
        let cred2 = codex_credential("opaque".into(), None, None);
        assert_eq!(cred2.expires_at, 0);
        assert_eq!(cred2.label, "Codex account");
    }

    #[test]
    fn codex_token_exchange_body_is_form_urlencoded() {
        assert_eq!(
            codex_token_exchange_body("auth-code", "verifier"),
            format!(
                "grant_type=authorization_code&code=auth-code&client_id={CODEX_CLIENT_ID}&redirect_uri=https%3A%2F%2Fauth.openai.com%2Fdeviceauth%2Fcallback&code_verifier=verifier"
            )
        );
    }

    static DEVICE_ACTIVE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    // The install/cancel/guard mechanics are proven once in
    // `auth::cancel_token`; this verifies the device flow is wired to its OWN
    // `DEVICE_ACTIVE` token and that the public `cancel()` (cancel_login) flips
    // it — i.e. cancelling the device-code poll, independently of OAuth.
    #[test]
    fn device_cancel_flag_lifecycle() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let _g = DEVICE_ACTIVE_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let first = Arc::new(AtomicBool::new(false));
        DEVICE_ACTIVE.install(first.clone());
        let second = Arc::new(AtomicBool::new(false));
        DEVICE_ACTIVE.install(second.clone());
        assert!(
            first.load(Ordering::SeqCst),
            "starting a new device-code login cancels the previous poll"
        );
        assert!(!second.load(Ordering::SeqCst));

        cancel();
        assert!(
            second.load(Ordering::SeqCst),
            "cancel_login must cancel the active device-code poll"
        );

        // The owning poll's guard clears the device flag on exit.
        let owned = Arc::new(AtomicBool::new(false));
        DEVICE_ACTIVE.install(owned.clone());
        drop(DEVICE_ACTIVE.guard(owned));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn github_label_lookup_uses_token_scheme_not_bearer() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        // Copilot CLI `gho_` user-OAuth tokens use the legacy `token <t>` scheme;
        // a `Bearer <t>` request would 401 and degrade the label (BUG-5). The
        // mock only answers 200 when the scheme is `token`.
        Mock::given(method("GET"))
            .and(path("/user"))
            .and(header("authorization", "token gho-test"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"login": "octocat"})),
            )
            .mount(&server)
            .await;
        let client = crate::http::build_client();
        let url = format!("{}/user", server.uri());
        let label = resolve_github_label(&client, "gho-test", &url).await;
        assert_eq!(label.as_deref(), Some("octocat"));
    }
}
