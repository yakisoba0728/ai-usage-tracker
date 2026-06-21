# Chunk 1 — Rust Bug Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the 4 confirmed Rust backend bugs (store keychain-write desync, unlocked store mutation race, scheduler restart double-fetch, OAuth cancel-flag lifecycle) without changing any other behavior.

**Architecture:** Behavior-preserving except where a bug is corrected. Each bug gets a small, focused change plus a deterministic unit test where feasible. All changes land on one branch (`fix/chunk1-rust-bugs`), one commit per bug, verified green, then fast-forward merged to `main`.

**Tech Stack:** Rust 2021, Tauri 2, tokio, `keyring` v3 (OS keychain, in-memory backend under `#[cfg(test)]`), `std::sync` primitives. Tests run via `cargo test --lib`.

## Global Constraints

- Commit messages carry **NO AI/Co-Authored-By watermark** of any kind.
- **One branch per chunk** (`fix/chunk1-rust-bugs`); push as-is; fast-forward merge to `main`; delete the branch when green.
- Verify before merge (run via `--manifest-path` so no `cd` prompt):
  - `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
  - `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
  - `cargo test --lib --manifest-path src-tauri/Cargo.toml`
- **Do NOT set `panic="abort"`** (provider isolation depends on unwind).
- Behavior-preserving everywhere except the 4 bug corrections below.
- Rust tests run in parallel; any test touching process-global state (env vars,
  the keychain in-memory map, the OAuth `ACTIVE` static, the scheduler
  `GENERATION` static) must isolate via unique ids or a dedicated `Mutex`.

---

## File structure

- `src-tauri/src/store.rs` — fix `update()` desync (Task 1); add `STORE_LOCK` + atomic `persist_records` (Task 2); new tests.
- `src-tauri/src/keychain.rs` — lift the `#[cfg(test)]` in-memory backend to a module with a per-id failure-injection hook `fail_store_for` (Task 1 support).
- `src-tauri/src/scheduler.rs` — extract `next_generation()`/`is_current()`, guard the immediate fetch, add a generation test (Task 3).
- `src-tauri/src/oauth_login.rs` — add `install_cancel_flag()` + `ActiveGuard` (clears `ACTIVE` on server exit), wire into `start`/`run_server`, add lifecycle test (Task 4).

---

## Task 1: store.rs `update()` — don't advance metadata on keychain-write failure

