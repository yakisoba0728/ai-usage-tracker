//! Device-code OAuth login using each official CLI's PUBLIC client_id, so a
//! manually-added account is treated as "logged in via that CLI". The backend
//! requests the device code, the UI opens the verification URL in the user's
//! browser, and this module polls to completion then persists the credential.
//! Feasible for Codex / Copilot / Gemini (login surfaces are not bot-gated).
//! Claude (console.anthropic.com is Cloudflare-blocked) and Cursor (no public
//! client_id) are not supported here.

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::jwt::jwt_payload;
use crate::model::Provider;
use crate::store::{self, StoredCredential};

const CODEX_ISSUER: &str = "https://auth.openai.com";
const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

const GH_CLIENT_ID: &str = "178c6fc778ccc68e1d6a";

const GEMINI_CID: &str =
    "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com";
const GEMINI_CSEC: &str = "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";

const POLL_MAX_SECS: u64 = 15 * 60;

#[derive(Clone, Serialize)]
pub struct LoginInfo {
    pub provider: Provider,
    pub verification_url: String,
    pub user_code: String,
    pub expires_in: u64,
}

#[derive(Clone, Serialize)]
struct LoginResult {
    provider: Provider,
    ok: bool,
    label: Option<String>,
    error: Option<String>,
}

pub fn supports_login(p: Provider) -> bool {
    matches!(p, Provider::Codex | Provider::Copilot | Provider::Gemini)
}

/// Request a device code and spawn the background poll. Returns what the UI
/// needs to show the code + open the browser.
pub async fn start(app: AppHandle, provider: Provider) -> Result<LoginInfo, String> {
    let http = crate::http::build_client();
    match provider {
        Provider::Codex => start_codex(app, http, provider).await,
        Provider::Copilot => start_github(app, http, provider).await,
        Provider::Gemini => start_gemini(app, http, provider).await,
        _ => Err(format!("{provider:?} does not support in-app login")),
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
    let resp = http.post(url).form(form).send().await.map_err(|e| e.to_string())?;
    read::<T>(resp, url).await
}

async fn read<T: DeserializeOwned>(resp: reqwest::Response, url: &str) -> Result<T, String> {
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("{url} ({status}): {}", &text[..text.len().min(200)]));
    }
    serde_json::from_str(&text).map_err(|e| format!("parse {url}: {e}"))
}

fn finish(app: &AppHandle, provider: Provider, res: Result<StoredCredential, String>) {
    let result = match res {
        Ok(c) => {
            let label = c.label.clone();
            store::add(c);
            LoginResult { provider, ok: true, label: Some(label), error: None }
        }
        Err(e) => LoginResult { provider, ok: false, label: None, error: Some(e) },
    };
    let _ = app.emit("login-complete", &result);
}

// ---- Codex (OpenAI) ----

#[derive(Deserialize)]
struct CodexUserCode {
    device_auth_id: String,
    user_code: String,
    #[serde(default)] interval: serde_json::Value,
}
#[derive(Deserialize)]
struct CodexPoll {
    authorization_code: String,
    code_verifier: String,
}
#[derive(Deserialize)]
struct CodexTokens {
    #[serde(default)] access_token: Option<String>,
    #[serde(default)] refresh_token: Option<String>,
    #[serde(default)] id_token: Option<String>,
}

async fn start_codex(app: AppHandle, http: reqwest::Client, provider: Provider) -> Result<LoginInfo, String> {
    let api = format!("{CODEX_ISSUER}/api/accounts");
    let r: CodexUserCode =
        post_json(&http, &format!("{api}/deviceauth/usercode"), serde_json::json!({"client_id":CODEX_CLIENT_ID}))
            .await?;
    let interval = interval_secs(&r.interval, 5);
    let info = LoginInfo {
        provider,
        verification_url: format!("{CODEX_ISSUER}/codex/device"),
        user_code: r.user_code.clone(),
        expires_in: 900,
    };
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let res = poll_codex(http, r.device_auth_id, r.user_code, interval).await;
        finish(&app2, provider, res);
    });
    Ok(info)
}

async fn poll_codex(
    http: reqwest::Client,
    device_auth_id: String,
    user_code: String,
    interval: u64,
) -> Result<StoredCredential, String> {
    let api = format!("{CODEX_ISSUER}/api/accounts");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(POLL_MAX_SECS);
    let auth = loop {
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
        if (status.as_u16() == 403 || status.as_u16() == 404) && std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
            continue;
        }
        return Err(format!("device auth ended ({status})"));
    };

    let tokens: CodexTokens = post_json(
        &http,
        &format!("{CODEX_ISSUER}/oauth/token"),
        serde_json::json!({
            "grant_type":"authorization_code",
            "code": auth.authorization_code,
            "client_id": CODEX_CLIENT_ID,
            "redirect_uri": format!("{CODEX_ISSUER}/deviceauth/callback"),
            "code_verifier": auth.code_verifier,
        }),
    )
    .await?;

    let access_token = tokens.access_token.ok_or("no access_token")?;
    let (label, account_id) = tokens
        .id_token
        .as_deref()
        .map(|t| {
            let claims = jwt_payload(t).ok();
            let email = claims.as_ref().and_then(|c| c.get("email")).and_then(|v| v.as_str()).map(String::from);
            let acct = claims
                .as_ref()
                .and_then(|c| c.get("https://api.openai.com/auth"))
                .and_then(|a| a.get("chatgpt_account_id"))
                .and_then(|v| v.as_str())
                .map(String::from);
            (email, acct)
        })
        .unwrap_or((None, None));

    Ok(StoredCredential {
        id: String::new(),
        provider: Provider::Codex,
        label: label.unwrap_or_else(|| "Codex account".into()),
        access_token,
        refresh_token: tokens.refresh_token,
        expires_at: 0,
        id_token: tokens.id_token,
        account_id,
    })
}

