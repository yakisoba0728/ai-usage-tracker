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
    // Order MUST match [Claude, Codex, Gemini, Copilot, Cursor].
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

#[tauri::command]
pub async fn list_accounts() -> Result<Vec<crate::store::StoredCredential>, String> {
    Ok(crate::store::list())
}

#[tauri::command]
pub async fn remove_account(id: String) -> Result<bool, String> {
    Ok(crate::store::remove(&id))
}

/// CLI-driven OAuth login (codexbar style): drive the provider's CLI `/login`
/// in a PTY. Emits `cli-login-url` (frontend opens browser) + `login-complete`.
#[tauri::command]
pub async fn login_via_cli(app: AppHandle, provider: crate::model::Provider) -> Result<(), String> {
    crate::cli_login::start(app, provider);
    Ok(())
}

/// Providers that can be logged in from the app (CLI-PTY or device-code).
#[tauri::command]
pub fn login_options() -> Vec<crate::model::Provider> {
    use crate::model::Provider;
    [Provider::Claude, Provider::Codex, Provider::Gemini, Provider::Copilot]
        .into_iter()
        .filter(|p| crate::cli_login::supports_cli_login(*p) || crate::login::supports_login(*p))
        .collect()
}
