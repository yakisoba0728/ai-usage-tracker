# Chunk 0: Safety Nets — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the regression safety nets that must exist *before* any behavior-changing chunk, so the full rewrite + bug fixes stay catchable.

**Architecture:** Pure test/fixture additions against current code. No production behavior changes. Each task adds a characterization or regression test and leaves the tree GREEN. These tests become the gate for later chunks (config migration, per-account, Claude web anchor).

**Tech Stack:** Rust (`cargo test --lib`, wiremock, tokio multi-thread tests, `include_str!` fixtures), env-override hooks (`AIT_CONFIG_PATH`, `AIT_ACCOUNTS_PATH`).

## Global Constraints

- Behavior-preserving: Chunk 0 adds tests ONLY. If a "characterization" test reveals current behavior, pin that behavior as-is (the *change* happens in its owning chunk).
- Frozen invariants (do not alter): IPC JSON shapes, event/command catalog, `ProviderApi` trait + `ProviderError` taxonomy, B-1 refresh invariant, atomic owner-only writes, per-provider failure isolation.
- Verification gate per task: `pnpm verify:runtime` green (lint, `tsc --noEmit`, vitest, `cargo test --lib`); `cargo clippy --all-targets -- -D warnings` green.
- Branch: `chunk0-safety-nets`. Commits carry NO AI watermark. Merge to main when green.
- Reference spec: `docs/superpowers/specs/2026-06-22-overhaul-design.md` (§3 invariants, §8 regression-safety).

---

### Task 1: B-1 concurrent-refresh end-to-end regression test

Pins the load-bearing invariant: two concurrent `refresh_if_expired` calls on the SAME expired credential perform the underlying refresh ONCE (per-id lock + re-read-inside-lock), and all callers return the rotated token — no stale-refresh-token replay.

**Files:**
- Modify (test module only): `src-tauri/src/providers/mod.rs` (append to `#[cfg(test)] mod tests`)
- May add: a test-only seam if `refresh_if_expired` cannot inject a fake refresh (prefer a wiremock token endpoint over changing production signatures; if a seam is unavoidable, keep it `#[cfg(test)]`-gated).

**Interfaces:**
- Consumes: `refresh_if_expired(http, cred) -> Result<StoredCredential, ProviderError>`, `store::{add, find}`, the per-id `stored_refresh_lock_for`, a provider whose `refresh_stored` hits a mockable token URL (Codex/Gemini).
- Produces: test `concurrent_refresh_refreshes_once_and_all_adopt_rotation`.

- [ ] **Step 1: Read the refresh path** — `providers/mod.rs` `refresh_if_expired` (per-id mutex + `store::find` re-read), and how a provider's `refresh_stored` reaches its token endpoint, to decide injection (wiremock token URL counting hits, or a test provider).
- [ ] **Step 2: Write the failing/█characterization test** — seed an expired `StoredCredential` via `store::add` (use `AIT_ACCOUNTS_PATH` temp). Mount a wiremock token endpoint that increments a hit counter and returns a rotated token. Fire N=8 concurrent `refresh_if_expired` on the same id (tokio multi-thread). Assert: hit counter == 1, every result carries the rotated `access_token`, and the persisted record equals the rotation.
- [ ] **Step 3: Run** — `cargo test --lib --manifest-path src-tauri/Cargo.toml concurrent_refresh -- --nocapture`. Expected: PASS (invariant already holds; this pins it).
- [ ] **Step 4: Full gate** — `cargo test --lib` + `cargo clippy --all-targets -- -D warnings`. Expected: GREEN.
- [ ] **Step 5: Commit** — `git commit -m "test(refresh): pin B-1 concurrent-refresh single-rotation invariant"`.

---

### Task 2: Config old-shape fixture + round-trip characterization

Captures the CURRENT on-disk config shape (provider-level `custom_name`/`primary_window`) as a fixture, and pins that a realistic config round-trips. This fixture is what Chunk 3's migration test will load to prove old data is migrated, not reset — the top silent-data-loss guard.

**Files:**
- Create: `src-tauri/tests/config_v0_provider_level.json` (a realistic OLD-shape config: poll_seconds, providers[6] with one provider carrying `custom_name`+`primary_window`+`sort_index`, an `auto_anchor` map; NO `schema_version`/`accounts` fields).
- Modify (test module): `src-tauri/src/config.rs` (`#[cfg(test)] mod tests`).

**Interfaces:**
- Consumes: `AppConfig::load_from`/save path (via `AIT_CONFIG_PATH`), `serde_json`.
- Produces: tests `config_v0_fixture_loads_without_data_loss_today` and `appconfig_roundtrips_current_shape`. Documents (comment) which fields Chunk 3 must migrate.