**Files:**
- Modify: `src-tauri/src/keychain.rs` (test backend → failure-injection hook)
- Modify: `src-tauri/src/store.rs:140-168` (`update`)
- Test: `src-tauri/src/store.rs` (new test in the existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `keychain::fail_store_for(id: &str, fail: bool)` (`#[cfg(test)]`, `pub(crate)`) — makes `store_secret(id, ..)` return `Err` while `fail==true`.
- Produces: `store::update(&StoredCredential) -> bool` now returns `false` when the id is unknown **or** the keychain write fails, and only persists metadata after the secret write succeeds.

- [ ] **Step 1: Replace the `#[cfg(test)]` backend in keychain.rs with a failure-injectable one**

In `src-tauri/src/keychain.rs`, replace the current `#[cfg(test)] fn backend()` block (lines 59-83) with a module-level test backend plus a hook. Replace:

```rust
/// In test builds the backend is a process-wide in-memory map, so unit tests
/// never touch (or prompt) the real OS keychain.
#[cfg(test)]
fn backend() -> &'static dyn Backend {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    struct Mem(Mutex<HashMap<String, String>>);
    impl Backend for Mem {
        fn store(&self, id: &str, blob: &str) -> Result<(), String> {
            self.0.lock().unwrap().insert(id.into(), blob.into());
            Ok(())
        }
        fn load(&self, id: &str) -> Result<Option<String>, String> {
            Ok(self.0.lock().unwrap().get(id).cloned())
        }
        fn delete(&self, id: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(id);
            Ok(())
        }
    }

    static MEM: OnceLock<Mem> = OnceLock::new();
    MEM.get_or_init(|| Mem(Mutex::new(HashMap::new())))
}
```

with:

```rust
/// In test builds the backend is a process-wide in-memory map, so unit tests
/// never touch (or prompt) the real OS keychain. It also supports per-id write
/// failure injection so store.rs can test the keychain-write-failure path.
#[cfg(test)]
mod test_backend {
    use super::Backend;
    use std::collections::{HashMap, HashSet};
    use std::sync::{Mutex, OnceLock};

    pub struct Mem {
        map: Mutex<HashMap<String, String>>,
        fail_ids: Mutex<HashSet<String>>,
    }

    impl Backend for Mem {
        fn store(&self, id: &str, blob: &str) -> Result<(), String> {
            if self.fail_ids.lock().unwrap().contains(id) {
                return Err("injected keychain store failure".into());
            }
            self.map.lock().unwrap().insert(id.into(), blob.into());
            Ok(())
        }
        fn load(&self, id: &str) -> Result<Option<String>, String> {
            Ok(self.map.lock().unwrap().get(id).cloned())
        }
        fn delete(&self, id: &str) -> Result<(), String> {
            self.map.lock().unwrap().remove(id);
            Ok(())
        }
    }

    static MEM: OnceLock<Mem> = OnceLock::new();

    pub fn mem() -> &'static Mem {
        MEM.get_or_init(|| Mem {
            map: Mutex::new(HashMap::new()),
            fail_ids: Mutex::new(HashSet::new()),
        })
    }

    /// Make `store(id, ..)` fail (or stop failing) for one id. Keyed by id so
    /// concurrent tests using other ids are unaffected.
    pub fn set_fail(id: &str, fail: bool) {
        let mut s = mem().fail_ids.lock().unwrap();
        if fail {
            s.insert(id.into());
        } else {
            s.remove(id);
        }
    }
}

#[cfg(test)]
fn backend() -> &'static dyn Backend {
    test_backend::mem()
}

/// Test hook: toggle keychain write failure for a single account id.
#[cfg(test)]
pub(crate) fn fail_store_for(id: &str, fail: bool) {
    test_backend::set_fail(id, fail);
}
```

- [ ] **Step 2: Write the failing test for the desync bug in store.rs**

Add to `src-tauri/src/store.rs` inside `mod tests` (after `metadata_record_serializes_without_secret_fields`):

```rust
    #[test]
    fn update_does_not_advance_metadata_when_keychain_write_fails() {
        let _g = ENV_LOCK.lock().unwrap();
        let path = temp_path("update_fail");
        let _ = std::fs::remove_file(&path);
        std::env::set_var("AIT_ACCOUNTS_PATH", &path);

        // Seed one account: token V1, expiry 100.
        let id = add(StoredCredential {
            id: "desync-test-acct".into(),
            provider: Provider::Codex,
            label: "a@b.c".into(),
            access_token: "ACCESS-V1".into(),
            refresh_token: Some("REFRESH-V1".into()),
            expires_at: 100,
            id_token: None,
            account_id: None,
        })
        .unwrap();

        // Make the keychain reject writes for this id, then attempt a rotation
        // that would advance the metadata (token V2, expiry far in the future).
        crate::keychain::fail_store_for(&id, true);
        let rotated = StoredCredential {
            id: id.clone(),
            provider: Provider::Codex,
            label: "a@b.c".into(),
            access_token: "ACCESS-V2".into(),
            refresh_token: Some("REFRESH-V2".into()),
            expires_at: 999_999,
            id_token: None,
            account_id: None,
        };
        assert!(
            !update(&rotated),
            "update must report failure when the keychain write fails"
        );

        // Re-enable writes and assert NOTHING advanced: the file keeps expiry 100
        // and the keychain keeps token V1 (no desync, no lockout).
        crate::keychain::fail_store_for(&id, false);
        let loaded = load();
        assert_eq!(loaded.len(), 1);
        assert_eq!(
            loaded[0].expires_at, 100,
            "metadata expiry must not advance when the secret write failed"
        );
        assert_eq!(
            loaded[0].access_token, "ACCESS-V1",
            "the old secret must remain in the keychain"
        );

        assert!(remove(&id));
        std::env::remove_var("AIT_ACCOUNTS_PATH");
        let _ = std::fs::remove_file(&path);
    }
```

- [ ] **Step 3: Run the test to verify it FAILS on current code**

Run: `cargo test --lib --manifest-path src-tauri/Cargo.toml update_does_not_advance_metadata_when_keychain_write_fails`
Expected: FAIL — current `update` returns `true` and persists `expires_at=999_999` (the `assert!(!update(...))` fails, and the expiry assertion would too).

- [ ] **Step 4: Implement the fix in store.rs `update()`**

Replace the body of `update` (`src-tauri/src/store.rs:140-168`):

```rust
pub fn update(cred: &StoredCredential) -> bool {
    let mut records = read_records();
    let Some(idx) = records.iter().position(|r| r.id == cred.id) else {
        return false; // unknown id; nothing to update
    };
    // Persist the secret FIRST. If the keychain write fails we must NOT advance
    // the on-disk metadata: that would leave accounts.json claiming a new
    // expiry/identity while the keychain still holds the OLD secret, desyncing
    // the two (and, for single-use rotating refresh tokens, risking lockout).
    let blob = match secret_blob_of(cred) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("keychain: serialize secret failed for {}: {e}", cred.id);
            return false;
        }
    };
    if let Err(e) = crate::keychain::store_secret(&cred.id, &blob) {
        eprintln!(
            "keychain: failed to persist updated secret for {}: {e}",
            cred.id
        );
        return false;
    }
    let r = &mut records[idx];
    r.provider = cred.provider;
    r.label = cred.label.clone();
    r.expires_at = cred.expires_at;
    r.account_id = cred.account_id.clone();
    persist_records(&records);
    true
}
```

Also update the doc comment above `update` (lines 137-139) to reflect the new contract:

```rust
/// Replace a stored credential in place (the refresh path persists rotated
/// tokens). Returns `true` only if the id was found AND the rotated secret was
/// written to the keychain; on a keychain-write failure the on-disk metadata is
/// left untouched and `false` is returned (the caller keeps using the in-memory
/// token for this cycle and the account self-heals next refresh).
```

- [ ] **Step 5: Run the test to verify it PASSES**

Run: `cargo test --lib --manifest-path src-tauri/Cargo.toml update_does_not_advance_metadata_when_keychain_write_fails`
Expected: PASS.

- [ ] **Step 6: Full verify + commit**

Run:
```
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --lib --manifest-path src-tauri/Cargo.toml
```
Expected: all green.

```bash
git add src-tauri/src/store.rs src-tauri/src/keychain.rs
git commit -m "fix(store): don't advance accounts.json metadata when keychain write fails"
```

---

## Task 2: store.rs — serialize mutations + atomic accounts.json write

**Files:**
- Modify: `src-tauri/src/store.rs` (`persist_records`, `add`, `update`, `remove`; add `STORE_LOCK`)
- Test: `src-tauri/src/store.rs` (new concurrency test)

**Interfaces:**
- Produces: account mutations (`add`/`update`/`remove`) are serialized behind a
  process-wide lock and `accounts.json` is written via temp-file + atomic rename,
  so concurrent refreshes can't lose records or read a torn file.

- [ ] **Step 1: Write a concurrency test that loses records without serialization**

Add to `src-tauri/src/store.rs` `mod tests`:

```rust
    #[test]
    fn concurrent_adds_do_not_lose_records() {
        let _g = ENV_LOCK.lock().unwrap();
        let path = temp_path("concurrent");
        let _ = std::fs::remove_file(&path);
        std::env::set_var("AIT_ACCOUNTS_PATH", &path);

        let handles: Vec<_> = (0..16)
            .map(|i| {
                std::thread::spawn(move || {
                    add(StoredCredential {
                        id: format!("conc-{i}"),
                        provider: Provider::Codex,
                        label: format!("user{i}"),
                        access_token: format!("a{i}"),
                        refresh_token: None,
                        expires_at: 0,
                        id_token: None,
                        account_id: None,
                    })
                    .unwrap();
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }

        // load() must also succeed (no torn/half-written file).
        let loaded = load();
        assert_eq!(loaded.len(), 16, "no records lost under concurrent adds");

        for i in 0..16 {
            remove(&format!("conc-{i}"));
        }
        std::env::remove_var("AIT_ACCOUNTS_PATH");
        let _ = std::fs::remove_file(&path);
    }
```

- [ ] **Step 2: Run the test to verify it FAILS on current code**

Run: `cargo test --lib --manifest-path src-tauri/Cargo.toml concurrent_adds_do_not_lose_records`
Expected: FAIL — the unsynchronized read-modify-write of `accounts.json` loses records (`loaded.len()` < 16). (Race test: if it happens to pass, re-run; it loses records on virtually every run with 16 contending threads.)

- [ ] **Step 3: Add the lock + atomic write to store.rs**

Add a module-level lock near the top of `src-tauri/src/store.rs` (after the `use crate::model::Provider;` line, line 8):

```rust
/// Serializes account mutations so an overlapping manual `refresh_now` + the
/// poll loop can't interleave a read-modify-write of accounts.json (lost update).
static STORE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
```

Replace `persist_records` (lines 76-84) with a temp-file + atomic-rename version:

```rust
fn persist_records(records: &[StoredRecord]) {
    let path = store_path();
    let file = StoreFile {
        accounts: records.to_vec(),
    };
    let Ok(json) = serde_json::to_string_pretty(&file) else {
        return;
    };
    // Write to a sibling temp file then atomically rename over the target, so a
    // concurrent reader never observes a half-written accounts.json.
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, json).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}
```

Add `let _guard = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());` as the FIRST line of `add`, `update`, and `remove`:

- `add` (currently line 107): insert right after the `pub fn add(mut cred: StoredCredential) -> Result<String, String> {` line.
- `update` (the function rewritten in Task 1): insert right after `pub fn update(cred: &StoredCredential) -> bool {`.
- `remove` (currently line 125): insert right after `pub fn remove(id: &str) -> bool {`.

(Recovering the guard from a poisoned lock via `into_inner` keeps a panicked refresh from permanently wedging all future account mutations in the long-lived menu-bar process. `load`/`list` stay lock-free — the atomic rename makes their reads safe.)

- [ ] **Step 4: Run the test to verify it PASSES**

Run: `cargo test --lib --manifest-path src-tauri/Cargo.toml concurrent_adds_do_not_lose_records`
Expected: PASS (all 16 records present).

- [ ] **Step 5: Full verify + commit**

Run the three verify commands (fmt/clippy/test) from Task 1 Step 6. Expected: all green (the Task 1 tests + all existing store tests still pass).

```bash
git add src-tauri/src/store.rs
git commit -m "fix(store): serialize account mutations and write accounts.json atomically"
```

---

## Task 3: scheduler.rs — guard the immediate fetch by generation

**Files:**
- Modify: `src-tauri/src/scheduler.rs`
- Test: `src-tauri/src/scheduler.rs` (new `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `next_generation() -> u64` and `is_current(my_gen: u64) -> bool` —
  the generation primitives the loop uses; a superseded loop performs no fetch,
  including the immediate one.

- [ ] **Step 1: Write the failing test for the generation primitives**

Append to `src-tauri/src/scheduler.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::{is_current, next_generation};

    #[test]
    fn newer_generation_supersedes_older() {
        let g1 = next_generation();
        assert!(is_current(g1), "a freshly minted generation is current");
        let g2 = next_generation();
        assert!(g2 > g1, "generation strictly increases");
        assert!(!is_current(g1), "an older generation is no longer current");
        assert!(is_current(g2), "the newest generation is current");
    }
}
```

- [ ] **Step 2: Run the test to verify it FAILS (functions don't exist yet)**

Run: `cargo test --lib --manifest-path src-tauri/Cargo.toml newer_generation_supersedes_older`
Expected: FAIL — `cannot find function next_generation`/`is_current` in this scope (compile error).

- [ ] **Step 3: Extract the generation helpers and guard the immediate fetch**

Replace the `start` function (`src-tauri/src/scheduler.rs:13-29`) and add the two helpers:

```rust
fn next_generation() -> u64 {
    GENERATION.fetch_add(1, Ordering::SeqCst) + 1
}

fn is_current(my_gen: u64) -> bool {
    my_gen == GENERATION.load(Ordering::SeqCst)
}

pub fn start(app: AppHandle, poll_seconds: u64) {
    let my_gen = next_generation();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let snap = app.state::<SnapshotStore>().inner().clone();
        let cfg = app.state::<ConfigStore>().inner().clone();
        // Skip even the immediate fetch if a newer loop already superseded us,
        // so a rapid restart can't run two refreshes concurrently.
        if is_current(my_gen) {
            refresh_once(&app, &cfg, &snap).await;
        }
        loop {
            tokio::time::sleep(Duration::from_secs(poll_seconds.max(30))).await;
            if !is_current(my_gen) {
                break; // a newer loop has taken over
            }
            refresh_once(&app, &cfg, &snap).await;
        }
    });
}
```

(`restart` at lines 31-34 is unchanged — it still calls `start`, which now mints the new generation via `next_generation()`.)

- [ ] **Step 4: Run the test to verify it PASSES**

Run: `cargo test --lib --manifest-path src-tauri/Cargo.toml newer_generation_supersedes_older`
Expected: PASS.

- [ ] **Step 5: Full verify + commit**

Run the three verify commands. Expected: all green.

```bash
git add src-tauri/src/scheduler.rs
git commit -m "fix(scheduler): guard the immediate fetch by generation so restart can't double-fetch"
```

---

## Task 4: oauth_login.rs — cancel prior login + clear ACTIVE on exit

**Files:**
- Modify: `src-tauri/src/oauth_login.rs` (`start`, `run_server`; add `install_cancel_flag` + `ActiveGuard`)
- Test: `src-tauri/src/oauth_login.rs` (new test in `mod tests`)

**Interfaces:**
- Produces: `install_cancel_flag(new: Arc<AtomicBool>)` — atomically cancels any
  previous in-flight login and installs the new flag as `ACTIVE`.
- Produces: `ActiveGuard(Arc<AtomicBool>)` — on drop, clears `ACTIVE` iff it still
  points at this guard's flag (a newer login that replaced it is left intact).

- [ ] **Step 1: Write the failing lifecycle test**

Add to `src-tauri/src/oauth_login.rs` inside `mod tests`:

```rust
    // Serializes the ACTIVE-static tests against each other (other oauth tests
    // don't touch ACTIVE). Reset ACTIVE at both ends to avoid cross-run bleed.
    static ACTIVE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn cancel_flag_lifecycle() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let _g = ACTIVE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        if let Ok(mut a) = ACTIVE.lock() {
            *a = None;
        }

        // Installing a second login cancels the first and installs itself.
        let first = Arc::new(AtomicBool::new(false));
        install_cancel_flag(first.clone());
        let second = Arc::new(AtomicBool::new(false));
        install_cancel_flag(second.clone());
        assert!(
            first.load(Ordering::SeqCst),
            "a new login must cancel the previous one"
        );
        assert!(!second.load(Ordering::SeqCst));

        // cancel() targets the current (second) flag.
        cancel();
        assert!(second.load(Ordering::SeqCst));

        // An ActiveGuard clears ACTIVE iff it owns the current flag.
        let owned = Arc::new(AtomicBool::new(false));
        install_cancel_flag(owned.clone());
        drop(ActiveGuard(owned.clone()));
        assert!(
            ACTIVE.lock().unwrap().is_none(),
            "guard owning the current flag clears ACTIVE on drop"
        );

        // A stale guard must NOT clear a newer login's flag.
        let newer = Arc::new(AtomicBool::new(false));
        install_cancel_flag(newer.clone());
        drop(ActiveGuard(Arc::new(AtomicBool::new(false))));
        assert!(
            ACTIVE.lock().unwrap().is_some(),
            "a stale guard must not clear the newer login's flag"
        );

        if let Ok(mut a) = ACTIVE.lock() {
            *a = None;
        }
    }
