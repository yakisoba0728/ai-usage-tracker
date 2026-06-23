//! Pure snapshot algebra: the merge/filter/payload functions that decide what
//! the refresh pipeline keeps and what each IPC event carries. Everything here
//! is side-effect-free (no `AppHandle`, no store I/O, no async) so it stays
//! fully unit-testable — these tests are the byte-identical proof that the merge
//! logic is unchanged across the commands split.

use std::collections::HashSet;

use crate::model::{LimitWindow, Provider, ServiceSource};

const FIVE_HOUR_RESET_SECS: i64 = 5 * 60 * 60;
const FIVE_HOUR_RESET_TOLERANCE_SECS: i64 = 60;

fn five_hour_window(s: &crate::model::ServiceUsage) -> Option<&LimitWindow> {
    s.windows
        .iter()
        .chain(s.detail_windows.iter())
        .find(|w| w.label == "5-hour")
}

/// Auto-anchor is based on the 5-hour reset state, not remaining percentage:
/// send one anchor when the provider reports no reset yet, or when the reset is
/// freshly five hours away. A lower reset countdown is already anchored.
pub(crate) fn should_auto_anchor_for_reset(s: &crate::model::ServiceUsage, now_sec: i64) -> bool {
    let Some(window) = five_hour_window(s) else {
        return false;
    };
    let Some(reset_at) = window.resets_at else {
        return true;
    };
    let delta = reset_at - now_sec;
    (FIVE_HOUR_RESET_SECS - FIVE_HOUR_RESET_TOLERANCE_SECS
        ..=FIVE_HOUR_RESET_SECS + FIVE_HOUR_RESET_TOLERANCE_SECS)
        .contains(&delta)
}

/// Drop disconnected entries for any provider that also has a connected entry.
/// Keeps all connected entries (multi-account support); only suppresses the
/// redundant "auto-detect failed" error when a stored account succeeded.
pub(crate) fn dedupe_services(services: &mut Vec<crate::model::ServiceUsage>) {
    let connected: HashSet<Provider> = services
        .iter()
        .filter(|s| s.connected)
        .map(|s| s.provider)
        .collect();
    services.retain(|s| {
        s.connected || s.source != ServiceSource::Auto || !connected.contains(&s.provider)
    });
}

pub(crate) fn filter_deleted_stored_services(
    services: &mut Vec<crate::model::ServiceUsage>,
    active_stored: &HashSet<String>,
) {
    services.retain(|s| s.source != ServiceSource::Stored || active_stored.contains(&s.id));
}

pub(crate) fn loading_payload(id: &str, provider: Provider) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "provider": provider,
    })
}

/// The enriched `anchor-result` payload (spec §3 allowed delta): keeps
/// `{id, ok, detail}` and ADDS `provider` (lowercase-serialized enum, or null
/// when the id can't be resolved to a provider) + `label` (the account/email
/// display string, or null when unknown). Shared by BOTH the manual
/// `send_anchor_now` and the auto-anchor emit sites so they never drift.
pub(crate) fn anchor_result_payload(
    service_id: &str,
    ok: bool,
    detail: Option<String>,
    provider: Option<Provider>,
    label: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "id": service_id,
        "ok": ok,
        "detail": detail,
        "provider": provider,
        "label": label,
    })
}

pub(crate) fn refresh_result_payload(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LimitWindow, Provider, ServiceSource, ServiceUsage};

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
    fn anchor_result_payload_keeps_legacy_fields_and_adds_provider_and_label() {
        // Spec §3 allowed delta: the payload KEEPS {id, ok, detail} and ADDS
        // {provider, label}. provider serializes lowercase; label is the account
        // display string. A failed send carries the detail string.
        let ok = anchor_result_payload(
            "stored:claude-1",
            true,
            None,
            Some(Provider::Claude),
            Some("person@example.invalid"),
        );
        let mut keys: Vec<&str> = ok.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["detail", "id", "label", "ok", "provider"]);
        assert_eq!(ok["id"], "stored:claude-1");
        assert_eq!(ok["ok"], true);
        assert_eq!(ok["detail"], serde_json::Value::Null);
        assert_eq!(ok["provider"], "claude");
        assert_eq!(ok["label"], "person@example.invalid");

        let failed = anchor_result_payload(
            "auto:zai",
            false,
            Some("boom".into()),
            Some(Provider::Zai),
            None,
        );
        assert_eq!(failed["ok"], false);
        assert_eq!(failed["detail"], "boom");
        assert_eq!(failed["provider"], "zai");
        assert_eq!(failed["label"], serde_json::Value::Null);

        // An unresolvable id leaves provider null (never a fabricated provider).
        let unknown = anchor_result_payload("bogus", false, Some("x".into()), None, None);
        assert_eq!(unknown["provider"], serde_json::Value::Null);
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
    fn auto_anchor_uses_five_hour_reset_state_not_usage_percent() {
        let now = 1_700_000_000;
        let mk =
            |label: &str, used_percent: f32, resets_at: Option<i64>| crate::model::LimitWindow {
                label: label.into(),
                used_percent: Some(used_percent),
                resets_at,
                used: None,
                limit: None,
            };

        let mut missing_reset = svc(
            "stored:claude",
            Provider::Claude,
            ServiceSource::Stored,
            true,
            None,
        );
        missing_reset.windows = vec![mk("5-hour", 75.0, None)];
        assert!(
            should_auto_anchor_for_reset(&missing_reset, now),
            "missing 5-hour reset should request one anchor regardless of usage percent"
        );

        let mut fresh_five_hour = svc(
            "stored:codex",
            Provider::Codex,
            ServiceSource::Stored,
            true,
            None,
        );
        fresh_five_hour.windows = vec![mk("5-hour", 91.0, Some(now + 5 * 60 * 60))];
        assert!(
            should_auto_anchor_for_reset(&fresh_five_hour, now),
            "a displayed 5-hour reset should be eligible even when usage is not 0%"
        );

        let mut already_anchored = svc(
            "stored:zai",
            Provider::Zai,
            ServiceSource::Stored,
            true,
            None,
        );
        already_anchored.windows = vec![mk("5-hour", 0.0, Some(now + 3 * 60 * 60))];
        assert!(
            !should_auto_anchor_for_reset(&already_anchored, now),
            "100% remaining with a shorter reset must not trigger another auto message"
        );
    }
}