// ---- GitHub Copilot ----

#[derive(Deserialize)]
struct GhDeviceCode {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default)] interval: u64,
}

async fn start_github(app: AppHandle, http: reqwest::Client, provider: Provider) -> Result<LoginInfo, String> {
    let r: GhDeviceCode = post_form(
        &http,
        "https://github.com/login/device/code",
        &[
            ("client_id", GH_CLIENT_ID),
            ("scope", "read:user"),
        ],
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
    tauri::async_runtime::spawn(async move {
        let res = poll_github(http, r.device_code, r.user_code, interval).await;
        finish(&app2, provider, res);
    });
    Ok(info)
}

async fn poll_github(
    http: reqwest::Client,
    device_code: String,
    _user_code: String,
    interval: u64,
) -> Result<StoredCredential, String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(POLL_MAX_SECS);
    let token = loop {
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
            #[serde(default)] access_token: Option<String>,
            #[serde(default)] error: Option<String>,
        }
        let t: GhTok = resp.json().await.map_err(|e| e.to_string())?;
        if let Some(tok) = t.access_token {
            break tok;
        }
        match t.error.as_deref() {
            Some("authorization_pending") | Some("slow_down") if std::time::Instant::now() < deadline => {
                let extra = if t.error.as_deref() == Some("slow_down") { 5 } else { 0 };
                tokio::time::sleep(std::time::Duration::from_secs(interval + extra)).await;
                continue;
            }
            Some(e) => return Err(format!("github device: {e}")),
            None => return Err("github device: no token, no error".into()),
        }
    };

    // Resolve username for the label.
    let label = match http
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {token}"))
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

// ---- Gemini (Google) ----

#[derive(Deserialize)]
struct GDeviceCode {
    device_code: String,
    user_code: String,
    verification_url: String,
    #[serde(default)] interval: u64,
}
#[derive(Deserialize)]
struct GToken {
    #[serde(default)] access_token: Option<String>,
    #[serde(default)] refresh_token: Option<String>,
    #[serde(default)] expires_in: Option<u64>,
    #[serde(default)] id_token: Option<String>,
    #[serde(default)] error: Option<String>,
}

async fn start_gemini(app: AppHandle, http: reqwest::Client, provider: Provider) -> Result<LoginInfo, String> {
    let r: GDeviceCode = post_form(
        &http,
        "https://oauth2.googleapis.com/device/code",
        &[
            ("client_id", GEMINI_CID),
            ("scope", "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email"),
        ],
    )
    .await?;
    let interval = r.interval.max(5);
    let info = LoginInfo {
        provider,
        verification_url: r.verification_url.clone(),
        user_code: r.user_code.clone(),
        expires_in: 900,
    };
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let res = poll_gemini(http, r.device_code, interval).await;
        finish(&app2, provider, res);
    });
    Ok(info)
}

async fn poll_gemini(http: reqwest::Client, device_code: String, interval: u64) -> Result<StoredCredential, String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(POLL_MAX_SECS);
    let tok = loop {
        let t: GToken = post_form(
            &http,
            "https://oauth2.googleapis.com/token",
            &[
                ("client_id", GEMINI_CID),
                ("client_secret", GEMINI_CSEC),
                ("device_code", &device_code),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ],
        )
        .await?;
        if t.access_token.is_some() {
            break t;
        }
        if matches!(t.error.as_deref(), Some("authorization_pending") | Some("slow_down"))
            && std::time::Instant::now() < deadline
        {
            let extra = if t.error.as_deref() == Some("slow_down") { 5 } else { 0 };
            tokio::time::sleep(std::time::Duration::from_secs(interval + extra)).await;
            continue;
        }
        return Err(t.error.unwrap_or_else(|| "google device: no token".into()));
    };
    let label = tok
        .id_token
        .as_deref()
        .and_then(|t| jwt_payload(t).ok())
        .and_then(|c| c.get("email").and_then(|v| v.as_str()).map(String::from))
        .unwrap_or_else(|| "Gemini account".into());
    let expires_at = tok
        .expires_in
        .map(|s| chrono::Utc::now().timestamp_millis() + (s as i64) * 1000)
        .unwrap_or(0);
    Ok(StoredCredential {
        id: String::new(),
        provider: Provider::Gemini,
        label,
        access_token: tok.access_token.unwrap(),
        refresh_token: tok.refresh_token,
        expires_at,
        id_token: tok.id_token,
        account_id: None,
    })
}

fn interval_secs(v: &serde_json::Value, default: u64) -> u64 {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        .unwrap_or(default)
        .max(1)
}
