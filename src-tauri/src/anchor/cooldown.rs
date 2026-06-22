//! Per-service anchor cooldown guard. Debounces the auto-trigger so a provider
//! the API still reports as "window empty" isn't re-anchored every poll, and
//! classifies send failures so only transient ones roll the reservation back.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::providers::ProviderError;

/// Per-service cooldown (seconds) so the auto-trigger sends at most once per this
/// interval — NOT once per usage-window (the window is hours long; this only
/// debounces repeat fires while the API still reports the window as empty).
const COOLDOWN_SECS: i64 = 600;

static LAST_ANCHOR: Mutex<Option<HashMap<String, i64>>> = Mutex::new(None);

/// Atomically check the cooldown and, if clear, record `now` and return true.
/// Returns false when within the cooldown (caller must NOT send).
pub fn try_begin(service_id: &str, now_sec: i64) -> bool {
    let mut guard = LAST_ANCHOR.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    match map.get(service_id) {
        Some(&t) if now_sec - t < COOLDOWN_SECS => false,
        _ => {
            map.insert(service_id.to_string(), now_sec);
            true
        }
    }
}

/// Roll back a `try_begin` reservation (call when the send failed).
pub fn clear(service_id: &str) {
    if let Some(map) = LAST_ANCHOR
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_mut()
    {
        map.remove(service_id);
    }
}

/// Whether an anchor-send failure is transient (worth an immediate retry) vs a
/// durable rejection (a retired model → 4xx, a missing credential, a parse
/// error). Only transient failures should roll back the cooldown; a durable one
/// must keep it so the auto-anchor doesn't retry-storm a failing provider every
/// poll (B-3 / B-13).
pub fn failure_is_transient(e: &ProviderError) -> bool {
    matches!(e, ProviderError::Network(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cooldown_allows_first_blocks_within_window_and_clears() {
        let id = "test:cooldown:unique-xyz";
        clear(id); // no prior state
        let t0 = 1_000_000i64;
        assert!(try_begin(id, t0), "first send allowed");
        assert!(!try_begin(id, t0 + 1), "within cooldown blocked");
        assert!(
            !try_begin(id, t0 + COOLDOWN_SECS - 1),
            "still within cooldown"
        );
        assert!(
            try_begin(id, t0 + COOLDOWN_SECS),
            "at/after cooldown allowed"
        );
        clear(id);
        assert!(try_begin(id, t0 + 2), "after clear, allowed again");
        clear(id); // cleanup
    }

    #[test]
    fn only_network_failures_are_transient() {
        assert!(failure_is_transient(&ProviderError::Network(
            "timeout".into()
        )));
        // Durable rejections must NOT clear the cooldown (else retry-storm).
        assert!(!failure_is_transient(&ProviderError::Status {
            status: 400,
            body: "model is not supported".into(),
        }));
        assert!(!failure_is_transient(&ProviderError::NotLoggedIn(
            "x".into()
        )));
        assert!(!failure_is_transient(&ProviderError::Parse("x".into())));
        assert!(!failure_is_transient(&ProviderError::Expired("x".into())));
    }
}
