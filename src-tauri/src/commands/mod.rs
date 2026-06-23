//! Tauri command surface + the shared refresh routine + managed state stores.
//!
//! The 15 `#[tauri::command]` handlers stay HERE in `commands/mod.rs` so the
//! companion `__cmd__<name>` macros `generate_handler!` expands to remain
//! reachable at `crate::commands::<name>` — `lib.rs`'s invoke list and the
//! `model.rs` catalog-freeze test reference these by that path unchanged. The
//! engine they delegate to is split into siblings: pure merge algebra in
//! `snapshot_merge`, the refresh pipeline in `refresh`, and the managed-state
//! types + provider table in `state`.

mod refresh;
mod snapshot_merge;
mod state;

use tauri::{AppHandle, Emitter, State};

use crate::config::AppConfig;
use crate::model::{
    auto_service_id, Provider, UsageSnapshot, EVENT_ANCHOR_RESULT, EVENT_REFRESH_RESULT,
};

// Re-export the pieces external modules (`scheduler`, `update`) and the
// catalog-freeze test reach via `crate::commands::…`, so their paths are
// unchanged after the split.
pub use refresh::{build_providers, refresh_once};
pub use state::{default_config_store, empty_snapshot_store, ConfigStore, SnapshotStore};

use refresh::{refresh_account_once, show_anchor_notification};
use snapshot_merge::{anchor_result_payload, refresh_result_payload};

/// Resolve the account display label for a service id, preferring the live
/// snapshot reading (`s.account`), then a stored credential's label, else None.
/// Never returns a raw `stored:<uuid>` id (finding notif-missing-account-identity).
async fn resolve_anchor_label(snap: &SnapshotStore, service_id: &str) -> Option<String> {
    if let Some(account) = snap
        .read()
        .await
        .services
        .iter()
        .find(|s| s.id == service_id)
        .and_then(|s| s.account.clone())
    {
        return Some(account);
    }
    crate::store::find_by_service_id(service_id).map(|c| c.label)
}

