//! Tauri commands + the shared refresh routine + managed state stores.

use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};
use tokio::sync::RwLock;

use crate::config::AppConfig;
use crate::model::{Provider, ServiceSource, UsageSnapshot};
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

/// Auto-detect provider constructors in canonical order. The index lines up
/// positionally with `AppConfig::enabled_array()` / `PROVIDER_ORDER`, so adding
/// a provider is one row here — no hand-counted indices to keep in sync. Local
/// parsing (keychain / credential files / env) runs for ALL providers; Claude
/// additionally supports a pasted session key via "Add account".
const PROVIDER_CTORS: [fn() -> Box<dyn ProviderApi>; 6] = [
    || Box::new(crate::providers::claude::ClaudeProvider::new()),
    || Box::new(crate::providers::codex::CodexProvider::new()),
    || Box::new(crate::providers::gemini::GeminiProvider::new()),
    || Box::new(crate::providers::copilot::CopilotProvider::new()),
    || Box::new(crate::providers::cursor::CursorProvider::new()),
    || Box::new(crate::providers::zai::ZaiProvider::new()),
];

pub fn build_providers(cfg: &AppConfig) -> Vec<Box<dyn ProviderApi>> {
    cfg.enabled_array()
        .into_iter()
        .zip(PROVIDER_CTORS)
        .filter_map(|(enabled, ctor)| enabled.then(ctor))
        .collect()
}

/// Fetch every enabled provider (concurrently, isolated), store + emit the
/// snapshot.
pub async fn refresh_once(
    app: &AppHandle,
    cfg: &ConfigStore,
    snap: &SnapshotStore,
) -> UsageSnapshot {
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
    let connected: HashSet<Provider> = services
        .iter()
        .filter(|s| s.connected)
        .map(|s| s.provider)
        .collect();
    services.retain(|s| {
        s.connected || s.source != ServiceSource::Auto || !connected.contains(&s.provider)
    });
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
pub async fn start_login(
    app: AppHandle,
    provider: crate::model::Provider,
) -> Result<crate::login::LoginInfo, String> {
    crate::login::start(app, provider).await
}

/// Display-only list of stored accounts — tokens never cross IPC (P0 #1).
#[tauri::command]
pub async fn list_accounts() -> Result<Vec<crate::model::AccountInfo>, String> {
    Ok(crate::store::list()
        .into_iter()
        .map(|c| crate::model::AccountInfo {
            id: c.id,
            provider: c.provider,
            label: c.label,
        })
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
    crate::store::add(cred)
}

/// Browser + localhost-callback OAuth login (codex-switcher pattern). Returns
/// the authorize URL for the frontend to open in the browser; emits
/// `login-complete` when tokens are captured.
#[tauri::command]
pub async fn login_oauth(
    app: AppHandle,
    provider: crate::model::Provider,
) -> Result<String, String> {
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
    use crate::model::{LimitWindow, ServiceSource, ServiceUsage};

    fn svc(
        id: &str,
        provider: Provider,
        source: ServiceSource,
        connected: bool,
        err: Option<&str>,
    ) -> ServiceUsage {
        ServiceUsage {
            id: id.into(),
            source,
            provider,
            connected,
            plan: None,
            account: None,
            error: err.map(crate::model::ServiceError::code),
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
            svc(
                "auto:zai",
                Provider::Zai,
                ServiceSource::Auto,
                false,
                Some("credentials not found: z.ai API key not set"),
            ),
            svc(
                "stored:zai-1",
                Provider::Zai,
                ServiceSource::Stored,
                true,
                None,
            ),
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
            svc(
                "auto:claude",
                Provider::Claude,
                ServiceSource::Auto,
                true,
                None,
            ),
            svc(
                "stored:claude-1",
                Provider::Claude,
                ServiceSource::Stored,
                true,
                None,
            ),
            svc(
                "auto:codex",
                Provider::Codex,
                ServiceSource::Auto,
                false,
                Some("not logged in"),
            ),
        ];
        dedupe_services(&mut services);
        // Both Claudes stay; Codex failure stays (no Codex success to mask it).
        assert_eq!(services.len(), 3);
        let claude_count = services
            .iter()
            .filter(|s| s.provider == Provider::Claude)
            .count();
        assert_eq!(claude_count, 2);
        assert!(services
            .iter()
            .filter(|s| s.provider == Provider::Claude)
            .all(|s| s.connected));
    }

    #[test]
    fn dedupe_keeps_pure_failure_when_no_success() {
        // Pure failure path (no stored account) — keep the actionable error.
        let mut services = vec![svc(
            "auto:gemini",
            Provider::Gemini,
            ServiceSource::Auto,
            false,
            Some("no oauth_creds.json"),
        )];
        dedupe_services(&mut services);
        assert_eq!(services.len(), 1);
        assert!(!services[0].connected);
    }

    #[test]
    fn dedupe_keeps_stored_failure_when_autodetect_succeeds() {
        let mut services = vec![
            svc("auto:zai", Provider::Zai, ServiceSource::Auto, true, None),
            svc(
                "stored:zai-1",
                Provider::Zai,
                ServiceSource::Stored,
                false,
                Some("stored token expired"),
            ),
        ];
        dedupe_services(&mut services);
        assert_eq!(services.len(), 2);
        assert!(services.iter().any(|s| s.id == "auto:zai" && s.connected));
        assert!(services
            .iter()
            .any(|s| s.id == "stored:zai-1" && !s.connected));
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

    #[test]
    fn build_providers_respects_enabled_flags_and_canonical_order() {
        let keys = |cfg: &crate::config::AppConfig| -> Vec<Provider> {
            build_providers(cfg).iter().map(|p| p.key()).collect()
        };

        // All enabled → every provider in canonical order.
        assert_eq!(
            keys(&crate::config::AppConfig::default()),
            vec![
                Provider::Claude,
                Provider::Codex,
                Provider::Gemini,
                Provider::Copilot,
                Provider::Cursor,
                Provider::Zai,
            ],
        );

        // Disabling index 2 (Gemini) drops exactly that provider, order intact.
        let mut cfg = crate::config::AppConfig::default();
        cfg.providers[2].enabled = false;
        assert_eq!(
            keys(&cfg),
            vec![
                Provider::Claude,
                Provider::Codex,
                Provider::Copilot,
                Provider::Cursor,
                Provider::Zai,
            ],
        );
    }
}
