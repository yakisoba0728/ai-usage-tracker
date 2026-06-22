//! The localhost callback server: the `tiny_http` receive loop that captures the
//! authorization code, and the token-exchange → credential → `login-complete`
//! tail. The cancel lifecycle (install/cancel/guard) and the completion step are
//! the shared `auth/` implementation; this module holds the OAuth callback
//! token instance (`OAUTH_ACTIVE`), independent of the device-code one.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::AppHandle;
use tiny_http::{Response, Server};

use crate::auth::cancel_token::CancelToken;
use crate::auth::finish::{emit_err, finish};
use crate::model::Provider;

use super::credential::{build_credential, classify_callback, CallbackAction};
use super::exchange::exchange;
use super::html::{error_html, html_security_headers, success_html};

pub(super) const LOCAL_REDIRECT_PATH: &str = "/auth/callback";
const TIMEOUT_SECS: u64 = 300;

/// Cancel flag for the active OAuth (LocalServer) login. Independent of the
/// device-code flow's token — `cancel_login` cancels both deliberately.
static OAUTH_ACTIVE: CancelToken = CancelToken::new();

pub fn cancel() {
    OAUTH_ACTIVE.cancel();
}

/// Install `new` as the active cancel flag, cancelling any prior in-flight login
/// first so its server thread exits promptly (freeing the callback port) instead
/// of lingering until its own timeout.
pub(super) fn install_cancel_flag(new: std::sync::Arc<AtomicBool>) {
    OAUTH_ACTIVE.install(new);
}

pub(super) struct RunCtx {
    pub(super) app: AppHandle,
    pub(super) provider: Provider,
    pub(super) client_id: String,
    pub(super) client_secret: Option<String>,
    pub(super) token_url: String,
    pub(super) redirect_uri: String,
    pub(super) verifier: String,
    pub(super) expected_state: String,
    pub(super) cancelled: std::sync::Arc<AtomicBool>,
}

pub(super) fn run_server(server: Server, ctx: RunCtx) {
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
    // Clear OAUTH_ACTIVE when this server thread exits (any return path), unless
    // a newer login has already taken over.
    let _active = OAUTH_ACTIVE.guard(cancelled.clone());
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
        Ok(t) => finish(&app, provider, build_credential(provider, &t)),
        Err(e) => emit_err(&app, provider, format!("token exchange: {e}")),
    }
}
