//! The shared login-completion step used by both login flows: take a built
//! credential (or an error), persist it via `store::add`, and emit the
//! `login-complete` event with the frozen `{provider, ok, label, error}`
//! payload. `emit_err` is the `Err(_)` shorthand.

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::model::{Provider, EVENT_LOGIN_COMPLETE};
use crate::store::{self, StoredCredential};

/// The `login-complete` event payload (frozen invariant — the frontend listens
/// on this exact shape).
#[derive(Clone, Serialize)]
struct LoginResult {
    provider: Provider,
    ok: bool,
    label: Option<String>,
    error: Option<String>,
}

/// Persist `res` (if Ok) and emit `login-complete`. On a successful store the
/// event carries `ok:true` + the account label; a store failure or an upstream
/// error emits `ok:false` + the message.
pub fn finish(app: &AppHandle, provider: Provider, res: Result<StoredCredential, String>) {
    let result = match res {
        Ok(c) => {
            let label = c.label.clone();
            match store::add(c) {
                Ok(_) => LoginResult {
                    provider,
                    ok: true,
                    label: Some(label),
                    error: None,
                },
                Err(e) => LoginResult {
                    provider,
                    ok: false,
                    label: None,
                    error: Some(format!("failed to store credential: {e}")),
                },
            }
        }
        Err(e) => LoginResult {
            provider,
            ok: false,
            label: None,
            error: Some(e),
        },
    };
    let _ = app.emit(EVENT_LOGIN_COMPLETE, &result);
}

/// Emit a `login-complete` failure for `provider` with `msg`. Equivalent to
/// `finish(app, provider, Err(msg))`.
pub fn emit_err(app: &AppHandle, provider: Provider, msg: String) {
    finish(app, provider, Err(msg));
}