```

- [ ] **Step 2: Run the test to verify it FAILS (helpers don't exist yet)**

Run: `cargo test --lib --manifest-path src-tauri/Cargo.toml cancel_flag_lifecycle`
Expected: FAIL — `cannot find function install_cancel_flag` / `cannot find ... ActiveGuard` (compile error).

- [ ] **Step 3: Add `install_cancel_flag` + `ActiveGuard`**

In `src-tauri/src/oauth_login.rs`, add right after the `cancel()` function (after line 101):

```rust
/// Install `new` as the active cancel flag, cancelling any prior in-flight login
/// first so its server thread exits promptly (freeing the callback port) instead
/// of lingering until its own timeout.
fn install_cancel_flag(new: std::sync::Arc<AtomicBool>) {
    if let Ok(mut g) = ACTIVE.lock() {
        if let Some(prev) = g.take() {
            prev.store(true, Ordering::SeqCst);
        }
        *g = Some(new);
    }
}

/// Clears `ACTIVE` when a login's server thread exits, but only if `ACTIVE` still
/// points at this login's flag (a newer login may have already replaced it).
struct ActiveGuard(std::sync::Arc<AtomicBool>);

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        if let Ok(mut g) = ACTIVE.lock() {
            if g
                .as_ref()
                .is_some_and(|cur| std::sync::Arc::ptr_eq(cur, &self.0))
            {
                *g = None;
            }
        }
    }
}
```

- [ ] **Step 4: Wire the helpers into `start` and `run_server`**

In `start` (`src-tauri/src/oauth_login.rs:120-123`), replace:

```rust
            let cancelled = std::sync::Arc::new(AtomicBool::new(false));
            if let Ok(mut g) = ACTIVE.lock() {
                *g = Some(cancelled.clone());
            }
