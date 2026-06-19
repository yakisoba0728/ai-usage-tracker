//! Tauri commands + the shared refresh routine + managed state stores.

use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};
use tokio::sync::RwLock;

use crate::config::AppConfig;
use crate::model::UsageSnapshot;
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
    Arc::new(RwLock::new(AppConfig::default()))
}

pub fn build_providers(cfg: &AppConfig) -> Vec<Box<dyn ProviderApi>> {
    let mut v: Vec<Box<dyn ProviderApi>> = Vec::new();
    // Order MUST match [Claude, Codex, Gemini, Copilot, Cursor, z.ai]. Local
    // parsing (keychain / credential files / env) stays for ALL providers;
    // Claude additionally supports a pasted session key via "Add account".
    if cfg.enabled[0] {
        v.push(Box::new(crate::providers::claude::ClaudeProvider::new()));
    }
    if cfg.enabled[1] {
        v.push(Box::new(crate::providers::codex::CodexProvider::new()));
    }
    if cfg.enabled[2] {
        v.push(Box::new(crate::providers::gemini::GeminiProvider::new()));
    }
    if cfg.enabled[3] {
        v.push(Box::new(crate::providers::copilot::CopilotProvider::new()));
    }
    if cfg.enabled[4] {
        v.push(Box::new(crate::providers::cursor::CursorProvider::new()));
    }
    if cfg.enabled[5] {
        v.push(Box::new(crate::providers::zai::ZaiProvider::new()));
    }
    v
}

/// Fetch every enabled provider (concurrently, isolated), store + emit the
/// snapshot, and update the tray label to the highest usage percent.
pub async fn refresh_once(app: &AppHandle, cfg: &ConfigStore, snap: &SnapshotStore) -> UsageSnapshot {
    let providers = build_providers(&*cfg.read().await);
    let mut services = fetch_all(providers).await;
    // Manually-added (OAuth) accounts from the store, in addition to auto-detected CLI ones.
    for cred in crate::store::list() {
        services.push(crate::providers::fetch_credential(&cred).await);
    }
    let snapshot = UsageSnapshot {
        fetched_at: chrono::Utc::now().timestamp(),
        services,
    };
    *snap.write().await = snapshot.clone();
    let _ = app.emit("usage-updated", &snapshot);

    let title = match snapshot.max_used_percent() {
        Some(p) => format!("{:.0}%", p.round()),
        None => "AI".to_string(),
    };
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_title(Some(&title));
    }
    snapshot
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