- [ ] **Step 1: Read** `config.rs` load/save (`load_from`, corrupt-preservation at the parse-fail branch) to know exactly how a v0 file is parsed today.
- [ ] **Step 2: Write the fixture** `config_v0_provider_level.json` mirroring the current `AppConfig`+`ProviderConfig` shape with real values (e.g. providers[0].custom_name = "Work Claude").
- [ ] **Step 3: Write the characterization test** — load the fixture via `AIT_CONFIG_PATH`; assert it parses (no corrupt-reset) and the provider-level `custom_name` is readable at `providers[0].custom_name`. Add `appconfig_roundtrips_current_shape` (serialize→deserialize equality). Add a `// CHUNK-3 MIGRATION TARGET:` comment listing `custom_name`/`primary_window` as the fields moving per-account.
- [ ] **Step 4: Run** — `cargo test --lib config_v0 appconfig_roundtrips`. Expected: PASS (pins today's behavior).
- [ ] **Step 5: Full gate + Commit** — `git commit -m "test(config): pin v0 provider-level shape + roundtrip as migration gate"`.

---

### Task 3: Windows cfg-path resolution test

The Windows-only `#[cfg]` arms (cursor db path, keyring names) compile out on the macOS dev box and are only clippy-linted on the Windows CI leg — never asserted. Pin the Windows path resolution before the rewrite reshuffles `secrets.rs`, so a Windows regression isn't invisible to the macOS loop.

**Files:**
- Modify (test module): `src-tauri/src/secrets.rs` (`#[cfg(all(test, target_os = "windows"))]` test) — and ensure an env-override hook exists for hermetic path testing (reuse existing `AIT_*` patterns; if cursor path has no override, add a `#[cfg(test)]`-friendly seam or assert against `%LOCALAPPDATA%`/`%APPDATA%` derivation).

**Interfaces:**
- Consumes: `cursor_state_db()` and any keyring service/account constants under `#[cfg(target_os="windows")]`.
- Produces: test `cursor_state_db_resolves_windows_localappdata` (runs on the Windows CI leg only).

- [ ] **Step 1: Read** `secrets.rs` Windows arms (`cursor_state_db`, keyring names) to see what env/dirs they derive from.
- [ ] **Step 2: Write a Windows-gated test** asserting the resolved path sits under the expected `%LOCALAPPDATA%`/`%APPDATA%` base (set the env in-test for hermeticity). Keep the macOS dev loop unaffected (`cfg(target_os="windows")`).
- [ ] **Step 3: Run locally** — `cargo test --lib secrets` on macOS: the Windows test compiles-out, macOS tests still pass. (Real execution happens on the Windows CI leg.)
- [ ] **Step 4: Full gate + Commit** — `git commit -m "test(secrets): pin Windows cursor/keyring path resolution"`.

---

### Task 4: IPC contract gate — verify and strengthen

The IPC contract is the frozen invariant that makes the whole rewrite catchable. Confirm the three pins exist and are strict; add any missing assertion so a rewrite that drops a command/event/field FAILS a test.

**Files:**
- Modify (test modules): `src-tauri/src/model.rs` (shape tests), and add a catalog test listing the 12 commands + 6 events if absent (assert against `lib.rs:122-135` generate_handler set — e.g. a compile-time reference or a documented constant test).
- Cross-check: `src/lib/ipc.ts` and `src/lib/types.ts` mirror (frontend vitest already covers types; verify).

**Interfaces:**
- Consumes: `model.rs` `service_usage_json_shape_matches_ts_contract`, `service_error_json_shape_matches_ts_contract`, `provider_error_codes_are_stable`.
- Produces: test `ipc_command_and_event_catalog_is_frozen` (or confirmation the existing pins suffice).

- [ ] **Step 1: Audit existing pins** — grep for the shape/code-stability tests; confirm they assert exact field names + the stable error codes.
- [ ] **Step 2: Add the catalog freeze if missing** — a test enumerating the expected command names + event names as a sorted constant, asserting it matches the intended set (so adding/removing one is a deliberate, test-visible change). Document the two allowed deltas (anchor-result payload +provider/label; new commands rename_account/set_launch_at_login/check_update_now) as TODO-for-their-chunk comments, NOT applied here.
- [ ] **Step 3: Run** — `cargo test --lib ipc_command service_usage_json service_error_json provider_error_codes` + frontend `pnpm test`. Expected: PASS.
- [ ] **Step 4: Full gate + Commit** — `git commit -m "test(ipc): freeze command+event catalog as rewrite gate"`.

---

### Task 5: Component-test approach — decision record

Frontend has no component-render infra (no testing-library/jsdom); all 76 tests target pure `lib/` modules. Decide deliberately (per spec §8): keep "logic-in-lib" — extract each rewritten component's decision logic into a tested `lib/` module BEFORE rewriting JSX. Record it so chunks 2/3/6 follow it.

**Files:**
- Modify: `docs/superpowers/specs/2026-06-22-overhaul-design.md` (append a short "Decision record: component testing = logic-in-lib" note under §8) — OR a `docs/superpowers/plans/decisions.md`.

**Interfaces:** none (documentation).

- [ ] **Step 1: Write the decision** — for each user-visible behavior the rewrite touches (rename resolution, anchor/notification text naming provider+account, threshold-cross targeting), the decision logic moves to a `lib/` pure function with vitest coverage before the JSX is rewritten. happy-dom + testing-library added ONLY if a flow cannot be cleanly extracted.
- [ ] **Step 2: Commit** — `git commit -m "docs: record logic-in-lib component-test strategy for rewrite"`.

---

## Self-Review

- **Spec coverage (§8 Chunk 0 list):** config migration old-shape fixture → Task 2 ✓; per-account name isolation → deferred to Chunk 3 (its owning behavior change; Task 2 lays the fixture) — noted; Claude web-anchor request shape → Chunk 1 (the send fn doesn't exist yet; can't pin a non-existent shape) — noted; B-1 concurrent-refresh e2e → Task 1 ✓; Windows cfg path → Task 3 ✓; component-test approach → Task 5 ✓; IPC contract freeze → Task 4 ✓.
- **Scope note:** per-account isolation and Claude web-anchor tests are intentionally NOT in Chunk 0 — they pin behavior that does not exist yet, so they live in the chunk that creates it (D2: behavior change ships with its own failing→passing test). Chunk 0 covers everything pinnable against *current* code.
- **No production behavior changes** in any task — pure tests/fixtures/docs.
