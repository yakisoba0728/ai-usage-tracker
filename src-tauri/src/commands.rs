//! Tauri commands + the shared refresh routine + managed state stores.

use std::collections::HashSet;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};
use tokio::sync::RwLock;

use crate::config::AppConfig;
use crate::model::{auto_service_id, stored_service_id, Provider, ServiceSource, UsageSnapshot};
use crate::providers::{fetch_all, ProviderApi};

pub type SnapshotStore = Arc<RwLock<UsageSnapshot>>;
pub type ConfigStore = Arc<RwLock<AppConfig>>;

static SNAPSHOT_REFRESH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
    let _guard = SNAPSHOT_REFRESH_LOCK.lock().await;
    refresh_once_inner(app, cfg, snap).await
}

async fn refresh_once_inner(
    app: &AppHandle,
    cfg: &ConfigStore,
    snap: &SnapshotStore,
) -> UsageSnapshot {
    let providers = build_providers(&*cfg.read().await);
    for p in &providers {
        emit_loading(app, &auto_service_id(p.key()), p.key());
    }
    let stored = crate::store::list();
    for cred in &stored {
        emit_loading(app, &stored_service_id(&cred.id), cred.provider);
    }
    let mut services = fetch_all(providers).await;
    let stored_results = futures::future::join_all(stored.iter().map(|cred| async move {
        (
            stored_service_id(&cred.id),
            crate::providers::fetch_credential(cred).await,
        )
    }))
    .await;
    let active_stored = active_stored_service_ids();
    for (id, service) in stored_results {
        if active_stored.contains(&id) {
            services.push(service);
        }
    }
    filter_deleted_stored_services(&mut services, &active_stored);
    // If a stored account connected for a provider, drop the auto-detect
    // failure for the same provider (the user's explicit Add-account wins).
    dedupe_services(&mut services);

    let snapshot = UsageSnapshot {
        fetched_at: chrono::Utc::now().timestamp(),
        services,
    };
    *snap.write().await = snapshot.clone();
    let _ = app.emit("usage-updated", &snapshot);
    // Auto window-anchoring: for each opted-in, connected, supported service
    // whose 5-hour window is empty (100% remaining), send one anchor message.
    let now_sec = chrono::Utc::now().timestamp();
    let auto = cfg.read().await.auto_anchor.clone();
    for s in &snapshot.services {
        if auto.get(&s.id).copied().unwrap_or(false)
            && s.connected
            && crate::anchor::supported(s.provider)
            && five_hour_used(s) == Some(0.0)
            && crate::anchor::try_begin(&s.id, now_sec)
        {
            let id = s.id.clone();
            let app2 = app.clone();
            tauri::async_runtime::spawn(async move {
                let res = crate::anchor::send(&id).await;
                match &res {
                    Ok(()) => eprintln!("anchor auto-fired {id}: ok"),
                    Err(e) => {
                        eprintln!("anchor auto-fired {id}: err: {e}");
                        // Roll back the cooldown ONLY for transient (network)
                        // failures so a quick retry is allowed. A durable
                        // rejection (e.g. a retired model → 4xx) keeps the
                        // cooldown so we don't fire one detached task every poll
                        // forever against a failing provider (B-3 / B-13).
                        if crate::anchor::failure_is_transient(e) {
                            crate::anchor::clear(&id);
                        }
                    }
                }
                let _ = app2.emit(
                    "anchor-result",
                    serde_json::json!({
                        "id": id,
                        "ok": res.is_ok(),
                        "detail": res.as_ref().err().map(|e| e.to_string()),
                    }),
                );
            });
        }
    }
    // Tray shows the app icon only — no title text (per user request).
    snapshot
}

/// The `used_percent` of the provider's 5-hour window (card or detail list).
fn five_hour_used(s: &crate::model::ServiceUsage) -> Option<f32> {
    s.windows
        .iter()
        .chain(s.detail_windows.iter())
        .find(|w| w.label == "5-hour")
        .and_then(|w| w.used_percent)
}

/// Drop disconnected entries for any provider that also has a connected entry.
/// Keeps all connected entries (multi-account support); only suppresses the
/// redundant "auto-detect failed" error when a stored account succeeded.
fn dedupe_services(services: &mut Vec<crate::model::ServiceUsage>) {
    let connected: HashSet<Provider> = services
        .iter()
        .filter(|s| s.connected)
        .map(|s| s.provider)
        .collect();
    services.retain(|s| {
        s.connected || s.source != ServiceSource::Auto || !connected.contains(&s.provider)
    });
}

fn active_stored_service_ids() -> HashSet<String> {
    crate::store::list()
        .into_iter()
        .map(|c| stored_service_id(&c.id))
        .collect()
}