```

with:

```rust
            let cancelled = std::sync::Arc::new(AtomicBool::new(false));
            install_cancel_flag(cancelled.clone());
```

In `run_server` (after the opening brace at line 159, before `let deadline = ...`), add as the first statement so every return path clears `ACTIVE` on drop:

```rust
    // Clear ACTIVE when this server thread exits (any return path), unless a
    // newer login has already taken over.
    let _active = ActiveGuard(cancelled.clone());
```

- [ ] **Step 5: Run the test to verify it PASSES**

Run: `cargo test --lib --manifest-path src-tauri/Cargo.toml cancel_flag_lifecycle`
Expected: PASS.

- [ ] **Step 6: Full verify + commit**

Run the three verify commands. Expected: all green (existing oauth_login tests unaffected).

```bash
git add src-tauri/src/oauth_login.rs
git commit -m "fix(oauth): cancel prior login and clear the active flag on server exit"
```

---

## Finalize chunk

- [ ] **Step 1: Final full verify**

```
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --lib --manifest-path src-tauri/Cargo.toml
```
Expected: all green; test count = previous 82 + 4 new (86).

- [ ] **Step 2: Merge to main and delete the branch**

```bash
git checkout main
git merge --ff-only fix/chunk1-rust-bugs
git branch -d fix/chunk1-rust-bugs
```

---

## Self-review (run before executing)

- **Spec coverage:** Spec Chunk 1 lists 4 bugs (store desync, store race, scheduler
  restart, oauth cancel) + the keychain failing-backend test gap. Tasks 1-4 cover all
  four; Task 1 Step 1 adds the failing-backend fixture. ✓
- **Placeholder scan:** No TBD/TODO; every code step shows full code. ✓
- **Type consistency:** `fail_store_for(id,&str, bool)`, `update(&StoredCredential)->bool`,
  `next_generation()->u64`, `is_current(u64)->bool`, `install_cancel_flag(Arc<AtomicBool>)`,
  `ActiveGuard(Arc<AtomicBool>)` are used consistently across steps. ✓
- **Out of scope (deferred to Chunk 2):** the `oauth_login::run_server` 10-arg → `RunCtx`
  refactor; broader scheduler/store concurrency tests beyond the two added here.
