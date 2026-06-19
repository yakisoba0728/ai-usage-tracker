//! Tauri commands + the shared refresh routine + managed state stores.

use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};
use tokio::sync::RwLock;

use crate::config::AppConfig;
use crate::model::{Provider, UsageSnapshot};
use crate::providers::{fetch_all, ProviderApi};

pub type SnapshotStore = Arc<RwLock<UsageSnapshot>>;
pub type ConfigStore = Arc<RwLock<AppConfig>>;

pub fn empty_snapshot_store() -> SnapshotStore {
    Arc::new(RwLock::new(UsageSnapshot {
        fetched_at: 0,
        services: vec![],
    }))
}

pub fn default_config_store() -> ConfigStore {
    Arc::new(RwLock::new(AppConfig::load()))
}

pub fn build_providers(cfg: &AppConfig) -> Vec<Box<dyn ProviderApi>> {
    let mut v: Vec<Box<dyn ProviderApi>> = Vec::new();
    // Order MUST match [Claude, Codex, Gemini, Copilot, Cursor, z.ai]. Local
    // parsing (keychain / credential files / env) stays for ALL providers;
    // Claude additionally supports a pasted session key via "Add account".
    if cfg.enabled_array()[0] {
        v.push(Box::new(crate::providers::claude::ClaudeProvider::new()));
    }
    if cfg.enabled_array()[1] {
        v.push(Box::new(crate::providers::codex::CodexProvider::new()));
    }
    if cfg.enabled_array()[2] {
        v.push(Box::new(crate::providers::gemini::GeminiProvider::new()));
    }
    if cfg.enabled_array()[3] {
        v.push(Box::new(crate::providers::copilot::CopilotProvider::new()));
    }
    if cfg.enabled_array()[4] {
        v.push(Box::new(crate::providers::cursor::CursorProvider::new()));
    }
    if cfg.enabled_array()[5] {
        v.push(Box::new(crate::providers::zai::ZaiProvider::new()));
    }
    v
}

/// Fetch every enabled provider (concurrently, isolated), store + emit the
/// snapshot.
pub async fn refresh_once(app: &AppHandle, cfg: &ConfigStore, snap: &SnapshotStore) -> UsageSnapshot {
    let providers = build_providers(&*cfg.read().await);
    // Emit per-provider loading events so the frontend can show a shimmer on
    // each card independently.
    for p in &providers {
        let _ = app.emit("provider-loading", p.key());
    }
    let stored = crate::store::list();
    for cred in &stored {
        let _ = app.emit("provider-loading", cred.provider);
    }
    let mut services = fetch_all(providers).await;
    // Manually-added (OAuth / API-key) accounts from the store, in addition
    // to auto-detected CLI ones.
    for cred in &stored {
        services.push(crate::providers::fetch_credential(cred).await);
    }
    // If a stored account connected for a provider, drop the auto-detect
    // failure for the same provider (the user's explicit Add-account wins).
    dedupe_services(&mut services);

    let snapshot = UsageSnapshot {
        fetched_at: chrono::Utc::now().timestamp(),
        services,
    };
    *snap.write().await = snapshot.clone();
    let _ = app.emit("usage-updated", &snapshot);
    // Tray shows the app icon only — no title text (per user request).
    snapshot
}

/// Drop disconnected entries for any provider that also has a connected entry.
/// Keeps all connected entries (multi-account support); only suppresses the
/// redundant "auto-detect failed" error when a stored account succeeded.
fn dedupe_services(services: &mut Vec<crate::model::ServiceUsage>) {
    use std::collections::HashSet;
    let connected: HashSet<Provider> = services.iter().filter(|s| s.connected).map(|s| s.provider).collect();
    services.retain(|s| s.connected || !connected.contains(&s.provider));
}

#[tauri::command]
pub async fn get_usage(snap: State<'_, SnapshotStore>) -> Result<UsageSnapshot, String> {
    Ok(snap.read().await.clone())
}

#[tauri::command]
pub async fn refresh_now(
    app: AppHandle,
    cfg: State<'_, ConfigStore>,
    snap: State<'_, SnapshotStore>,
) -> Result<UsageSnapshot, String> {
    Ok(refresh_once(&app, &cfg.inner().clone(), &snap.inner().clone()).await)
}

#[tauri::command]
pub async fn get_config(cfg: State<'_, ConfigStore>) -> Result<AppConfig, String> {
    Ok(cfg.read().await.clone())
}