fn filter_deleted_stored_services(
    services: &mut Vec<crate::model::ServiceUsage>,
    active_stored: &HashSet<String>,
) {
    services.retain(|s| s.source != ServiceSource::Stored || active_stored.contains(&s.id));
}

fn loading_payload(id: &str, provider: Provider) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "provider": provider,
    })
}

fn emit_loading(app: &AppHandle, id: &str, provider: Provider) {
    let _ = app.emit("provider-loading", loading_payload(id, provider));
}

fn refresh_result_payload(
    service_id: &str,
    result: Result<&crate::model::ServiceUsage, &String>,
) -> serde_json::Value {
    let (ok, detail) = match result {
        Ok(service) if service.connected => (true, None),
        Ok(service) => (
            false,
            service
                .error
                .as_ref()
                .map(|e| e.detail.clone().unwrap_or_else(|| e.code.clone())),
        ),
        Err(err) => (false, Some(err.clone())),
    };
    serde_json::json!({
        "id": service_id,
        "ok": ok,
        "detail": detail,
    })
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
    new.save()
        .map_err(|e| format!("could not save config: {e}"))?;
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
    crate::store::remove(&id)
}

/// Add an account by pasting a raw credential (Claude session key, or any
/// provider's access/session token). No OAuth flow involved.
#[tauri::command]
pub async fn add_session_key(
    provider: crate::model::Provider,
    key: String,
    label: Option<String>,
) -> Result<String, String> {
    let access_token = match provider {
        crate::model::Provider::Claude => crate::providers::claude::normalize_session_key(&key),
        _ => key.trim().to_string(),
    };
    let cred = crate::store::StoredCredential {
        id: String::new(),
        provider,
        label: label.unwrap_or_else(|| format!("{provider:?}")),
        access_token,
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
    crate::login::cancel();
}

/// Send a minimal "anchor" message for one service to start its usage window.
/// Tokens stay in Rust; the frontend only passes the masked service id.
/// Emits `anchor-result` so the frontend can toast the outcome.
#[tauri::command]
pub async fn send_anchor_now(app: AppHandle, service_id: String) -> Result<(), String> {
    let res = crate::anchor::send(&service_id).await;
    let _ = app.emit(
        "anchor-result",
        serde_json::json!({
            "id": service_id,
            "ok": res.is_ok(),
            "detail": res.as_ref().err().map(|e| e.to_string()),
        }),
    );
    res.map_err(|e| e.to_string())
}

/// Re-fetch a single account (auto:<provider> or stored:<id>) and merge it into
/// the snapshot. Emits provider-loading then usage-updated, and a `refresh-result`
/// on EVERY path so a failure is surfaced instead of silently dropped (F-7).
#[tauri::command]
pub async fn refresh_account(
    app: AppHandle,
    cfg: State<'_, ConfigStore>,
    snap: State<'_, SnapshotStore>,
    service_id: String,
) -> Result<(), String> {
    let result = refresh_account_once(&app, cfg.inner(), snap.inner(), &service_id).await;
    let _ = app.emit(
        "refresh-result",
        refresh_result_payload(&service_id, result.as_ref()),
    );
    result.map(|_| ())
}

async fn refresh_account_once(
    app: &AppHandle,
    cfg: &ConfigStore,
    snap: &SnapshotStore,
    service_id: &str,
) -> Result<crate::model::ServiceUsage, String> {
    let _guard = SNAPSHOT_REFRESH_LOCK.lock().await;
    refresh_account_once_inner(app, cfg, snap, service_id).await
}

async fn refresh_account_once_inner(
    app: &AppHandle,
    cfg: &ConfigStore,
    snap: &SnapshotStore,
    service_id: &str,
) -> Result<crate::model::ServiceUsage, String> {
    // Resolve the fresh ServiceUsage for this id.
    let fresh: Option<crate::model::ServiceUsage> = if service_id.starts_with("stored:") {
        let cred = crate::store::find_by_service_id(service_id);
        match cred {
            Some(c) => {
                emit_loading(app, service_id, c.provider);
                let fresh = crate::providers::fetch_credential(&c).await;
                if crate::store::find(&c.id).is_none() {
                    remove_service_from_snapshot(app, snap, service_id).await;
                    return Err(format!("account removed while refreshing: {service_id}"));
                }
                Some(fresh)
            }
            None => None,
        }
    } else {
        // auto:<provider> — build that one provider from the enabled set and fetch it.
        let providers = build_providers(&*cfg.read().await);
        let mut found = None;
        for p in providers {
            if auto_service_id(p.key()) == service_id {
                emit_loading(app, service_id, p.key());
                let mut batch = crate::providers::fetch_all(vec![p]).await;
                found = batch.pop();
                break;
            }
        }
        found
    };
    let Some(fresh) = fresh else {
        return Err(format!("unknown or disabled account: {service_id}"));
    };
    // Merge: replace the same-id entry (or push if absent), then emit.
    let returned = fresh.clone();
    {
        let mut guard = snap.write().await;
        if let Some(slot) = guard.services.iter_mut().find(|s| s.id == fresh.id) {
            *slot = fresh;
        } else {
            guard.services.push(fresh);
        }
        guard.fetched_at = chrono::Utc::now().timestamp();
    }
    let snapshot = snap.read().await.clone();
    let _ = app.emit("usage-updated", &snapshot);
    Ok(returned)
}

async fn remove_service_from_snapshot(app: &AppHandle, snap: &SnapshotStore, service_id: &str) {
    {
        let mut guard = snap.write().await;
        guard.services.retain(|s| s.id != service_id);
        guard.fetched_at = chrono::Utc::now().timestamp();
    }
    let snapshot = snap.read().await.clone();
    let _ = app.emit("usage-updated", &snapshot);
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

    #[test]
    fn filter_deleted_stored_services_drops_missing_stored_accounts() {
        let mut services = vec![
            svc(
                "auto:codex",
                Provider::Codex,
                ServiceSource::Auto,
                true,
                None,
            ),
            svc(
                "stored:kept",
                Provider::Claude,
                ServiceSource::Stored,
                true,
                None,
            ),
            svc(
                "stored:deleted",
                Provider::Claude,
                ServiceSource::Stored,
                true,
                None,
            ),
        ];
        let current = std::collections::HashSet::from(["stored:kept".to_string()]);
        filter_deleted_stored_services(&mut services, &current);

        let ids: Vec<&str> = services.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["auto:codex", "stored:kept"]);
    }

    #[test]
    fn refresh_result_payload_marks_disconnected_usage_failed() {
        let disconnected = svc(
            "stored:claude-1",
            Provider::Claude,
            ServiceSource::Stored,
            false,
            Some("not_logged_in"),
        );

        let payload = refresh_result_payload("stored:claude-1", Ok(&disconnected));

        assert_eq!(payload["id"], "stored:claude-1");
        assert_eq!(payload["ok"], false);
        assert_eq!(payload["detail"], "not_logged_in");
    }

    #[test]
    fn loading_payload_targets_exact_service_id() {
        let payload = loading_payload("stored:claude-work", Provider::Claude);

        assert_eq!(payload["id"], "stored:claude-work");
        assert_eq!(payload["provider"], "claude");
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
    fn five_hour_used_reads_the_5h_window_from_either_list() {
        let mk = |label: &str, p: f32| crate::model::LimitWindow {
            label: label.into(),
            used_percent: Some(p),
            resets_at: None,
            used: None,
            limit: None,
        };
        let mut s = svc("auto:zai", Provider::Zai, ServiceSource::Auto, true, None);
        s.windows = vec![mk("Weekly", 70.0), mk("5-hour", 0.0)];
        assert_eq!(super::five_hour_used(&s), Some(0.0));
        let mut s2 = svc(
            "auto:claude",
            Provider::Claude,
            ServiceSource::Auto,
            true,
            None,
        );
        s2.detail_windows = vec![mk("5-hour", 12.0)];
        assert_eq!(super::five_hour_used(&s2), Some(12.0));
        let s3 = svc(
            "auto:cursor",
            Provider::Cursor,
            ServiceSource::Auto,
            true,
            None,
        );
        assert_eq!(super::five_hour_used(&s3), None);
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

    #[test]
    fn add_claude_session_key_persists_only_session_key_cookie_value() {
        let _guard = crate::store::STORE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let path = std::env::temp_dir().join(format!(
            "ait_commands_session_cookie_{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        std::env::set_var("AIT_ACCOUNTS_PATH", &path);

        let id = tauri::async_runtime::block_on(add_session_key(
            Provider::Claude,
            "Cookie: other=not-this; sessionKey=sk-ant-sid01-session; x=also-not-this".into(),
            Some("Claude web".into()),
        ))
        .unwrap();

        let loaded = crate::store::load();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, id);
        assert_eq!(loaded[0].access_token, "sk-ant-sid01-session");
        assert!(!loaded[0].access_token.contains("other=not-this"));
        assert!(!loaded[0].access_token.contains("also-not-this"));

        let _ = crate::store::remove(&id);
        std::env::remove_var("AIT_ACCOUNTS_PATH");
        let _ = std::fs::remove_file(&path);
    }
}
