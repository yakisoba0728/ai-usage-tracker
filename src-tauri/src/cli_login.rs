//! CLI-driven OAuth login (codexbar's approach): run the official CLI's login
//! command in a PTY, dismiss its interactive prompts, capture the OAuth URL it
//! prints, and wait for the CLI to confirm. The CLI does the actual OAuth
//! (handling Cloudflare) and writes a fresh token to its usual location, which
//! the provider then reads. No manual `claude`/`codex` run required.

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::model::Provider;

const TIMEOUT_SECS: u64 = 120;
const ENTER_INTERVAL_MS: u64 = 800;

/// What the frontend needs to open the browser + show the code.
#[derive(Clone, Serialize)]
pub struct CliLoginUrl {
    pub provider: Provider,
    pub url: String,
}

/// Same shape as login::LoginResult so the frontend's `login-complete` handler
/// works for both device-code and CLI flows.
#[derive(Clone, Serialize)]
struct LoginResult {
    provider: Provider,
    ok: bool,
    label: Option<String>,
    error: Option<String>,
}

struct LoginSpec {
    binary: String,
    args: Vec<String>,
    url_contains: String, // filter that picks the real auth URL (skips docs links)
    success_markers: Vec<String>,
}

fn spec_for(p: Provider) -> Option<LoginSpec> {
    match p {
        Provider::Claude => Some(LoginSpec {
            binary: "claude".into(),
            args: vec!["/login".into()],
            url_contains: "oauth/authorize".into(),
            success_markers: vec![
                "Successfully logged in".into(),
                "Logged in successfully".into(),
                "Login successful".into(),
            ],
        }),
        // Codex/Gemini/Copilot CLI logins can be added here with their commands.
        _ => None,
    }
}

pub fn supports_cli_login(p: Provider) -> bool {
    spec_for(p).is_some() && which(&spec_for(p).unwrap().binary)
}

/// Quick PATH check for a binary (no `which` shell-out).
fn which(bin: &str) -> bool {
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(':') {
            let p = std::path::Path::new(dir).join(bin);
            if p.is_file() {
                return true;
            }
        }
    }
    false
}

/// Kick off a CLI login in the background. Emits `cli-login-url` when the OAuth
/// URL is captured (frontend opens the browser) and `login-complete` on success
/// or timeout.
pub fn start(app: AppHandle, provider: Provider) {
    let Some(spec) = spec_for(provider) else {
        let _ = app.emit(
            "login-complete",
            LoginResult {
                provider,
                ok: false,
                label: None,
                error: Some("no CLI login available for this provider".into()),
            },
        );
        return;
    };
    if !which(&spec.binary) {
        let _ = app.emit(
            "login-complete",
            LoginResult {
                provider,
                ok: false,
                label: None,
                error: Some(format!("`{}` CLI not found in PATH", spec.binary)),
            },
        );
        return;
    }
    let app2 = app.clone();
    std::thread::spawn(move || run(app2, provider, spec));
}

fn run(app: AppHandle, provider: Provider, spec: LoginSpec) {
    let pair = match native_pty_system().openpty(PtySize {
        rows: 50,
        cols: 200,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(p) => p,
        Err(e) => return emit_err(&app, provider, format!("openpty: {e}")),
    };

    let mut cmd = CommandBuilder::new(&spec.binary);
    for a in &spec.args {
        cmd.arg(a);
    }
    cmd.cwd("/tmp");

    let child = match pair.slave.spawn_command(cmd) {
        Ok(c) => c,
        Err(e) => return emit_err(&app, provider, format!("spawn {}: {e}", spec.binary)),
    };
    let child: Arc<Mutex<Option<Box<dyn portable_pty::Child + Send>>>> =
        Arc::new(Mutex::new(Some(child)));
    drop(pair.slave);

    let mut writer = match pair.master.take_writer() {
        Ok(w) => w,
        Err(e) => return emit_err(&app, provider, format!("take_writer: {e}")),
    };
    let mut reader = match pair.master.try_clone_reader() {
        Ok(r) => r,
        Err(e) => return emit_err(&app, provider, format!("try_clone_reader: {e}")),
    };

    let acc: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let acc2 = acc.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if let Ok(mut s) = acc2.lock() {
                        s.push_str(&String::from_utf8_lossy(&buf[..n]));
                    }
                }
            }
        }
    });

    let start = Instant::now();
    let mut last_enter = Instant::now();
    let mut url_sent = false;
    let mut outcome: Option<LoginResult> = None;

    while start.elapsed() < Duration::from_secs(TIMEOUT_SECS) {
        std::thread::sleep(Duration::from_millis(150));
        if last_enter.elapsed() >= Duration::from_millis(ENTER_INTERVAL_MS) {
            let _ = writer.write_all(b"\r");
            let _ = writer.flush();
            last_enter = Instant::now();
        }
        let snap = acc.lock().map(|s| s.clone()).unwrap_or_default();

        if !url_sent {
            if let Some(url) = find_url(&snap, &spec.url_contains) {
                let _ = app.emit("cli-login-url", CliLoginUrl { provider, url });
                url_sent = true;
            }
        }
        for m in &spec.success_markers {
            if snap.contains(m.as_str()) {
                outcome = Some(LoginResult {
                    provider,
                    ok: true,
                    label: Some(format!("{provider:?}")),
                    error: None,
                });
                break;
            }
        }
        if outcome.is_some() {
            break;
        }
    }

    // Kill the CLI process regardless.
    if let Ok(mut guard) = child.lock() {
        if let Some(mut c) = guard.take() {
            let _ = c.kill();
        }
    }

    let result = outcome.unwrap_or_else(|| LoginResult {
        provider,
        ok: false,
        label: None,
        error: Some(format!("login timed out after {TIMEOUT_SECS}s")),
    });
    let _ = app.emit("login-complete", result);
}

fn emit_err(app: &AppHandle, provider: Provider, msg: String) {
    let _ = app.emit(
        "login-complete",
        LoginResult {
            provider,
            ok: false,
            label: None,
            error: Some(msg),
        },
    );
}

/// Find the first URL in `text` that contains `filter` (extracts up to whitespace).
fn find_url(text: &str, filter: &str) -> Option<String> {
    let mut rest = text;
    while let Some(idx) = rest.find("http") {
        let end = idx + rest[idx..].find(char::is_whitespace).unwrap_or(rest[idx..].len());
        let url = rest[idx..end].trim_end_matches(|c: char| ".,;:)]}>\"'".contains(c));
        if url.contains(filter) {
            return Some(url.to_string());
        }
        rest = &rest[end..];
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_url_filters_docs_link() {
        let text = "see https://code.claude.com/docs/en/security \n open https://claude.com/cai/oauth/authorize?state=x done";
        let url = find_url(text, "oauth/authorize").unwrap();
        assert!(url.starts_with("https://claude.com/cai/oauth/authorize"));
    }
}
