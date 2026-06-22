# AI Usage Tracker — Deep Multi-Agent Audit

**Repository:** `/Users/yakisoba/Documents/GitHub/ai-usage-tracker`
**Date:** 2026-06-22
**Scope:** Backend (Rust/Tauri), Frontend (React/TypeScript), Cross-cutting (security, robustness, build/CI)
**Method:** Multi-agent investigation; every finding below was independently, adversarially verified as a real issue against the source tree.

---

## Executive summary

This audit consolidates **36 confirmed findings** (after deduplication of one same-location pair) spanning the Rust/Tauri backend, the React/TypeScript frontend, and cross-cutting security/CI concerns. The codebase is in good shape overall: there are **no critical findings**, a single **high**, two **medium**, and the remainder are localized **low** and **nit** issues. The bulk of risk is concentrated in a few token-lifecycle and secrets-handling paths.

The one **high**-severity issue is an auth-breaking gap: Codex accounts added via the device-code login flow store a valid refresh token but hardcode `expires_at = 0`, so proactive refresh is never triggered and the stored-fetch path has no reactive 401-refresh — the account silently dies a few hours after login and stays broken until manual re-login (`login.rs:237`). The two **medium** issues are both about credential persistence: the Codex CLI's `~/.codex/auth.json` is rewritten non-atomically so a crash/ENOSPC mid-write can corrupt it after the old refresh token is already burned (`codex.rs:133`); and the app's own plaintext token store `accounts.json` is written world-readable (mode 0644), a least-privilege defect that becomes directly exploitable on the Linux `~/.config` fallback path (`store.rs:74`).

Three further security findings are **low** but worth prompt attention: refreshed Claude tokens transit the process argv via `/usr/bin/security -w <token>` (`keychain_write.rs:20`); and the OAuth localhost callback acts on an attacker-supplied `error` param before validating `state`, allowing a cross-site abort of an in-flight login (`oauth_login.rs:220`).

A recurring backend theme is **silent persistence/robustness gaps**: several rotation/write-back failures are only `eprintln`'d (invisible in a packaged desktop app), and the auto-anchor `clear()`-on-failure logic defeats its own cooldown, producing an unbounded once-per-poll retry storm against a failing provider. On the frontend, the dominant themes are **cold-start sentinel handling** (`fetched_at: 0` renders absurd "updated 489000h ago" text and flashes the new-user empty state to configured users) and **dead code / contract drift** (unused locale keys, dead CSS, an unreachable `provider` prop, a TS type that claims `| null` but delivers `undefined`). Cross-cutting CI hygiene gaps (clippy/fmt gated to macOS leaving Windows `#[cfg]` code unlinted; no frontend ESLint at all) are the main maintainability risks. None of the findings involve token leakage across the IPC boundary, and the documented plaintext-at-rest design decision is respected throughout.

---

## Severity counts

| Severity | Count |
|----------|-------|
| Critical | 0 |
| High     | 1 |
| Medium   | 2 |
| Low      | 17 |
| Nit      | 16 |
| **Total**| **36** |

**Deduplication note:** 37 raw confirmed findings were reduced to 36 by merging the two `AddAccountDialog.tsx:93` findings (login-complete listener re-subscription churn + missing `t` dependency) — same file, same effect, same line — into a single entry (B-9) that preserves both sub-defects.

---

## Top findings (most important first)

1. **[high]** Device-code Codex accounts never refresh — `login.rs:237` (B-4)
2. **[medium]** Non-atomic write of `~/.codex/auth.json` corrupts CLI auth after RT burned — `codex.rs:133` (B-5)
3. **[medium]** `accounts.json` (plaintext tokens) written world-readable — `store.rs:74` (X-1)
4. **[low]** Refreshed Claude token passed on argv to `/usr/bin/security` — `keychain_write.rs:20` (X-2)
5. **[low]** OAuth callback acts on `error` before validating `state` — `oauth_login.rs:220` (X-3)
6. **[low]** Concurrent refreshes lose the rotated refresh token (lost update) — `mod.rs:90-112` (B-1)
7. **[low]** Failed write-back of rotated refresh token only `eprintln`'d — `mod.rs:100` (B-2)
8. **[low]** Auto-anchor `clear()`-on-failure → unbounded once-per-poll retry storm — `commands.rs:83-111` (B-3)
9. **[low]** `config::load()` silently resets ALL user config on any parse error — `config.rs:101` (B-7)
10. **[low]** Cold-start `fetched_at: 0` renders "updated 489000h ago" footer — `Dashboard.tsx:194` (F-1)
11. **[low]** clippy/fmt gated to macOS leave Windows `#[cfg]` code unlinted — `ci.yml:42` (X-4)
12. **[low]** Per-card "Refresh" button swallows backend errors silently — `AccountCard.tsx:220` (F-7)

---

# Backend

## B-4 — Device-code-logged Codex accounts never refresh (access JWT expiry breaks the account)

- **File:** `src-tauri/src/login.rs:237`
- **Severity:** high · **Category:** token-lifecycle (bug) · **Confidence:** high

**Description.** A Codex account added via the device-code flow stores a valid `refresh_token` but `expires_at = 0`, and that refresh token is then never used, so the account stops working a few hours after login (when the access JWT expires) and stays broken until manual re-login. Three facts compound:
1. `poll_codex` hardcodes `expires_at: 0` (`login.rs:237`) and its `CodexTokens` struct (`login.rs:143-151`) does not even parse `expires_in`, so expiry is never derived from the JWT either.
2. The proactive refresh in `fetch_credential` is gated on `cred.expires_at > 0 && cred.expires_at < now_ms` (`providers/mod.rs:97`), so `expires_at == 0` skips proactive refresh forever.
3. The stored-fetch path `codex::fetch_stored → fetch_with` (`codex.rs:395-406, 276`) issues the usage request directly with NO reactive 401-then-refresh retry.

The stored refresh token is therefore dead weight. This is inconsistent with the loopback-OAuth Codex path, which DOES set `expires_at` from `expires_in` (`oauth_login.rs:345-348`), so loopback-added accounts refresh correctly while device-code ones do not.

**Recommended fix.** At device-code login time, derive expiry from the access (or id) token: `expires_at = crate::jwt::jwt_exp(&access_token).map(|s| s*1000).unwrap_or(0)`, matching `codex::build_refreshed_cred` (`codex.rs:348-351`). This re-enables the proactive-refresh gate. As defense-in-depth (and to cover any already-persisted `expires_at == 0` records), give `codex::fetch_stored` a reactive 401-then-refresh-then-retry path modeled on the Codex local-CLI proactive path (`codex.rs:238-272`).

---

