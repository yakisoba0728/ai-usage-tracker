//! The localhost callback server: the `tiny_http` receive loop that captures the
//! authorization code, the cancel lifecycle (`ACTIVE`/`install_cancel_flag`/
//! `ActiveGuard`) that lets a newer login pre-empt an in-flight one, and the
//! token-exchange → credential → `login-complete` tail.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tiny_http::{Response, Server};

use crate::model::{Provider, EVENT_LOGIN_COMPLETE};
use crate::store;

use super::credential::{build_credential, classify_callback, CallbackAction};
use super::exchange::exchange;
use super::html::{error_html, html_security_headers, success_html};

pub(super) const LOCAL_REDIRECT_PATH: &str = "/auth/callback";
const TIMEOUT_SECS: u64 = 300;

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
pub(super) fn install_cancel_flag(new: std::sync::Arc<AtomicBool>) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
