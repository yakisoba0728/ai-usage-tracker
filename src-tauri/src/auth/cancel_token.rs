//! One cancel-flag lifecycle shared by both login flows (browser OAuth in
//! `oauth/` and device-code in `login.rs`). Each flow owns its OWN
//! `CancelToken` instance (`static`), so the two flags stay independent —
//! `cancel_login` cancels both deliberately. This module unifies the
//! IMPLEMENTATION (install / cancel / guard) that the two flows previously
//! duplicated, NOT the flags themselves.
//!
//! Semantics preserved exactly from the prior `ACTIVE`/`install_cancel_flag`/
//! `ActiveGuard` (and `DEVICE_ACTIVE`/…) pair:
//! - `install` cancels the previously-installed flag (so a stale server thread
//!   or poll exits promptly) before adopting the new one.
//! - `cancel` flips the currently-installed flag (if any).
//! - the guard clears the slot on drop only if it still owns the current flag
//!   (a newer login may have already replaced it).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// A single in-flight-login cancel slot. Construct one `static` per flow.
pub struct CancelToken {
    active: Mutex<Option<Arc<AtomicBool>>>,
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancelToken {
    pub const fn new() -> Self {
        Self {
            active: Mutex::new(None),
        }
    }

    /// Flip the currently-installed cancel flag, if any.
    pub fn cancel(&self) {
        if let Ok(guard) = self.active.lock() {
            if let Some(flag) = guard.as_ref() {
                flag.store(true, Ordering::SeqCst);
            }
        }
    }

    /// Install `new` as the active cancel flag, cancelling any prior in-flight
    /// login first so its background thread/poll exits promptly instead of
    /// lingering until its own timeout.
    pub fn install(&self, new: Arc<AtomicBool>) {
        if let Ok(mut g) = self.active.lock() {
            if let Some(prev) = g.take() {
                prev.store(true, Ordering::SeqCst);
            }
            *g = Some(new);
        }
    }

    /// A guard that clears this token's slot when the login's background work
    /// exits, but only if the slot still points at `flag` (a newer login may
    /// have already replaced it). The returned guard borrows `self`, so it must
    /// be created from a `static` token (both call sites are).
    pub fn guard(&self, flag: Arc<AtomicBool>) -> CancelGuard<'_> {
        CancelGuard { token: self, flag }
    }

    #[cfg(test)]
    fn reset(&self) {
        if let Ok(mut g) = self.active.lock() {
            *g = None;
        }
    }

    #[cfg(test)]
    fn is_clear(&self) -> bool {
        self.active.lock().map(|g| g.is_none()).unwrap_or(false)
    }
}

/// Clears the owning [`CancelToken`]'s slot on drop iff it still owns the
/// current flag.
pub struct CancelGuard<'a> {
    token: &'a CancelToken,
    flag: Arc<AtomicBool>,
}

impl Drop for CancelGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut g) = self.token.active.lock() {
            if g.as_ref().is_some_and(|cur| Arc::ptr_eq(cur, &self.flag)) {
                *g = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A fresh token for these tests; mirrors the real `static` usage. Serialized
    // so the two assertions never interleave on the shared slot.
    static TEST_TOKEN: CancelToken = CancelToken::new();
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn install_cancel_and_guard_lifecycle() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        TEST_TOKEN.reset();

        // Installing a second login cancels the first and installs itself.
        let first = Arc::new(AtomicBool::new(false));
        TEST_TOKEN.install(first.clone());
        let second = Arc::new(AtomicBool::new(false));
        TEST_TOKEN.install(second.clone());
        assert!(
            first.load(Ordering::SeqCst),
            "a new login must cancel the previous one"
        );
        assert!(!second.load(Ordering::SeqCst));

        // cancel() targets the current (second) flag.
        TEST_TOKEN.cancel();
        assert!(second.load(Ordering::SeqCst));

        // A guard clears the slot iff it owns the current flag.
        let owned = Arc::new(AtomicBool::new(false));
        TEST_TOKEN.install(owned.clone());
        drop(TEST_TOKEN.guard(owned.clone()));
        assert!(
            TEST_TOKEN.is_clear(),
            "guard owning the current flag clears the slot on drop"
        );

        // A stale guard must NOT clear a newer login's flag.
        let newer = Arc::new(AtomicBool::new(false));
        TEST_TOKEN.install(newer.clone());
        drop(TEST_TOKEN.guard(Arc::new(AtomicBool::new(false))));
        assert!(
            !TEST_TOKEN.is_clear(),
            "a stale guard must not clear the newer login's flag"
        );

        TEST_TOKEN.reset();
    }
}