## B-5 — Codex `~/.codex/auth.json` write-back is non-atomic (crash/ENOSPC corrupts CLI auth after RT burned)

- **File:** `src-tauri/src/providers/codex.rs:133`
- **Severity:** medium · **Category:** robustness · **Confidence:** high

**Description.** `write_auth()` truncates and rewrites `~/.codex/auth.json` in place with a raw `std::fs::write`, unlike the app's own account store, which writes a sibling temp file then atomically renames (`store.rs:84-86`). This path runs on macOS (the primary platform) and is NOT `cfg`-gated. By the time `write_auth` is reached, the OAuth refresh has already succeeded and OpenAI has rotated the refresh token server-side. If the process crashes, the disk fills (ENOSPC truncates first, then `write_all` fails — a deterministic trigger needing no timing luck), or the app is killed mid-write, `auth.json` is left empty/partial. The Codex CLI then cannot parse it, AND the app's own next poll (`read_auth`, `codex.rs:239`) returns `ProviderError::Parse`. Recovery depends on whether the rotated-away RT is genuinely single-use.

**Recommended fix.** Mirror `store::persist_accounts`: write `path.with_extension("json.tmp")`, then `std::fs::rename(&tmp, &path)`, guarded by `is_ok()`; on Unix, `fsync` the temp file before rename. Factor a shared atomic-write helper reused by the file-based paths (this and `accounts.json`). Note: atomic rename converts "corrupt file" into "stale-but-parseable file" — it fully recovers the user only when the old RT still works, but is worth doing regardless. See related B-6 (Claude non-macOS branch).

---

## B-1 — Concurrent refreshes of the same stored credential lose the rotated refresh token

- **File:** `src-tauri/src/providers/mod.rs:90-112`
- **Severity:** low · **Category:** concurrency · **Confidence:** medium

**Description.** `fetch_credential` performs a read-modify-write across an await point: it reads a snapshot of `cred`, awaits `refresh_stored` (a network OAuth refresh that ROTATES the refresh token for Codex/Gemini), then calls `store::update`. `STORE_LOCK` (`store.rs:19`) is held only inside `store::update`, NOT across read+refresh+write. Nothing higher up serializes refreshes: `refresh_once` (scheduler poll + `refresh_now`), and `refresh_account` all reach `fetch_credential` independently. The scheduler GENERATION counter only prevents two poll *loops* from overlapping. So two concurrent refreshes of the same Codex/Gemini account both read the same old RT and both POST it; last-write-wins. Notably, `store.rs`'s own doc comment claims `STORE_LOCK` prevents exactly this lost update — an over-promised invariant. The catastrophic "account silently disconnects" outcome is conditional on server reuse-detection semantics not verifiable from this repo, hence low.

**Recommended fix.** Serialize the entire refresh-of-a-stored-credential critical section with an async-aware per-account lock (`tokio::sync::Mutex` keyed by credential id, or one global tokio Mutex around refresh+update). **Inside the lock, RE-READ the credential from the store immediately before refreshing** — the load-bearing detail, so a refresher that waited observes the rotation the winner already persisted. This also eliminates the double-POST entirely. Do not hold a `std::sync::Mutex` across the await.

---

## B-2 — Failed write-back of the rotated refresh token is only `eprintln`'d (silently stuck account)

- **File:** `src-tauri/src/providers/mod.rs:100`
- **Severity:** low · **Category:** robustness · **Confidence:** medium

**Description.** After a successful `refresh_stored`, the rotated credential is persisted with `if !crate::store::update(&updated) { eprintln!(...) }`. For rotating-RT providers (Codex), the network refresh has already consumed the old RT server-side. If persistence fails at the fs layer, the in-memory `updated` is used for the current fetch then dropped; the next poll reads the OLD credential whose RT is now invalid → refresh fails → the account is permanently stuck, with the only trace being an `eprintln` invisible in a packaged desktop app. Note the finding's deeper defect: `persist_accounts` (`store.rs:74-88`) silently swallows both serialization failure and `let _ = std::fs::rename(&tmp, &path)` (`store.rs:86`), and `store::update` returns `true` unconditionally — so the `eprintln` at `mod.rs:101` only fires on id-not-found (which can't happen here); the real silence is inside `persist_accounts`.

**Recommended fix.** First make `persist_accounts`/`update` return a `Result` that reflects the actual fs outcome (prerequisite — today `update` returns `true` even when rename failed). Then, on a persistence failure during a rotation, attach a `ServiceError` (e.g. `token_not_persisted`) to the returned `ServiceUsage` so the card warns the user, instead of log-only. A Tauri-event alternative also works.

---

## B-3 — Auto-anchor fire-and-forget tasks are unbounded; `clear()`-on-failure defeats the cooldown

- **File:** `src-tauri/src/commands.rs:83-111`
- **Severity:** low · **Category:** robustness · **Confidence:** high

**Description.** For each opted-in/connected/supported service whose 5-hour window reads 0%, `refresh_once` spawns a detached tokio task to send an anchor. On send failure the task calls `anchor::clear` (`commands.rs:98`), removing the cooldown entry, so the next poll (≥30s) re-fires immediately. A persistently failing anchor never consumes a token, so the 0% guard stays true, yielding one new detached network task every poll indefinitely — no backoff, no cap, each logging to stderr — defeating the 600s `COOLDOWN_SECS` debounce on exactly the failure path where backoff matters most. Detached tasks also outlive the spawning generation (scheduler restart does not cancel them). Opt-in and self-limiting on success, hence low.

**Recommended fix.** On failure, do NOT fully clear the cooldown — record a shorter failure-backoff timestamp (optionally exponential per service id) so repeated failures back off rather than retry every poll. The minimal correct change is removing the `clear(&id)` call so the existing 600s gate applies. Optionally track in-flight handles so a service can't have overlapping anchors and a restart can abort them. See related B-13 (hardcoded anchor models amplify this) and B-12 (SSE-200 false success).

---

## B-12 — Codex anchor treats HTTP 200 + in-band SSE error as success (suppresses re-anchor 10 min)

- **File:** `src-tauri/src/anchor.rs:111`
- **Severity:** low · **Category:** robustness · **Confidence:** medium

**Description.** `send_codex` decides success purely from the HTTP status: it drains the body with `resp.text().await.unwrap_or_default()` and returns `Err` only when `!status.is_success()`. The OpenAI Responses streaming API can return HTTP 200 and then emit an in-band failure frame (`event: response.failed` / a `data: {"error": ...}` / `response.incomplete`). The drained text is never inspected, so the function returns `Ok(())`. For auto-anchor, `Ok` keeps the `try_begin` cooldown reservation in place (clear only runs on `Err`), so the 600s window holds though nothing was anchored — a silent failure that self-heals only after the cooldown. Narrow trigger (single-char anchor turn rarely fails mid-stream), hence low.