/// The provider for a service id: from the `auto:<provider>` prefix, or the
/// stored credential's provider. Used at the manual emit site, where we only
/// have the masked id.
fn provider_for_service_id(service_id: &str) -> Option<Provider> {
    for p in crate::model::PROVIDER_ORDER {
        if auto_service_id(p) == service_id {
            return Some(p);
        }
    }
    crate::store::find_by_service_id(service_id).map(|c| c.provider)
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

/// Carry backend-managed fields from the `current` config into an `incoming`
/// one that omits them. The UI never reads or writes `device_id` (FEAT-2) or
/// `last_notified_version` (FEAT-5) — both are `skip_serializing_if=None`, so if
/// the frontend reconstructs `AppConfig` from state and ever drops them, a blind
/// `set_config` overwrite would wipe them: regenerating a NEW device_id on the
/// next anchor (breaking "stable per-install") or re-notifying an old release.
/// Pure + unit-tested so `set_config` can stay a thin command shell.
fn preserve_backend_managed_fields(incoming: &mut AppConfig, current: &AppConfig) {
    incoming.device_id = current.device_id.clone();
    incoming.last_notified_version = current.last_notified_version.clone();
}

#[tauri::command]
pub async fn set_config(
    app: AppHandle,
    cfg: State<'_, ConfigStore>,
    mut new: AppConfig,
) -> Result<(), String> {
    new.validate().map_err(|e| e.to_string())?;
    // Preserve backend-managed fields the UI never writes (FEAT-2/FEAT-5) — see
    // `preserve_backend_managed_fields`.
    {
        let mut current = cfg.write().await;
        preserve_backend_managed_fields(&mut new, &current);
        let poll = new.poll_seconds;
        new.save()
            .map_err(|e| format!("could not save config: {e}"))?;
        *current = new;
        drop(current);
        crate::scheduler::restart(&app, poll);
    }
    Ok(())
}

/// Apply a per-account rename to a config in place: set/clear
/// `accounts[service_id].custom_name`, dropping a now-empty entry. Pure logic,
/// extracted so it's unit-testable without constructing a Tauri `State` (the
/// BUG-2 per-account isolation is proven against THIS function).
pub fn apply_account_rename(cfg: &mut AppConfig, service_id: &str, name: Option<String>) {
    let trimmed = name.and_then(|n| {
        let t = n.trim().to_string();
        (!t.is_empty()).then_some(t)
    });
    let entry = cfg.accounts.entry(service_id.to_string()).or_default();
    entry.custom_name = trimmed;
    // Drop a now-empty entry so we don't accumulate blank placeholders.
    if entry.custom_name.is_none() && entry.primary_window.is_none() {
        cfg.accounts.remove(service_id);
    }
}

/// Rename a single account by its service id (`auto:<provider>` / `stored:<id>`).
/// Writes `accounts[service_id].custom_name` in the config and persists — this
/// is PER-ACCOUNT (BUG-2 fix), so renaming one account never touches another of
/// the same provider. `name = None` (or blank) clears the override back to the
/// canonical provider label. Does NOT touch the poll scheduler.
#[tauri::command]
pub async fn rename_account(
    cfg: State<'_, ConfigStore>,
    service_id: String,
    name: Option<String>,
) -> Result<(), String> {
    let mut guard = cfg.write().await;
    apply_account_rename(&mut guard, &service_id, name);
    guard
        .save()
        .map_err(|e| format!("could not save config: {e}"))?;
    Ok(())
}

/// Manually check for a newer GitHub release NOW (the "Check for updates"
/// button, FEAT-5). Runs regardless of the `auto_update_check` toggle (`force =
/// true`) and always notifies for a newer release. Returns `Some({version,
/// html_url})` when an update is available so the UI can show it, `None` when
/// already up to date. A check failure (offline / rate-limited) surfaces as a
/// command error string rather than panicking.
#[tauri::command]
pub async fn check_update_now(
    app: AppHandle,
    cfg: State<'_, ConfigStore>,
) -> Result<Option<crate::update::AvailableUpdate>, String> {
    crate::update::run_update_check(&app, &cfg.inner().clone(), true).await
}

/// Enable/disable launch-at-login (FEAT-4) AND persist the intent. Toggles the
/// OS login item via the autostart plugin's `autolaunch()` manager, then writes
/// `launch_at_login` to the config so `.setup` can reconcile a manually-removed
/// item on the next start. An OS-side failure is returned as an error (the
/// config is NOT written in that case, so the flag never claims a state the OS
/// didn't accept).
#[tauri::command]
pub async fn set_launch_at_login(
    app: AppHandle,
    cfg: State<'_, ConfigStore>,
    enable: bool,
) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    if enable {
        manager
            .enable()
            .map_err(|e| format!("could not enable launch at login: {e}"))?;
    } else {
        manager
            .disable()
            .map_err(|e| format!("could not disable launch at login: {e}"))?;
    }
    let mut guard = cfg.write().await;
    guard.launch_at_login = enable;
    guard
        .save()
        .map_err(|e| format!("could not save config: {e}"))?;
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
    Ok(crate::store::try_list()?
        .into_iter()
        .map(|c| crate::model::AccountInfo {
            id: c.id,
            provider: c.provider,
            label: c.label,
        })
        .collect())
}

fn prune_removed_account_config(cfg: &mut AppConfig, id: &str) {
    let service_id = crate::model::stored_service_id(id);
    cfg.accounts.remove(&service_id);
    cfg.auto_anchor.remove(&service_id);
}

#[tauri::command]
pub async fn remove_account(cfg: State<'_, ConfigStore>, id: String) -> Result<bool, String> {
    remove_account_inner(cfg.inner(), id).await
}

async fn remove_account_inner(cfg: &ConfigStore, id: String) -> Result<bool, String> {
    let removed = crate::store::remove(&id)?;
    if removed {
        // Drop the per-account entries the add path mints so the maps don't grow
        // forever across add/remove churn (UG-1/UG-2). Refresh lock is keyed by
        // the RAW id; the anchor cooldown by the `stored:<id>` service-id form.
        crate::providers::forget_stored_refresh_lock(&id);
        let service_id = crate::model::stored_service_id(&id);
        crate::anchor::clear(&service_id);
        let mut guard = cfg.write().await;
        prune_removed_account_config(&mut guard, &id);
        if let Err(e) = guard.save() {
            eprintln!(
                "config: removed account {service_id}, but failed to prune stale settings: {e}"
            );
        }
    }
    Ok(removed)
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
    if access_token.trim().is_empty() {
        return Err("credential is blank".into());
    }
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
    crate::oauth::start(app, provider)
}