#[tauri::command]
pub async fn set_config(
    app: AppHandle,
    cfg: State<'_, ConfigStore>,
    new: AppConfig,
) -> Result<(), String> {
    new.validate().map_err(|e| e.to_string())?;
    let poll = new.poll_seconds;
    new.save();
    *cfg.write().await = new;
    crate::scheduler::restart(&app, poll);
    Ok(())
}

#[tauri::command]
pub async fn start_login(app: AppHandle, provider: crate::model::Provider) -> Result<crate::login::LoginInfo, String> {
    crate::login::start(app, provider).await
}

/// Display-only list of stored accounts — tokens never cross IPC (P0 #1).
#[tauri::command]
pub async fn list_accounts() -> Result<Vec<crate::model::AccountInfo>, String> {
    Ok(crate::store::list()
        .into_iter()
        .map(|c| crate::model::AccountInfo { id: c.id, provider: c.provider, label: c.label })
        .collect())
}

#[tauri::command]
pub async fn remove_account(id: String) -> Result<bool, String> {
    Ok(crate::store::remove(&id))
}

/// Add an account by pasting a raw credential (Claude session key, or any
/// provider's access/session token). No OAuth flow involved.
#[tauri::command]
pub async fn add_session_key(
    provider: crate::model::Provider,
    key: String,
    label: Option<String>,
) -> Result<String, String> {
    let cred = crate::store::StoredCredential {
        id: String::new(),
        provider,
        label: label.unwrap_or_else(|| format!("{provider:?}")),
        access_token: key,
        refresh_token: None,
        expires_at: 0,
        id_token: None,
        account_id: None,
    };
    Ok(crate::store::add(cred))
}

/// Browser + localhost-callback OAuth login (codex-switcher pattern). Returns
/// the authorize URL for the frontend to open in the browser; emits
/// `login-complete` when tokens are captured.
#[tauri::command]
pub async fn login_oauth(app: AppHandle, provider: crate::model::Provider) -> Result<String, String> {
    crate::oauth_login::start(app, provider)
}

/// Cancel the in-progress OAuth login (closes the local callback server).
#[tauri::command]
pub fn cancel_login() {
    crate::oauth_login::cancel();
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LimitWindow, ServiceUsage};

    fn svc(provider: Provider, connected: bool, err: Option<&str>) -> ServiceUsage {
        ServiceUsage {
                    provider,
                    connected,
                    plan: None,
                    account: None,
                    error: err.map(String::from),
                    windows: vec![],
                    detail_windows: vec![],
                    raw_response: None,
                }
    }

    #[test]
    fn dedupe_drops_failed_autodetect_when_stored_succeeds() {
        // z.ai: auto-detect (env) failed + stored account succeeded → keep only
        // the stored success. (The classic "key not set" bug.)
        let mut services = vec![
            svc(Provider::Zai, false, Some("credentials not found: z.ai API key not set")),
            svc(Provider::Zai, true, None),
        ];
        dedupe_services(&mut services);
        assert_eq!(services.len(), 1);
        assert!(services[0].connected);
        assert_eq!(services[0].provider, Provider::Zai);
    }

    #[test]
    fn dedupe_keeps_all_connected_for_multi_account() {
        // Two distinct connected Claude accounts (CLI + pasted session) both stay.
        // An UNRELATED provider's failure (no success for it) also stays so the
        // user still sees the actionable error.
        let mut services = vec![
            svc(Provider::Claude, true, None),
            svc(Provider::Claude, true, None),
            svc(Provider::Codex, false, Some("not logged in")),
        ];
        dedupe_services(&mut services);
        // Both Claudes stay; Codex failure stays (no Codex success to mask it).
        assert_eq!(services.len(), 3);
        let claude_count = services.iter().filter(|s| s.provider == Provider::Claude).count();
        assert_eq!(claude_count, 2);
        assert!(services.iter().filter(|s| s.provider == Provider::Claude).all(|s| s.connected));
    }

    #[test]
    fn dedupe_keeps_pure_failure_when_no_success() {
        // Pure failure path (no stored account) — keep the actionable error.
        let mut services = vec![svc(Provider::Gemini, false, Some("no oauth_creds.json"))];
        dedupe_services(&mut services);
        assert_eq!(services.len(), 1);
        assert!(!services[0].connected);
    }

    // Silence unused-field warning (LimitWindow is required by the struct but
    // the test helper doesn't use it directly).
    #[test]
    fn _limitwindow_type_is_used() {
        let _ = LimitWindow {
            label: String::new(),
            used_percent: None,
            resets_at: None,
            used: None,
            limit: None,
        };
    }
}