**Recommended fix.** After confirming `status.is_success()`, scan the drained text for the PRESENCE of an explicit failure frame (`event: response.failed`, `event: error`, top-level `"error"`/`response.incomplete`) and return `ProviderError::Status` in that case. Do NOT hard-require a `response.completed` success frame (it would break the existing `send_codex` test at `anchor.rs:343` and risks false negatives on the undocumented endpoint). This preserves `clear()`-on-failure so the next poll retries promptly.

---

## B-13 — Hardcoded anchor model names will silently break the write path on provider deprecation

- **File:** `src-tauri/src/anchor.rs:18`
- **Severity:** low · **Category:** maintainability · **Confidence:** high

**Description.** Anchor bodies hardcode model ids — `ZAI_MODEL=glm-4.6`, `CLAUDE_MODEL=claude-haiku-4-5-20251001`, `CODEX_MODEL=gpt-5.5` — and the comments already note prior breakage (`claude-3-5-haiku` 404, `glm-4-flash` code 1211, `codex-mini-latest` "not supported"). When any is retired, anchor send returns 4xx; combined with `clear()`-on-failure (B-3), the auto-anchor then retries every poll forever with no backoff. This is acknowledged-staleness per the project's design notes, hence low.

**Recommended fix.** No code change strictly required. Best targeted fix dovetails with B-3: in the failure handler (`commands.rs:96-99`), only `clear()` on transient `ProviderError::Network`; on non-transient 4xx `ProviderError::Status` (model rejection) leave the cooldown in place (or escalate backoff). Optionally surface a one-time "anchor model rejected — update needed" signal to the UI rather than only `eprintln`. Keep model ids centralized (already done) and document a bump checklist.

---

## B-11 — `anchor::send` reads stored/auto tokens with no expiry check; manual "Anchor now" can fire an expired token

- **File:** `src-tauri/src/anchor.rs:173`
- **Severity:** low · **Category:** robustness · **Confidence:** high

**Description.** The anchor write path resolves credentials directly and never refreshes: `resolve_stored()` returns the raw `StoredCredential.access_token`; `resolve_claude_auto`/`resolve_codex_auto` read the token straight from the credential file. This is asymmetric with the read path (`fetch_credential`, `mod.rs:97`, refreshes when `expires_at > 0 && < now_ms`). For auto-anchor inside `refresh_once` the gap is masked (fetch runs first, gated by `s.connected`), but the manual `send_anchor_now` (`commands.rs:241`) calls `anchor::send` directly with no preceding refresh. A stored Codex account whose access token expired since the last poll posts a stale bearer → 401 instead of refreshing. Narrow window, recoverable, hence low.

**Recommended fix.** In the stored branch of `send()`, when `cred.expires_at > 0 && cred.expires_at < now_ms`, run `refresh_stored` + `store::update` before sending — ideally via a shared helper returning an "active" credential, mirroring `fetch_credential` (`mod.rs:97-112`). Route the auto-resolved arms through the provider refresh helpers too, so a manual anchor is never strictly weaker than a poll.

---

## B-2b — Concurrent `refresh_account` vs poll `refresh_once` last-writer-wins on snapshot

- **File:** `src-tauri/src/commands.rs:292-303`
- **Severity:** low · **Category:** concurrency · **Confidence:** high

**Description.** Both writers compute their data via network `.await` BEFORE taking the snapshot write lock: `refresh_once` fetches then wholesale-replaces the vector (`*snap.write().await = snapshot.clone()`, `commands.rs:77`); `refresh_account` fetches then merges under the lock (`commands.rs:292-300`). The `RwLock` serializes only the write instant, not read-compute-write, and there is no shared refresh lock, so the two can interleave with benign last-writer-wins between two near-simultaneous fresh reads of the same service. The snapshot is in-memory only and the poll re-fetches every ≥30s, so any "shows the slightly-older of two fetches" self-heals. (The originally-claimed "lost/duplicated entries" mechanism does NOT occur: the find-or-push runs entirely inside the lock, and `refresh_once` rebuilds the complete set.)

**Recommended fix.** Serialize all snapshot-producing paths behind one async refresh mutex held across fetch+write (the same mutex proposed for B-1). Note: per-entry timestamp comparison does not fit — `ServiceUsage` has no per-entry timestamp. Given the benign, self-healing impact, this is likely not worth the added global serialization cost unless addressed alongside B-1.

---

## B-7 — `config::load()` silently resets ALL user config to defaults on any parse error

- **File:** `src-tauri/src/config.rs:101`
- **Severity:** low · **Category:** robustness · **Confidence:** high

**Description.** `AppConfig::load()` does `serde_json::from_str(&text).unwrap_or_default()`. If `config.json` exists but fails to deserialize for ANY reason — broken hand-edit, wrong-typed value (`poll_seconds: "300"`), a `providers` array of length ≠ 6 (the `[ProviderConfig; 6]` fixed array makes any provider-count change a hard error), or a future incompatible schema — the entire config silently reverts to `Default`, losing per-provider enabled flags, `custom_name`, `notify_thresholds`, `primary_window`, `sort_index` order, and `auto_anchor` opt-ins, with no log/toast/IPC error. The atomic `.tmp`/rename save protects against torn writes but not against a value/schema error. Loss is reconstructable UI prefs only (no tokens — those live in a separate store) and the corrupt file is conditional, hence low.

