//! The refresh pipeline: building the enabled provider set, the full-snapshot
//! refresh (`refresh_once`), the single-account refresh (`refresh_account_once`),
//! the detached auto-anchor dispatch, and the per-id serialized refresh lock
//! (B-1). Everything here is impure — it reads/writes the snapshot store, emits
//! Tauri events, and fires OS notifications. The pure merge algebra it leans on
//! lives in `snapshot_merge`.

use std::collections::HashSet;

use tauri::{AppHandle, Emitter};

use crate::config::AppConfig;
use crate::model::{
    auto_service_id, stored_service_id, Provider, EVENT_ANCHOR_RESULT, EVENT_PROVIDER_LOADING,
    EVENT_USAGE_UPDATED,
};
use crate::notify::anchor_notification;
use crate::providers::{fetch_all, ProviderApi};

use super::snapshot_merge::{
    anchor_result_payload, dedupe_services, filter_deleted_stored_services, five_hour_used,
    loading_payload,
};
use super::state::{ConfigStore, SnapshotStore, PROVIDER_CTORS};

static SNAPSHOT_REFRESH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
) -> crate::model::UsageSnapshot {
    let _guard = SNAPSHOT_REFRESH_LOCK.lock().await;
    refresh_once_inner(app, cfg, snap).await
}

async fn refresh_once_inner(
    app: &AppHandle,
    cfg: &ConfigStore,
    snap: &SnapshotStore,
) -> crate::model::UsageSnapshot {
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

    let snapshot = crate::model::UsageSnapshot {
        fetched_at: chrono::Utc::now().timestamp(),
        services,
    };
    *snap.write().await = snapshot.clone();
    let _ = app.emit(EVENT_USAGE_UPDATED, &snapshot);
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
            // Source provider + account label from THIS snapshot reading (the
            // auto path has the full `ServiceUsage` in hand — no store lookup).
            let provider = s.provider;
            let label = s.account.clone();
            let app2 = app.clone();
            tauri::async_runtime::spawn(auto_anchor_task(app2, id, provider, label));
        }
    }
    // Tray shows the app icon only — no title text (per user request).
    snapshot
}

/// The detached body of one auto-anchor send. Extracted verbatim from the
/// `spawn` closure so it's a named, reachable async fn (spec §7 note) — the
/// move-captured values (`app`, `id`, `provider`, `label`) become its params,
/// behavior-identical to the inline closure.
async fn auto_anchor_task(app: AppHandle, id: String, provider: Provider, label: Option<String>) {
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
    let ok = res.is_ok();
    // Fire the OS notification (is_auto = true) — works even when the
    // menu-bar window is hidden.
    show_anchor_notification(&app, provider, label.as_deref(), ok, true);
    let _ = app.emit(
        EVENT_ANCHOR_RESULT,
        anchor_result_payload(
            &id,
            ok,
            res.as_ref().err().map(|e| e.to_string()),
            Some(provider),
            label.as_deref(),
        ),
    );
}

fn active_stored_service_ids() -> HashSet<String> {
    crate::store::list()
        .into_iter()
        .map(|c| stored_service_id(&c.id))
        .collect()
}

fn emit_loading(app: &AppHandle, id: &str, provider: Provider) {
    let _ = app.emit(EVENT_PROVIDER_LOADING, loading_payload(id, provider));
}

/// Fire the OS-native notification for an anchor outcome (works when the
/// menu-bar window is hidden). The TEXT is the pure `notify::anchor_notification`
/// builder; this wrapper is the thin, untested `.show()` shell. Errors (e.g.
/// permission denied on an unsigned bundle) are swallowed — a failed toast must
/// never panic the refresh path.
pub(crate) fn show_anchor_notification(
    app: &AppHandle,
    provider: Provider,
    label: Option<&str>,
    ok: bool,
    is_auto: bool,
) {
    use tauri_plugin_notification::NotificationExt;
    let text = anchor_notification(provider, label, ok, is_auto);
    let _ = app
        .notification()
        .builder()
        .title(text.title)
        .body(text.body)
        .show();
}

pub(crate) async fn refresh_account_once(
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
    let _ = app.emit(EVENT_USAGE_UPDATED, &snapshot);
    Ok(returned)
}

async fn remove_service_from_snapshot(app: &AppHandle, snap: &SnapshotStore, service_id: &str) {
    {
        let mut guard = snap.write().await;
        guard.services.retain(|s| s.id != service_id);
        guard.fetched_at = chrono::Utc::now().timestamp();
    }
    let snapshot = snap.read().await.clone();
    let _ = app.emit(EVENT_USAGE_UPDATED, &snapshot);
}

#[cfg(test)]
mod tests {
    use super::*;

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