/// Cancel the in-progress OAuth login (closes the local callback server).
#[tauri::command]
pub fn cancel_login() {
    crate::oauth::cancel();
    crate::login::cancel();
}

/// Resolve the stable per-install `device_id` from the managed config (the
/// `anthropic-device-id` for the Claude web anchor). It is ensured at startup,
/// so the read lock almost always hits; the write-lock `ensure` is a defensive
/// fallback (e.g. a config reset between setup and the first anchor).
pub(crate) async fn device_id_for_anchor(cfg: &ConfigStore) -> String {
    if let Some(id) = cfg
        .read()
        .await
        .device_id
        .clone()
        .filter(|s| !s.trim().is_empty())
    {
        return id;
    }
    cfg.write().await.ensure_device_id()
}

/// Send a minimal "anchor" message for one service to start its usage window.
/// Tokens stay in Rust; the frontend only passes the masked service id.
/// Emits `anchor-result` (enriched with provider+label) so the frontend can
/// toast the outcome, and fires an OS-native notification (works with the
/// window hidden). This is the MANUAL path (`is_auto = false`).
#[tauri::command]
pub async fn send_anchor_now(
    app: AppHandle,
    snap: State<'_, SnapshotStore>,
    cfg: State<'_, ConfigStore>,
    service_id: String,
) -> Result<(), String> {
    let device_id = device_id_for_anchor(cfg.inner()).await;
    let res = crate::anchor::send(&service_id, &device_id).await;
    let ok = res.is_ok();
    // Source provider + label from the masked id (no full ServiceUsage here):
    // provider from the id prefix / stored cred; label from the snapshot reading
    // or the stored credential, never the raw id.
    let provider = provider_for_service_id(&service_id);
    let label = resolve_anchor_label(snap.inner(), &service_id).await;
    if let Some(provider) = provider {
        show_anchor_notification(&app, provider, label.as_deref(), ok, false);
    }
    let _ = app.emit(
        EVENT_ANCHOR_RESULT,
        anchor_result_payload(
            &service_id,
            ok,
            res.as_ref().err().map(|e| e.to_string()),
            provider,
            label.as_deref(),
        ),
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
        EVENT_REFRESH_RESULT,
        refresh_result_payload(&service_id, result.as_ref()),
    );
    result.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvVarGuard {
        key: &'static str,
        old: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &std::path::Path) -> Self {
            let old = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, old }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.old {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn temp_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("ait_commands_{tag}_{}.json", std::process::id()))
    }

    #[test]
    fn provider_for_service_id_resolves_auto_prefix() {
        assert_eq!(
            provider_for_service_id("auto:claude"),
            Some(Provider::Claude)
        );
        assert_eq!(provider_for_service_id("auto:zai"), Some(Provider::Zai));
        assert_eq!(provider_for_service_id("auto:codex"), Some(Provider::Codex));
        // An unknown / malformed id with no matching stored cred → None.
        assert_eq!(provider_for_service_id("auto:nope"), None);
        assert_eq!(provider_for_service_id("garbage"), None);
    }

    #[test]
    fn apply_account_rename_is_per_service_id_isolated() {
        // The command's core mutation: renaming one Claude account must NOT touch
        // another Claude account of the same provider (BUG-2 isolation).
        let mut cfg = AppConfig::default();

        apply_account_rename(&mut cfg, "auto:claude", Some("Personal".into()));
        apply_account_rename(&mut cfg, "stored:abc", Some("Work".into()));
        // Rename the first again — the second must be unaffected.
        apply_account_rename(&mut cfg, "auto:claude", Some("Personal v2".into()));

        assert_eq!(
            cfg.accounts
                .get("auto:claude")
                .unwrap()
                .custom_name
                .as_deref(),
            Some("Personal v2")
        );
        assert_eq!(
            cfg.accounts
                .get("stored:abc")
                .unwrap()
                .custom_name
                .as_deref(),
            Some("Work"),
            "renaming auto:claude must not touch stored:abc"
        );

        // Clearing the name (None / blank) drops the now-empty entry.
        apply_account_rename(&mut cfg, "auto:claude", None);
        assert!(!cfg.accounts.contains_key("auto:claude"));
        apply_account_rename(&mut cfg, "stored:abc", Some("   ".into()));
        assert!(
            !cfg.accounts.contains_key("stored:abc"),
            "a whitespace-only name clears the override"
        );
    }

    #[test]
    fn apply_account_rename_preserves_a_pinned_window() {
        // Clearing the name must NOT delete the entry when a primary_window is
        // still pinned on it.
        let mut cfg = AppConfig::default();
        cfg.accounts.insert(
            "auto:codex".into(),
            crate::config::AccountConfig {
                custom_name: Some("Old".into()),
                primary_window: Some("Weekly".into()),
            },
        );
        apply_account_rename(&mut cfg, "auto:codex", None);
        let entry = cfg
            .accounts
            .get("auto:codex")
            .expect("entry survives because a window is still pinned");
        assert!(entry.custom_name.is_none());
        assert_eq!(entry.primary_window.as_deref(), Some("Weekly"));
    }

    /// `set_config`'s core guard: a frontend save that omits the backend-managed
    /// `device_id` (FEAT-2) / `last_notified_version` (FEAT-5) must NOT wipe them
    /// — they are carried over from the current config. A device_id wipe would
    /// regenerate a new id on the next anchor, breaking "stable per-install".
    #[test]
    fn preserve_backend_managed_fields_carries_over_when_incoming_omits() {
        let current = AppConfig {
            device_id: Some("dev-stable-uuid".into()),
            last_notified_version: Some("0.3.0".into()),
            ..Default::default()
        };
        // The UI reconstructs a config WITHOUT these (it "never writes" them).
        let mut incoming = AppConfig {
            poll_seconds: 450,
            device_id: None,
            last_notified_version: None,
            ..Default::default()
        };
        preserve_backend_managed_fields(&mut incoming, &current);
        assert_eq!(
            incoming.device_id.as_deref(),
            Some("dev-stable-uuid"),
            "device_id must survive a UI save that omits it"
        );
        assert_eq!(
            incoming.last_notified_version.as_deref(),
            Some("0.3.0"),
            "last_notified_version must survive a UI save that omits it"
        );
        // The UI's real edits (poll_seconds) are untouched.
        assert_eq!(incoming.poll_seconds, 450);
    }

    /// Frontend-provided backend-managed values must never win: even if a stale
    /// UI payload happens to include `device_id` / `last_notified_version`, the
    /// backend's current values remain authoritative.
    #[test]
    fn preserve_backend_managed_fields_ignores_incoming_backend_values() {
        let current = AppConfig {
            device_id: Some("old".into()),
            last_notified_version: Some("0.3.0".into()),
            ..Default::default()
        };
        let mut incoming = AppConfig {
            device_id: Some("stale-ui-device".into()),
            last_notified_version: Some("0.1.0".into()),
            ..Default::default()
        };
        preserve_backend_managed_fields(&mut incoming, &current);
        assert_eq!(incoming.device_id.as_deref(), Some("old"));
        assert_eq!(incoming.last_notified_version.as_deref(), Some("0.3.0"));
    }

    #[test]
    fn list_accounts_returns_error_when_store_is_corrupt() {
        let _guard = crate::store::STORE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let path = temp_path("corrupt_list");
        std::fs::write(&path, "{ this is not valid json").unwrap();
        let _env = EnvVarGuard::set("AIT_ACCOUNTS_PATH", &path);

        let result = tauri::async_runtime::block_on(list_accounts());

        assert!(
            result.is_err(),
            "corrupt accounts.json must be reported, not hidden as an empty list"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn prune_removed_account_config_removes_only_the_deleted_stored_service() {
        let mut cfg = AppConfig::default();
        cfg.accounts.insert(
            "stored:victim".into(),
            crate::config::AccountConfig {
                custom_name: Some("Victim".into()),
                primary_window: Some("Weekly".into()),
            },
        );
        cfg.accounts.insert(
            "stored:kept".into(),
            crate::config::AccountConfig {
                custom_name: Some("Kept".into()),
                primary_window: None,
            },
        );
        cfg.auto_anchor.insert("stored:victim".into(), true);
        cfg.auto_anchor.insert("stored:kept".into(), true);

        prune_removed_account_config(&mut cfg, "victim");

        assert!(
            !cfg.accounts.contains_key("stored:victim"),
            "removing a stored account must prune its per-account config"
        );
        assert!(
            !cfg.auto_anchor.contains_key("stored:victim"),
            "removing a stored account must prune its auto-anchor opt-in"
        );
        assert!(
            cfg.accounts.contains_key("stored:kept"),
            "unrelated stored account config must survive"
        );
        assert_eq!(cfg.auto_anchor.get("stored:kept"), Some(&true));
    }

    #[test]
    fn remove_account_returns_success_when_only_config_cleanup_save_fails() {
        let _guard = crate::store::STORE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let accounts_path = temp_path("remove_cleanup_best_effort_accounts");
        let config_path = std::env::temp_dir().join(format!(
            "ait_commands_remove_cleanup_config_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&accounts_path);
        let _ = std::fs::remove_dir_all(&config_path);
        std::fs::create_dir_all(&config_path).unwrap();
        let _accounts_env = EnvVarGuard::set("AIT_ACCOUNTS_PATH", &accounts_path);
        let _config_env = EnvVarGuard::set("AIT_CONFIG_PATH", &config_path);

        let id = crate::store::add(crate::store::StoredCredential {
            id: "remove-me".into(),
            provider: Provider::Codex,
            label: "remove-me@example.invalid".into(),
            access_token: "ACCESS".into(),
            refresh_token: None,
            expires_at: 0,
            id_token: None,
            account_id: None,
        })
        .unwrap();
        let service_id = crate::model::stored_service_id(&id);
        let mut initial_cfg = AppConfig::default();
        initial_cfg.accounts.insert(
            service_id.clone(),
            crate::config::AccountConfig {
                custom_name: Some("Remove me".into()),
                primary_window: None,
            },
        );
        initial_cfg.auto_anchor.insert(service_id.clone(), true);
        let cfg_store: ConfigStore = std::sync::Arc::new(tokio::sync::RwLock::new(initial_cfg));

        let response = tauri::async_runtime::block_on(remove_account_inner(&cfg_store, id));

        assert_eq!(
            response,
            Ok(true),
            "store removal already succeeded, so config cleanup save failure must not become an IPC error"
        );
        assert!(
            crate::store::find("remove-me").is_none(),
            "account store deletion should remain committed"
        );
        let cfg_after = tauri::async_runtime::block_on(async { cfg_store.read().await.clone() });
        assert!(!cfg_after.accounts.contains_key(&service_id));
        assert!(!cfg_after.auto_anchor.contains_key(&service_id));

        let _ = std::fs::remove_file(&accounts_path);
        let _ = std::fs::remove_dir_all(&config_path);
    }

    #[test]
    fn add_session_key_rejects_blank_trimmed_token() {
        let _guard = crate::store::STORE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let path = temp_path("blank_token");
        let _ = std::fs::remove_file(&path);
        let _env = EnvVarGuard::set("AIT_ACCOUNTS_PATH", &path);

        let result =
            tauri::async_runtime::block_on(add_session_key(Provider::Codex, "   ".into(), None));

        assert!(
            result.is_err(),
            "blank credentials must not create a stored account"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn add_session_key_rejects_blank_claude_session_cookie() {
        let _guard = crate::store::STORE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let path = temp_path("blank_claude_cookie");
        let _ = std::fs::remove_file(&path);
        let _env = EnvVarGuard::set("AIT_ACCOUNTS_PATH", &path);

        let result = tauri::async_runtime::block_on(add_session_key(
            Provider::Claude,
            "Cookie: other=1; sessionKey=   ; x=2".into(),
            None,
        ));

        assert!(
            result.is_err(),
            "Claude credentials that normalize to blank must be rejected"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn add_claude_session_key_persists_only_session_key_cookie_value() {
        let _guard = crate::store::STORE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let path = temp_path("session_cookie");
        let _ = std::fs::remove_file(&path);
        let _env = EnvVarGuard::set("AIT_ACCOUNTS_PATH", &path);

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
        let _ = std::fs::remove_file(&path);
    }
}
