//! Browser OAuth login via a localhost callback server. The provider allows a
//! localhost redirect, so we run a local callback server and capture the
//! authorization code automatically. Used by Codex (OpenAI PKCE) and Gemini
//! (Google Authorization Code + loopback, matching gemini-cli's oauth2.ts).

mod callback_server;
mod credential;
mod exchange;
mod html;
mod pkce;
mod spec;

use std::sync::atomic::AtomicBool;

use tauri::AppHandle;
use tiny_http::Server;

use crate::model::Provider;

use callback_server::{install_cancel_flag, run_server, RunCtx, LOCAL_REDIRECT_PATH};
use pkce::{pkce_challenge, pkce_verifier, random_b64};
use spec::{build_authorize_url, spec_for, LoginMode};

pub use callback_server::cancel;

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