**Recommended fix.** Distinguish "missing" from "corrupt". On parse error, log via `eprintln!` (matching `codex.rs:258`/`claude/mod.rs:432`) and rename the bad file to `config.json.corrupt-<timestamp>` (timestamped so repeated failures don't clobber the first copy) before falling back to default, so the data is recoverable and the failure visible.

---

# Secrets & credential handling

## B-6 — Claude non-macOS credential write-back (`~/.claude/.credentials.json`) is non-atomic

- **File:** `src-tauri/src/providers/claude/keychain_write.rs:44`
- **Severity:** nit · **Category:** robustness · **Confidence:** high

**Description.** On non-macOS targets, `write_creds()` rewrites `~/.claude/.credentials.json` with a raw `std::fs::write` rather than temp-file + atomic-rename. As with B-5, this happens after `refresh_oauth` rotated the single-use RT. However — because the old access token is already expired/rejected before this write runs — a crash mid-write ends in a forced re-auth whether the file is left partial (current) or stale-but-intact (post-fix): the fix changes a parse error into an auth error, the same user-visible outcome. The branch is also non-macOS only (Linux unsupported; Windows CI-compile-only). Hence nit, not the medium that the prose mentions.

**Recommended fix.** For consistency and to spare a concurrent CLI reader a syntactically half-written file, serialize to a sibling temp path then `std::fs::rename` over `.credentials.json` (atomic on Windows and Unix); share one atomic-write helper with B-5. Treat as a low/nit consistency cleanup — it does not prevent the credential loss inherent to single-use RTs.

---

## B-8 — `existing_keychain_account` only parses the quoted `acct` form; hex/missing forms fall back to `$USER`

- **File:** `src-tauri/src/providers/claude/keychain_write.rs:65`
- **Severity:** low · **Category:** robustness · **Confidence:** high

**Description.** The parser matches `"acct"<blob>="` + trailing `"`. `security find-generic-password` prints the account in hex (`"acct"<blob>=0x6861...`) when it contains non-printable/non-ASCII bytes (e.g. a username with non-ASCII characters; empirically `José` → `0x4A6F73C3A9`, Korean `유저` → `0xEC9CA0ECA080`), and `<NULL>` when absent. In those cases the parser returns `None` and `write_creds` falls back to `$USER`. If the pre-existing entry's account differs from `$USER` and renders as hex, `add-generic-password -U` forks a duplicate item — the exact "shared machine / renamed user" scenario the comment at `keychain_write.rs:13-17` tries to avoid. (Correction to the original finding: a space does NOT trigger hex — it stays quoted; only genuinely non-ASCII bytes do.) Reads still find one of the entries, so it's "safe-ish", hence low.

**Recommended fix.** Also parse the hex form: when the line is `"acct"<blob>=0x<hex>`, strip `0x`, decode the hex (up to the trailing double-space) to bytes, `String::from_utf8`. Keep the `$USER` fallback for the genuine `<NULL>`/no-entry case. Alternatively probe via `-g`/exit-code before writing. At minimum document the non-ASCII fallback.

---

## B-10 — `read_macos_keychain` returns the raw `security -w` output including its trailing newline (latent footgun)

- **File:** `src-tauri/src/secrets.rs:120`
- **Severity:** nit · **Category:** robustness · **Confidence:** high

**Description.** `security find-generic-password -w` prints the secret followed by a trailing newline; `read_macos_keychain` returns `String::from_utf8(out.stdout)` verbatim. Both current callers tolerate it (Claude path trims in `parse_claude_creds_raw`; Copilot path trims in `parse_copilot_keystore_value`), so there is NO live bug. But the function's contract implies a clean value, and any future caller using the return as a bearer token verbatim would send a newline-corrupted credential and get a confusing 401. Latent only, hence nit.

**Recommended fix.** Trim at the source: `String::from_utf8(out.stdout).map(|s| s.trim_end_matches('\n').to_string())` (or `.trim().to_string()`), so the return value is a clean credential regardless of caller. Both existing callers already trim, so this cannot regress them.

---

## B-14 — Cursor `SQLITE_BUSY`/locked-DB and corrupt-DB errors are mislabeled as NotLoggedIn/Parse

- **File:** `src-tauri/src/providers/cursor.rs:69`
- **Severity:** nit · **Category:** robustness · **Confidence:** medium

**Description.** `read_cursor_token` opens `state.vscdb` READ_ONLY. Because `cursor_state_db()` already guarantees the file exists, any open failure here is an IO/lock/corruption condition — never "absent" — yet it maps to `ProviderError::NotLoggedIn` ("open state.vscdb: ..."), which the frontend localizes to "Not signed in.", and a prepare/query failure maps to `Parse`. So a transient read failure while the user IS signed in surfaces as a misleading sign-in prompt. The more likely BUSY path is actually line 73's `query_row(...).ok()`, which swallows any step error into `None`. Experimental, read-only, self-healing on the next poll, hence nit.

**Recommended fix.** Since the file is known to exist, map open/prepare failures to a retryable `ProviderError::Network` ("Cursor DB temporarily unavailable, retry") rather than `NotLoggedIn`/`Parse`. Critically, change the line-73 `query_row(...).ok()` to propagate its error first, otherwise the most likely BUSY path stays mislabeled. Low value; do opportunistically if touching this code.

---

# Provider normalization & data model

## B-15 — Codex "Credits balance" maps a *remaining* balance into the `LimitWindow.used` field (semantic inversion)

- **File:** `src-tauri/src/providers/codex.rs:206`
- **Severity:** nit · **Category:** correctness · **Confidence:** high

**Description.** `normalize()` stores the credits balance (a *remaining* dollar amount, e.g. fixture `"9.99"`) as `LimitWindow{ label:"Credits balance", used: Some(v), limit: None }`. Elsewhere `used` means "amount consumed" (cursor, zai, `push_window`); here it holds the amount *remaining*. Because `limit`/`used_percent` are `None`, the modal renders it as a bare number, so it is not arithmetically wrong today — but the field semantics are inverted relative to every other window, fragile if the UI ever derives a percent or `remaining = limit - used`. Hence nit.

**Recommended fix.** Store the balance informationally: keep `used = None` and embed the value in the label (e.g. `"Credits balance: $9.99"`), or add a dedicated note field. At minimum, comment that `used` here is remaining, not consumption.

---

## B-16 — Copilot metered category with absent `unlimited` and all-None quota fields produces a blank headline window

- **File:** `src-tauri/src/providers/copilot.rs:218`
- **Severity:** nit · **Category:** robustness · **Confidence:** high

**Description.** `normalize()` treats `unlimited` via `unwrap_or(false)`, so a category where `unlimited` is absent is metered. If it also lacks `entitlement`/`remaining`/`percent_remaining` (all `None`), the code still pushes a `LimitWindow` with `used_percent`/`used`/`limit` all `None`; the sort treats `None` as `0.0`, so this empty window can become `windows[0]` — the card headline — rendering as a blank/zero usage bar. The live fixture never hits this, but a partial future payload could. There is no guard requiring at least one displayable field. Hence nit.

**Recommended fix.** Skip emitting a metered window when nothing is displayable — e.g. `if pct.is_none() && q.entitlement.is_none() && remaining.is_none() { /* skip */ }` — mirroring the guard `cursor::normalize` already uses (`cursor.rs:108-110`).

---

## F-3 — TS `ProviderConfig` declares `custom_name`/`primary_window` as required, but Rust omits them at runtime

- **File:** `src/lib/types.ts:79`
- **Severity:** nit · **Category:** maintainability (contract) · **Confidence:** high

**Description.** In `config.rs` these fields are `#[serde(default, skip_serializing_if = "Option::is_none")]`, so when `None` (the common case) the keys are OMITTED from `get_config`'s JSON (not `null`). The mirrored TS interface declares them required `string | null`, so at runtime they are `undefined`, not `null` — an unsound type. This works today only by luck of `?.`/`??` guards at every read site. There is also no Rust-side shape-guard test for `ProviderConfig`/`AppConfig` (unlike `ServiceUsage`), and `types.test.ts` deliberately opts out of pinning the inner key set. Hence nit.

**Recommended fix.** Declare `custom_name?: string | null` and `primary_window?: string | null` (optional AND nullable) — matching the established `ServiceError.detail` pattern and the slim wire shape. Note: dropping `| null` entirely would be wrong, because `null` IS genuinely written on the TS write side (`InspectorSettings.tsx:61,107`). Consider adding a `ProviderConfig` shape-guard test mirroring the `ServiceUsage` one in `model.rs`.

---

# Frontend

## F-1 — Cold-start `fetched_at: 0` snapshot renders an absurd "updated 489000h ago" footer

- **File:** `src/components/Dashboard.tsx:194`
- **Severity:** low · **Category:** bug · **Confidence:** high

**Description.** On a fresh launch the store is `UsageSnapshot { fetched_at: 0, services: [] }` (`commands.rs:15-20`). The first `getUsage()` resolves immediately from this store with `fetched_at: 0` (the async startup `refresh_once` emits `usage-updated` seconds later). In Dashboard, `fetchedAt = snapshot?.fetched_at ?? null` (`Dashboard.tsx:194`) lets the literal `0` through (0 is not nullish), and `formatUpdatedAgo` only treats `null` as "awaiting" (`format.ts:91`); with `fetchedAtSec = 0` it computes `elapsed ≈ 1.78e9 s` and renders e.g. "updated 489000h ago" for the entire multi-second cold-start window on every launch.

**Recommended fix.** Guard inside `formatUpdatedAgo`: `if (fetchedAtSec == null || fetchedAtSec <= 0) return t("time.awaiting");` — this is strictly better than the `Dashboard.tsx:194`-only fix because it also protects the other call sites (`AccountDetailDialog.tsx:223`, `SessionsTab.tsx:46`). Add a regression test asserting `formatUpdatedAgo(0, now, t) === "time.awaiting"`.

---

## F-7 — Per-card "Refresh" button swallows backend errors with no user feedback

- **File:** `src/components/dashboard/AccountCard.tsx:220`
- **Severity:** low · **Category:** ux · **Confidence:** high

**Description.** The Refresh button calls `void refreshAccount(row.id)` with no `.catch`. The backend `refresh_account` early-returns `Err("unknown or disabled account: ...")` and emits NOTHING on that path (only `provider-loading`/`usage-updated` on success). So on a stale/disabled/unknown id the promise rejects, the rejection is `void`-ignored, and the user sees nothing. Contrast the sibling `void sendAnchorNow(row.id)`, whose failures ARE surfaced via the always-emitted `anchor-result` event + global toast. Reachability is narrow (ordinary fetch errors take the success path because `fetch_credential` returns a `ServiceUsage` with an `error` field, not a `Result`); only a genuine stale-id race hits the silent path. Hence low.

**Recommended fix.** Preferred: make `refresh_account` emit a result event on every path (mirroring `send_anchor_now`), so one global Dashboard listener can toast it — avoids prop-drilling `pushToast`. Add a `toast.refreshFailed` i18n key. Alternatively attach a `.catch` that toasts.

---

## F-4 — `formatResetShort` shows "0m" instead of "soon" for a reset under 30 seconds away

- **File:** `src/lib/format.ts:62`
- **Severity:** low · **Category:** correctness · **Confidence:** high

**Description.** The "soon" copy is only used when `diff <= 0`. For a reset 1–29s in the future, `diff` is positive (skips "soon"), then `mins = Math.round(diff/60000)` rounds to 0, and because `mins < 60` it emits `t("time.reset.minutes", { m: 0 })` → the literal "0m". Reachable on real renders (the `useNow` clock ticks any approaching reset through the sub-30s range). The existing `diff <= 0 → "soon"` branch shows the author's clear intent. Self-heals within ~30s, hence low.

**Recommended fix.** After computing `mins`, add `if (mins < 1) return t("time.reset.soon");` (equivalent to `mins === 0` here since `diff > 0`). Do NOT use `Math.ceil` (would render "1m" for a 5s window). Add a regression test for a sub-30s diff.

---

## F-9 — `AddAccountDialog` login-complete listener re-subscribes every render AND omits `t` from deps

- **File:** `src/components/AddAccountDialog.tsx:93`
- **Severity:** low · **Category:** robustness + correctness · **Confidence:** high
- *(Merged: re-subscription churn [low] + missing-`t`-dep stale-i18n [nit], same file/line.)*

**Description (sub-defect 1 — re-subscription churn, low).** The `onLoginComplete` effect lists `[onChanged, onOpenChange]` as deps. From Dashboard, `onChanged={() => void refresh()}` is a fresh closure on every render, and Dashboard re-renders frequently (10s `useNow` tick, toast push/dismiss, snapshot updates), so the effect tears down and re-creates the async Tauri `listen('login-complete')` subscription on essentially every render. Because `listen()` is async, each cycle has a brief window where the old unlisten has run but the new listen hasn't resolved, during which a `login-complete` event could be dropped — most likely exactly while a login is in flight. `refresh` itself is stable (`useCallback([])`); the wrapper is the sole source of instability.

**Description (sub-defect 2 — stale-language error text, nit).** The same effect uses `t('addAccount.loginFailed')` (`AddAccountDialog.tsx:87`) but omits `t` from its deps. react-i18next returns a new `t` identity on language change; with `t` excluded, a login that fails right after a language toggle could surface the failure message in the previous language. Currently masked because the unstable `onChanged` already forces re-subscription, but it becomes load-bearing once `onChanged` is memoized.

**Recommended fix.** Stabilize the callback at the Dashboard call site: `onChanged={refresh}` (a `Promise`-returning fn is assignable to the `() => void` prop slot), or wrap in `useCallback`; alternatively store `onChanged`/`onOpenChange` in refs and depend on `[]`. AND add `t` to the dependency array (defensive — becomes necessary once `onChanged` is stabilized).

---

## F-5 — `SettingsDialog` left-nav items are non-interactive dead UI

- **File:** `src/components/SettingsDialog.tsx:93`
- **Severity:** low · **Category:** a11y / dead-UI · **Confidence:** high

**Description.** The settings left-nav renders each section ("General", "Providers") as a plain `<div>` with no `onClick`, `href`, `role`, `tabIndex`, or keyboard handler. The comment at lines 70-72 says "the nav scrolls to them", but there is no `scrollIntoView`, no anchor, no ref wiring, and the `Section` components have no `id` — clicking is inert. (Severity is low/nit, not a true a11y trap: nothing is activatable, so no keyboard/SR user is *blocked*; the only genuine thread is a `<nav>` landmark containing zero navigable elements. The items render as labels, not buttons — `text-left` is alignment, not a button affordance.)

**Recommended fix.** Proportionate fix (both sections always render on this small dialog): drop the misleading `<nav>` semantics / button-like affordances, render the items as plain labels (or remove the left rail), and delete/correct the stale comment at lines 70-72. If interactivity is genuinely wanted, give each `Section` an id and render the items as `<button>`/`<a href>` calling `scrollIntoView` with `aria-current`.

---

## F-2 — Cold-start empty snapshot flashes EmptyState/NoResults to already-configured users

- **File:** `src/hooks/useSnapshot.ts:44`
- **Severity:** nit · **Category:** ux · **Confidence:** high

**Description.** Because the first `getUsage()` resolves with the non-null empty sentinel `{fetched_at:0, services:[]}`, `useSnapshot` sets `snapshot` to that object and `loading` to false. In Dashboard, the `loading && snapshot == null ? <LoadingState/>` branch is bypassed (snapshot non-null), and since `hasConfigured = allServices.length > 0` is false, the UI shows `<EmptyState>` ("add your first account") until the first real `usage-updated` arrives. A configured user briefly sees the new-user empty state. The window launches hidden and the webview mounts once, so the flash is usually completed before the user opens the dashboard — observable only if they open it during first-refresh latency. Hence nit.

**Recommended fix.** Treat the `fetched_at === 0` sentinel as "not yet loaded" by extending the LoadingState condition to catch it, e.g. `(loading || snapshot == null || snapshot.fetched_at === 0) && error == null ? <LoadingState/> : snapshot == null ? <ErrorState/> : ...`. (Naïve variants that only null-out the snapshot or only gate EmptyState fall through to ErrorState/NoResults respectively — verify the full branch.)

---

## F-6 — `serviceStatus` re-implements the severity→status mapping instead of reusing `severityToStatus`

- **File:** `src/lib/status.ts:14`
- **Severity:** nit · **Category:** maintainability · **Confidence:** high

**Description.** `serviceStatus` inlines the exact crit/warn/ok/unknown chain that the exported `severityToStatus` helper already provides in the same file (and which `AccountCard.tsx:253`/`LimitsTab.tsx:50` already compose as canonical). The two must stay in lockstep; duplication invites drift. Hence nit.

**Recommended fix.** Replace the inline chain with `return severityToStatus(percentSeverity(pct));` — type-compatible (`percentSeverity` returns `Severity | null`, which `severityToStatus` accepts) and behaviorally identical. Covered by existing `status.test.ts`.

---

## F-8 — `formatUpdatedAgo` has no day tier — long-stale timestamps render as unbounded hours

- **File:** `src/lib/format.ts:107`
- **Severity:** nit · **Category:** ux · **Confidence:** high

**Description.** Unlike `formatResetShort` (which has a day tier), `formatUpdatedAgo` tops out at hours+minutes with no upper bound, so a long-stale `fetched_at` (app suspended, or a provider failing for a long time) renders "Updated 200h 3m ago". Truthful and never crashes; only reachable in degraded states. The two formatters have legitimately different typical ranges, so the missing day tier is a defensible simplicity tradeoff. Hence nit.

**Recommended fix.** Optional. If addressed, add a day tier mirroring `formatResetShort` (`if (hours >= 24) …`), AND add the new `time.updatedDays`/`time.agoDays` keys to BOTH `en.json` and `ko.json`, else i18next falls back to the raw key.

---

## F-10 — Manual "Send now" anchor shows duplicate feedback (inline status + global toast)

- **File:** `src/components/dashboard/detail/InspectorSettings.tsx:184`
- **Severity:** nit · **Category:** ux · **Confidence:** high

**Description.** The ConfirmDialog `onConfirm` calls `sendAnchorNow(service.id)` and sets a local `anchorStatus` line, but `send_anchor_now` ALWAYS emits `anchor-result` (`commands.rs:243-250`), which Dashboard subscribes to and toasts. So a manual inspector send produces BOTH the inline "sent/failed" text AND a top-right toast for the same action — and the wording disagrees ("Sent." vs "Anchor message sent.", "Failed: {{error}}" vs "Anchor failed: {{error}}"). The card-level Send button intentionally relies on the toast only. Hence nit.

**Recommended fix.** Pick one channel for the inspector: simplest is to remove the local `anchorStatus` state and the `.then/.catch` in `InspectorSettings` and rely on the global toast (matching the card button). Suppressing the toast for inspector sends would require threading an origin flag through IPC — not justified.

---

## F-11 — Dead CSS: `.provider-card-fetching` and `.threshold-flash` are never applied

- **File:** `src/index.css:210`
- **Severity:** nit · **Category:** dead-code · **Confidence:** high

**Description.** `index.css` defines `.provider-card-fetching::after` (lines 210-222) and `.threshold-flash` (lines 224-226) plus the `@keyframes provider-fetch-sweep` and `@keyframes threshold-card-flash` that only those rules use. A repo-wide grep finds zero applications: the live fetch indicator is the pulsing dot (`.provider-fetch-dot`), and the threshold crossing only triggers a toast (`Dashboard.tsx:182`), never a card flash. Both rule blocks and their keyframes are dead.

**Recommended fix.** Remove the two rules and the two now-orphaned keyframes (safe — referenced by nothing else). Alternatively wire `threshold-flash` onto the crossed card if the flash UX was intended.

---

## F-12 — `ErrorState` `provider` prop is never passed — provider-hint branch is unreachable

- **File:** `src/components/ErrorState.tsx:17`
- **Severity:** nit · **Category:** dead-code · **Confidence:** high

**Description.** `ErrorState` accepts an optional `provider` prop and branches on it to show `error.hint.<provider>`, but its only call site (`Dashboard.tsx:242`) passes no `provider`, so the branch and all `error.hint.*` lookups never run (only `error.checkConnection` does). Git history confirms this is refactor residue from a former per-card error component; the snapshot-failure view is provider-agnostic, so no provider is ever available to pass. The `error.hint.*` subtree is orphaned by the same dead branch.

**Recommended fix.** Drop the `provider` prop and the conditional, keep only `t('error.checkConnection')`, drop the now-unused `Provider` import, and also remove the orphaned `error.hint.*` keys in both `en.json` (lines 52-60) and `ko.json`. (The "wire a provider" alternative is wrong — this view has no single provider to blame.)

---

## F-13 — Five (actually six) locale keys are dead — never referenced by any `t()` call (en+ko)

- **File:** `src/locales/en.json:28`
- **Severity:** nit · **Category:** dead-code (i18n) · **Confidence:** high

**Description.** Five keys exist in both `en.json` and `ko.json` but are never read: `error.offline` (`formatServiceError` only builds `error.<code>` over the 5 `SERVICE_ERROR_CODES`, and "offline" is only a Rust *test* fixture), and `card.online`, `card.disconnected`, `card.noReset`, `card.limitNotReported` (AccountCard uses a different set of `card.*` keys). The locale parity test only checks en/ko symmetry + the 5 error codes — it never checks that keys are consumed, so dead keys accumulate silently. (Correction: `card.offline` is ALSO dead — six, not five.)

**Recommended fix.** Delete the named keys (plus `card.offline`) from both `en.json` and `ko.json`. Optionally extend `locales.test.ts` with a static-analysis guard that scans `src` for literal `t("…")` keys + the known dynamic prefixes (`section.`, `error.`, `error.hint.`, `detail.tab.`, `addAccount.*`) and asserts no leaf is unreferenced, so future dead keys fail CI.

---

# Cross-cutting

## X-1 — `accounts.json` (plaintext tokens) is written world-readable

- **File:** `src-tauri/src/store.rs:74`
- **Severity:** medium · **Category:** security · **Confidence:** high

**Description.** `persist_accounts()` writes via `std::fs::write` to a new temp file then renames. `std::fs::write` on a freshly created file uses the umask default (typically mode 0644 — verified `-rw-r--r--` on the live file), so `accounts.json` ends up world-readable. It contains `access_token`/`refresh_token`/`id_token` in cleartext. The documented plaintext-at-rest decision only justifies the OWNER reading without a keychain prompt — NOT exposing tokens to every other local UNIX account. On default macOS, `~/Library/Application Support` is `0700`, so the directory ACL is the effective barrier and this is least-privilege/defense-in-depth — but it becomes directly exploitable on the `~/.config/ai-usage-tracker` fallback path (`store.rs:54-61`), which `create_dir_all`s without `0700` and whose parent is frequently not `0700`. No `set_permissions`/`mode()` call exists anywhere. Hence medium (not the originally-claimed high).

**Recommended fix.** Restrict the temp file (and thus the renamed target) to owner-only at write time. On `cfg(unix)`: `OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)` then write+rename — `rename` preserves the inode mode, and this leaves no world-readable window (unlike a post-write `set_permissions`). Also `create_dir_all` the config dir then set it `0700`. Apply the same hardening to `config.rs:111` (config.json is also 0644, less sensitive). Guard with `cfg(unix)`.

---

## X-2 — Refreshed Claude credentials are passed to `/usr/bin/security` on the command line (`-w <token>`), exposing the token in the process table

- **File:** `src-tauri/src/providers/claude/keychain_write.rs:20`
- **Severity:** low · **Category:** security · **Confidence:** high

**Description.** `write_creds()` shells out to `security add-generic-password ... -w <s> -U`, where `s` is the full serialized credentials blob (refreshed access + rotating refresh token). Passing the secret as an argv element makes it visible in the global process table for the subprocess's lifetime: any other local user/unprivileged process running `ps -ww` can capture the live Claude OAuth tokens (empirically confirmed cross-user argv visibility on macOS). This is the live production refresh path (not test-gated). Apple's own man page warns "Use of the -p or -w options is insecure. Specify -w as the last option to be prompted." — and the code places `-w s` mid-args, not last. Local-only, transient, hence low. (The read path uses `-w` only to print, exposing no secret.)

**Recommended fix.** Avoid putting the secret on argv. Best: use the Security-framework API (`SecItemAdd`/`SecItemUpdate` via the `keyring`/`security-framework` crate), keeping the secret in-process. Minimum: use the stdin/prompted `-w`-last form — note `add-generic-password` prompts for the password TWICE, so pipe the value twice (e.g. `printf 'BLOB\nBLOB\n' | security ... -U -w`). Do NOT use `-X` (still argv).

---

## X-3 — OAuth localhost callback acts on attacker-supplied `error` before validating `state` (cross-site login abort)

- **File:** `src-tauri/src/oauth_login.rs:220`
- **Severity:** low · **Category:** security · **Confidence:** high

**Description.** In `run_server()`, the callback handler checks `params.get("error")` (lines 220-225) and emits a failure BEFORE the `state` check (line 226). The server listens on the fixed, guessable `127.0.0.1:1455` (random fallback only if taken). While a login is in flight (300s window), any web page the user visits can issue a no-CORS navigation/`fetch`/`<img>` to `http://localhost:1455/auth/callback?error=foo` and, with no state validation on the error branch, force the login to terminate with "provider error: foo". Impact is a nuisance/self-DoS — the `code` success path IS state-checked, so no code injection or token theft. (The reflected-XSS speculation is a FALSE premise: tiny_http 0.12.0 `from_string` hardcodes `Content-Type: text/plain; charset=UTF-8`, so there is no XSS sink.) Hence low.

**Recommended fix.** Validate `state` FIRST for every callback branch (including the error branch); reject/ignore any request whose `state != expected_state`. The genuine provider-error redirect carries the matching `state`, so this does not break legitimate provider errors. The Content-Type/HTML-escape recommendation is non-load-bearing (already text/plain).

---

## X-4 — clippy/fmt gated to macOS leave Windows-only `#[cfg]` code completely unlinted

- **File:** `.github/workflows/ci.yml:42`
- **Severity:** low · **Category:** maintainability (build/CI) · **Confidence:** high

**Description.** The `rust` matrix runs macOS + Windows, but `cargo fmt --check` and `cargo clippy -D warnings` are guarded by `if: matrix.os == 'macos-latest'`. On macOS, every `#[cfg(target_os = "windows")]` / `#[cfg(not(target_os = "macos"))]` block is compiled out, so clippy never sees it — including `secrets.rs:53-57` (Windows Cursor DB path with `dirs::data_dir()?` Option propagation), the Windows Copilot keyring code (`copilot.rs:110-180`), `keychain_write.rs:41-47`, and `lib.rs:37`. The Windows leg runs only `cargo test` (no clippy/fmt). Git history shows this is a regression: the guard was flipped from `ubuntu-latest` to `macos-latest` when the Linux runner was dropped, losing the lint coverage Linux's leg used to provide. Since Windows is CI-compile-verified but not hardware-tested, clippy is its main safety net. Compile-checking is intact (test still compiles those arms), so only warning-level issues go uncaught. Hence low.

**Recommended fix.** Run clippy on BOTH matrix legs — remove the `if: matrix.os == 'macos-latest'` guard from the clippy step (keep `--all-targets -- -D warnings`). This is better than the pre-regression Linux setup because the Windows runner lints the true `target_os = "windows"` arm. `fmt` is platform-agnostic, so it may stay gated to one OS. Keep `fail-fast: false`.

---

## X-5 — No frontend lint (ESLint absent entirely); CI only type-checks

- **File:** `.github/workflows/ci.yml:23`
- **Severity:** nit · **Category:** maintainability (build/CI) · **Confidence:** high

**Description.** The `frontend` job runs only `tsc --noEmit` + `vitest`. There is no ESLint config and `eslint` is not a dependency. `tsc` does not catch React-specific defects: `react-hooks/exhaustive-deps`, `rules-of-hooks`, a11y, floating promises. Hooks are used heavily (Dashboard has 17 hook calls with hand-maintained deps arrays). Telling asymmetry: the Rust job enforces `clippy -D warnings` while the frontend half has none. No live defect was found (a scan of Dashboard deps arrays came back clean), and the floating-promises argument is weak (`void openUrl(...)` is the correct discard), so this is a preventive-tooling gap — hence nit. (Related to F-9, which a deps lint would have caught.)

**Recommended fix.** Add `eslint`, `@eslint/js`, `eslint-plugin-react-hooks`, `eslint-plugin-react-refresh` (optionally `eslint-plugin-jsx-a11y`) as devDependencies with a flat `eslint.config.js`, a `"lint": "eslint ."` script, and a `pnpm lint` CI step. The load-bearing value is `react-hooks` (exhaustive-deps + rules-of-hooks); consider starting as warnings to avoid blocking CI on pre-existing noise.

---

## X-6 — CSP whitelists `asset:` in `img-src` but the asset protocol is never enabled or used

- **File:** `src-tauri/tauri.conf.json:24`
- **Severity:** nit · **Category:** maintainability (build/CI) · **Confidence:** high

**Description.** The CSP `img-src 'self' asset: data:` permits the Tauri `asset:` custom protocol, but `app.security.assetProtocol` is not configured (disabled by default in Tauri 2), no capability grants it, and no frontend code calls `convertFileSrc` or references `asset:`. The token is dead config — it widens the declared CSP surface for a capability the app does not have. Not exploitable (the protocol isn't served), but misleading config / stale-hardening smell. Hence nit.

**Recommended fix.** Drop `asset:` from `img-src` (leaving `'self' data:`). If asset-protocol access is wanted later, add `app.security.assetProtocol.enable: true` with a scoped `scope` and the matching capability.

---

## X-7 — CI Node/pnpm versions pinned only in workflows; no `engines`/`.nvmrc` for local dev parity

- **File:** `package.json:6`
- **Severity:** nit · **Category:** maintainability (build/CI) · **Confidence:** high

**Description.** CI pins Node 24 and pnpm 9 (`ci.yml:18-21`, `build-smoke.yml:23-26`), but `package.json` has no `engines` or `packageManager` field, and there is no `.nvmrc`/`.node-version`. Local developers can run an arbitrary Node/pnpm that diverges from CI. Not a correctness bug — `pnpm install --frozen-lockfile` pins dependency resolution regardless — purely a local-dev parity nicety. Hence nit.

**Recommended fix.** Add `"packageManager": "pnpm@9.15.4"` (Corepack pin) and `"engines": { "node": ">=24" }` to `package.json` (and optionally `.nvmrc` with `24`). Note `engines` is advisory unless `engine-strict=true`, and `packageManager` auto-activates only with Corepack enabled — so this improves parity without enforcing it.

---

## Appendix — finding index by area

| ID | Severity | File | Area |
|----|----------|------|------|
| B-4 | high | login.rs:237 | Backend / token-lifecycle |
| B-5 | medium | codex.rs:133 | Backend / token-lifecycle (atomicity) |
| B-1 | low | mod.rs:90-112 | Backend / concurrency |
| B-2 | low | mod.rs:100 | Backend / robustness |
| B-2b | low | commands.rs:292-303 | Backend / concurrency |
| B-3 | low | commands.rs:83-111 | Backend / anchor robustness |
| B-12 | low | anchor.rs:111 | Backend / anchor robustness |
| B-13 | low | anchor.rs:18 | Backend / anchor maintainability |
| B-11 | low | anchor.rs:173 | Backend / anchor robustness |
| B-7 | low | config.rs:101 | Backend / data-model |
| B-8 | low | keychain_write.rs:65 | Backend / secrets |
| B-6 | nit | keychain_write.rs:44 | Backend / secrets (atomicity) |
| B-10 | nit | secrets.rs:120 | Backend / secrets |
| B-14 | nit | cursor.rs:69 | Backend / secrets |
| B-15 | nit | codex.rs:206 | Backend / normalization |
| B-16 | nit | copilot.rs:218 | Backend / normalization |
| F-1 | low | Dashboard.tsx:194 | Frontend / render-logic |
| F-7 | low | AccountCard.tsx:220 | Frontend / components |
| F-4 | low | format.ts:62 | Frontend / render-logic |
| F-9 | low | AddAccountDialog.tsx:93 | Frontend / state-hooks (merged) |
| F-5 | low | SettingsDialog.tsx:93 | Frontend / components |
| F-2 | nit | useSnapshot.ts:44 | Frontend / state-hooks |
| F-3 | nit | types.ts:79 | Frontend / contract |
| F-6 | nit | status.ts:14 | Frontend / render-logic |
| F-8 | nit | format.ts:107 | Frontend / render-logic |
| F-10 | nit | InspectorSettings.tsx:184 | Frontend / components |
| F-11 | nit | index.css:210 | Frontend / components |
| F-12 | nit | ErrorState.tsx:17 | Frontend / components |
| F-13 | nit | en.json:28 | Frontend / i18n |
| X-1 | medium | store.rs:74 | Cross-cutting / security |
| X-2 | low | keychain_write.rs:20 | Cross-cutting / security |
| X-3 | low | oauth_login.rs:220 | Cross-cutting / security |
| X-4 | low | ci.yml:42 | Cross-cutting / build-CI |
| X-5 | nit | ci.yml:23 | Cross-cutting / build-CI |
| X-6 | nit | tauri.conf.json:24 | Cross-cutting / build-CI |
| X-7 | nit | package.json:6 | Cross-cutting / build-CI |

*Total: 36 confirmed findings (1 high, 2 medium, 17 low, 16 nit).*
