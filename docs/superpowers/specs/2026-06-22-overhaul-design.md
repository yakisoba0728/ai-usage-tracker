# AI Usage Tracker — Overhaul Design

Date: 2026-06-22
Status: **Approved — implementing (Option A, value-first)**. Owner confirmed: `yakisoba0728`.
Supersedes (for Claude): `2026-06-21-window-anchoring-design.md` (its Claude OAuth `/v1/messages` anchor approach is now retired).

## 1. Context & goals

A single large request: fix the live anchor failures, fix UI/UX bugs, add new features (launch-at-login, GitHub-release auto-update notifier, OS notifications), and do a **full rewrite / refactor** of the codebase — UI/UX included.

The diagnostic workflow (`ait-diagnose`, 5 finders, current-code + git-log, treating `docs/audit/2026-06-22-deep-audit.md` as stale) found:

- **Baseline is GREEN and healthy.** 154 Rust lib tests + 76 frontend vitest cases pass. Most audit items (B-1/2/4/5/11/12/3/13, F-1…F-13, X-1/2/3) are **already fixed** in current code. The tree is **not a tangle** — `lib/` is small tested pure modules, every Rust provider has a golden `normalize()` fixture, and the IPC contract is test-pinned three ways.
- **55 open findings**, clustered exactly on the user's decisions: Claude anchor (session-key model), per-account identity, anchor success-validation, and the three net-new features.

## 2. Guiding principle: reconstruction behind frozen invariants

The user chose **전면 재작성 (full rewrite)** over a bounded refactor. We honor that — but we execute it as **module-by-module reconstruction behind frozen invariants**, not a ground-zero rewrite. The reasoning is evidence-based: the diagnostic showed the code is not tangled and ships 230 passing tests. A literal from-scratch rewrite would throw away that verified safety net and deliver the user's urgent fixes *last*. Instead, each module is rebuilt against contracts that stay frozen, so every step is verifiable.

Two disciplines make this safe:

- **D1 — Freeze the invariants (§3).** They are the regression net; they do not change except where a decision explicitly requires it.
- **D2 — Separate restructure from behavior change.** For every module: (a) restructure with all tests staying green, then (b) apply any behavior change (Claude session-key anchor, z.ai body-check, config reshape, every bug fix) as its **own** step, gated by a **new failing→passing test written first**. Never ship "I rewrote it and fixed it" in one diff — you lose the ability to tell which change broke what. **A bug fix is a behavior change.**

## 3. Frozen invariants (the safety net)

These stay test-pinned and unchanged throughout (except the two explicitly-marked decision deltas):

1. **IPC JSON shapes** — `model.rs::service_usage_json_shape_matches_ts_contract` / `service_error_json_shape_matches_ts_contract` ↔ `src/lib/types.ts`. *(Delta: `AppConfig` gains per-account settings + new toggles — versioned, §6.2.)*
2. **Event + command catalog** — events `usage-updated`, `provider-loading`, `anchor-result`, `refresh-result`, `login-complete`, `trigger-refresh`; the 12 `invoke` handlers in `lib.rs:122-135` ↔ `src/lib/ipc.ts`. *(Delta: `anchor-result` payload gains `provider`+`label`; new commands `rename_account`, `set_launch_at_login`, `check_update_now`.)*
3. **`ProviderApi` trait + `ProviderError` taxonomy** (`providers/mod.rs`) with `provider_error_codes_are_stable`. Frozen.
4. **Tested `lib/` model modules** (`dashboardState`, `accountActionState`, `snapshotState`, `addAccountState`, `thresholdToasts`, `inspectorModel`, `format`, `status`, `providers`) — kept as the decision-logic net; components are rebuilt around them.
5. **Backend reliability invariants** (keep verbatim): per-id serialized refresh + re-read-inside-lock (B-1), atomic owner-only writes via `util::write_atomic`, corrupt-file preservation, per-provider failure isolation in `fetch_all` (unwinding panics — no `panic=abort`).

## 4. Scope — workstreams

Deduped from 55 findings into workstreams. Severity from the diagnostic.

### Bugs (behavior changes)
- **BUG-1 (critical)** Claude anchor 401 — session key posted as OAuth Bearer to `/v1/messages`. → web-chat anchor (§6.1).
- **BUG-2 (high)** Rename-all — `custom_name`/`primary_window`/`sort_index` are per-provider; editing one account changes all accounts of that provider. → per-account schema (§6.2).
- **BUG-3 (medium)** z.ai anchor validates only HTTP status, not body — 200+in-band-error reads as success. → generalized anchor success contract (§6.3).
- **BUG-4 (medium)** `auto:claude` anchor skips refresh — **moot**, deleted with Claude OAuth removal (§6.1).
- **BUG-5 (low)** GitHub device-login uses `Bearer` where `gho_` tokens need `token` scheme → label degrades. → use `token` scheme (`login.rs:426`).
- **BUG-6 (low)** Raw `stored:<uuid>` leaks into card subtitle when label is null. → provider label + short id / custom_name.
- **BUG-7 (nit)** ConfirmDialog dismiss says "Close" not "Cancel".

### Features
- **FEAT-1 (critical schema)** Per-account settings + config migration (§6.2) — overlaps BUG-2; the migration is the load-bearing data-loss gate.
- **FEAT-2 (high)** Claude session-key-only + web-chat anchor (§6.1) — **gated spike**.
- **FEAT-3 (high)** Anchor notification: OS-native (`tauri-plugin-notification`) + in-app toast, naming provider + account (§6.4).
- **FEAT-4 (high)** Launch-at-login (`tauri-plugin-autostart`) (§6.5).
- **FEAT-5 (medium)** GitHub-release auto-update notifier (§6.6).

### Reconstruction (refactor, behavior-preserving)
- Backend: split `commands.rs` (ipc / refresh / snapshot_merge / state); decompose `oauth_login.rs` + dedup `login.rs` cancel/finish; split each provider into `creds/parse/refresh/mod`; `anchor.rs` into `send/resolve/cooldown`; extract shared `paths.rs`; collapse Claude to `web.rs` + session-key.
- Frontend: extract Dashboard engines into hooks (`useAccountActions`, `useActionResultEvents`, `useThresholdToasts`, `useToasts`); extract `useAddAccountFlow` + view split; `toneForPercent` helper; move sub-components to sibling files.

## 5. Execution order — **Decided: Option A (value-first)**

**Decision (2026-06-22): Option A.** The user chose value-first so the live bugs (Claude 401, z.ai, rename-all) are fixed before the structural reconstruction. This was the one fork, because it changes *when the live pain is fixed*.

A pure leaves-first reconstruction (Chunk 0→8) delivers the Claude/z.ai/rename fixes in the **middle** chunks — i.e. the user keeps hitting them while structural work lands first. Two options:

- **Option A — Value-first (recommended).** Land the urgent bug fixes and the new features as **early, standalone chunks** (each with its own characterization test + behavior-change discipline D2), then do the structural reconstruction underneath. The user gets working anchors / rename / features fast; the rewrite proceeds without blocking relief.
- **Option B — Leaves-first.** Strict structural order (util→model→config→providers→anchor→commands→frontend→features). Cleaner module-by-module story, but the live fixes land late.

**Recommendation: A.** The frozen invariants make it safe to fix-then-reconstruct: a fix shipped against the current structure stays green when the module is later rebuilt, because the tests move with it.

### Proposed chunk plan (Option A)

- **Chunk 0 — Safety nets first (prereq).** Add the missing characterization tests *before* any change: (i) config schema_version + old-shape migration fixtures (§6.2, the critical data-loss gate); (ii) per-account name isolation test (two accounts, independent names); (iii) new Claude web-anchor request-shape wiremock test; (iv) B-1 concurrent-refresh end-to-end test; (v) decide component-test approach (extract-logic-to-lib, §8). Freeze IPC-contract tests as the gate.
- **Chunk 1 — Claude spike + anchor (FEAT-2/BUG-1).** **GATED:** live spike first (§6.1). If green → implement web-chat anchor + remove Claude OAuth/keychain auto-detect + `auto:claude` migration. If red → fallback to Claude view-only (no anchor), revisit with user.
- **Chunk 2 — Anchor robustness (BUG-3) + notification (FEAT-3).** Generalized "did the send consume the window" success contract for z.ai/Codex/Claude; enrich `anchor-result` with provider+label; OS notification + toast.
- **Chunk 3 — Per-account settings (FEAT-1/BUG-2).** Config data-model reshape + migration (guarded by Chunk-0 tests), `rename_account` command, UI in AccountDetailDialog, frontend resolvers keyed by service_id.
- **Chunk 4 — New features (FEAT-4/FEAT-5) + misc bugs (BUG-5/6/7).** Autostart, update notifier, small fixes.
- **Chunk 5 — Backend reconstruction.** providers (parallel), `commands.rs` split, `oauth_login`/`login` decompose, `anchor` split, `paths.rs`. Behavior-preserving (D2).
- **Chunk 6 — Frontend reconstruction.** Extract hooks, split Dashboard/AddAccountDialog, view components, UI/UX polish. Behavior-preserving.
- **Chunk 7 — CI / regression hardening.** eslint `--max-warnings 0` gate, Windows cfg path tests, live-anchor harness + manual checklist doc.

Each chunk = its own branch, `pnpm verify:runtime` green, merge to main (per workflow prefs). Anchor/notification chunks additionally require the manual live checklist (§8).

## 6. Feature & bug designs

### 6.1 Claude session-key-only + web-chat anchor — **GATED SPIKE + fallback**

The session-key anchor path is reverse-engineered from a single browser capture; it is **only verifiable live** and may fail on Cloudflare (`cf_clearance`/`__cf_bm`), the `anthropic-device-id` requirement, or body shape. So Chunk 1 opens with a **live spike**, and carries a fallback.

**Spike (gate):** a throwaway live call sequence proving the three steps work with a session-key cookie:
1. `GET https://claude.ai/api/organizations` → `org.uuid` (reuse `web.rs::fetch_with_session_key` parse; factor `fetch_first_org_uuid`).
2. `POST .../organizations/{uuid}/chat_conversations` `{"uuid": <client v4>, "name": ""}` → conversation uuid.
3. `POST .../organizations/{org}/chat_conversations/{conv}/completion` with a minimal `{prompt:".", parent_message_uuid:…}` body; headers `anthropic-client-platform: web_claude_ai`, `anthropic-device-id: <stable per-install uuid in config>`, `Origin`/`Referer: https://claude.ai`, `Accept: text/event-stream`; **forward the full pasted cookie** (sessionKey + cf_clearance/__cf_bm if present) rather than rebuilding `sessionKey=` only; realistic `User-Agent`.
4. Drain SSE; success = ≥1 assistant `completion`/`message_delta` frame and no `event: error`/`{"error":…}`.

**If spike green:** implement `send_claude_web(http, cookie)` in the Claude module; route the `stored:` Claude arm to it; **delete** `send_claude`, `resolve_claude_auto`, `CLAUDE_MESSAGES_URL/VERSION/OAUTH_BETA/MODEL`, and the old OAuth wiremock test (replace with web-anchor test). Remove Claude OAuth/keychain auto-detect: `ClaudeProvider` becomes session-key-only (no `auto:claude` card), delete `keychain_write.rs` + OAuth/refresh/`write_back`/`fetch_with` in `claude/mod.rs` + the keychain branch in `secrets.rs`; drop `local-session` from Claude in `addAccountOptions.ts`. **Migration:** auto accounts are never persisted, so nothing on disk to migrate; one-time config migration drops any `auto:claude` key from `auto_anchor`.

**If spike red:** Claude degrades to **view-only** (session-key fetch keeps working; anchor button hidden for Claude). Surface to user; do not build the chunk on an unproven foundation.

**Cloudflare handling:** classify a 403-with-Cloudflare-body as a distinct **non-transient** failure (keep cooldown) and toast "refresh your Claude cookie".

### 6.2 Per-account settings + migration — **CRITICAL data-loss gate**

Root cause: `custom_name`/`primary_window`/`sort_index` live in `ProviderConfig` inside `[ProviderConfig; 6]` indexed by provider, so all accounts of a provider share one slot (`config.rs:9-53`). `auto_anchor` is already correctly per-service_id (the shape to mirror).

**New shape:** add `accounts: HashMap<String, AccountConfig>` to `AppConfig` keyed by service_id, `AccountConfig { custom_name: Option<String>, primary_window: Option<String> }`. **Keep** `enabled` + `notify_thresholds` + `sort_index` per-provider (genuinely provider-level). Add `schema_version` (default 0).

**Migration (the gate):** `AppConfig::load_from` runs a versioned `migrate()` — for each provider slot with non-None `custom_name`/`primary_window`, seed `accounts[auto_service_id(provider)]` then clear the slot field; bump schema_version so it runs once. **Chunk 0 adds old-shape fixture tests first** (the single biggest silent-data-loss risk: today a relocated field would make old configs parse-invalid and reset every pref + sort + auto_anchor). Do the same audit for `store.rs`/`accounts.json`.

**Read/write sites to update:** `providerDisplayName`/`resolveHeadlineWindow`/`patchProviderConfig` take a service id; rename UI moves into `AccountDetailDialog` (knows service id) via a new `rename_account(id, label)` command (mutates `StoredCredential.label` atomically, or `AccountConfig.custom_name`); `types.ts` adds `AccountConfig`; `ipc.ts` browser-default config; tray menu optionally honors per-account name.

### 6.3 Generalized anchor success contract (BUG-3)

z.ai `send_zai` discards the parsed body, so a 200-with-error reads as success (asymmetric with Codex's `sse_has_failure`). Introduce a per-provider "**did the send consume the window**" validator: z.ai → require `choices[0].message` and reject a top-level `error`/`code`; Codex → keep `sse_has_failure`; Claude web → the SSE scanner from §6.1. Reuse it across all three so a 200-but-failed never clears the cooldown as success.

### 6.4 Anchor notification (FEAT-3)

`anchor-result` is `{id, ok, detail}` — no provider/account. Add `tauri-plugin-notification` (Cargo + npm + `notification:default` capability), register in `lib.rs`. Enrich the payload at **both** emit sites (`send_anchor_now` and the auto path) with `provider` + `label` (source from snapshot / `store::find_by_service_id`). Fire the OS notification **from Rust** (works when the window is hidden — menu-bar app) titled with provider+account+outcome; keep the in-app toast, extend `ipc.ts` payload + i18n keys to interpolate provider/account; distinguish auto vs manual. Pure text builder (input: provider+account+window+pct → string) extracted to lib and unit-tested (§8).

### 6.5 Launch-at-login (FEAT-4)

Add `tauri-plugin-autostart` (Cargo + npm + `autostart:default` capability), register in `lib.rs`. `AppConfig.launch_at_login: bool` (`#[serde(default)]`); command `set_launch_at_login(enable)` toggles `autolaunch().enable()/.disable()` + persists; `.setup` reconciles (re-enable if config says on but OS item removed). Toggle row in SettingsDialog "General". macOS LaunchAgent / Windows Run key (both plugin-supported); document the unsigned-app Gatekeeper note.

### 6.6 Auto-update notifier (FEAT-5)

No new plugin (uses reqwest + opener + notification). `check_for_update(app)` GETs `api.github.com/repos/yakisoba0728/ai-usage-tracker/releases/latest` (UA + `Accept: application/vnd.github+json`), compares `tag_name` (strip `v`) to `env!("CARGO_PKG_VERSION")` with a small numeric semver compare. If newer → OS notification "Update available {version}", click opens `html_url` via opener. Check at startup + a long interval (24h spawn or every-N-polls in scheduler); store last-notified version to avoid repeats. `AppConfig.auto_update_check: bool` (default true) + SettingsDialog toggle + manual `check_update_now` command + "Check for updates" button. No CSP change (reqwest runs in Rust, not webview).

### 6.7 Misc bugs

BUG-5 `login.rs:426` use `Authorization: token {t}`. BUG-6 fallback to provider label + short id (or custom_name once §6.2 lands). BUG-7 add `common.cancel` key for ConfirmDialog dismiss.

## 7. Reconstruction map (chunks 5–6)

Behavior-preserving (D2). Backend: `commands.rs`→`ipc/`+`refresh.rs`+`snapshot_merge.rs`+`state.rs`; `oauth_login.rs`→`oauth/{spec,pkce,callback_server,exchange,credential,html}` + shared `auth/cancel_token.rs`+`auth/finish.rs` with `login.rs`; each provider→`<p>/{creds,parse,refresh,mod}` with a shared `oauth_refresh` helper; `anchor.rs`→`anchor/{send/{claude,codex,zai},resolve,cooldown}`; extract `paths.rs` (shared dir-private policy). Keep `store.rs` single (cohesive). Frontend: hooks `useAccountActions`/`useActionResultEvents`/`useThresholdToasts`/`useToasts`/`useAddAccountFlow`; view splits `ProviderRail`/`AuthOptionList`/`SessionKeyPanel`/`DeviceCodePanel`/`AddedAccountsList`; `toneForPercent` in `status.ts`; Dashboard/AddAccountDialog become thin shells. Providers rewrite in parallel (isolated behind trait + golden fixtures).

## 8. Regression-safety strategy

- **Characterization-first (Chunk 0):** config migration old-shape fixtures; per-account name isolation; Claude web-anchor request shape; B-1 concurrent-refresh e2e; Windows cfg path resolution (run on Windows CI leg via `AIT_*_PATH` env hooks).
- **Component logic → lib:** keep the "logic-in-lib" discipline — extract each rewritten component's decision logic (which name shows, toast/notification text, which threshold fires for which account) into a tested `lib/` pure module **before** rewriting JSX (cheaper than adding testing-library; option to add happy-dom only if a flow can't be cleanly extracted).
- **Live-only paths:** Claude web anchor success, z.ai send success, Codex no-in-band-failure, and the OS notification naming the right account are **live-only**. Add `examples/live_anchor.rs` (env-gated) and a documented **manual live checklist** as a required pre-merge gate for any anchor/notification chunk.
- **Bug fix = new test first:** every BUG-* lands as failing→passing (D2).

## 9. Per-chunk verification gate

1. `pnpm verify:runtime` green (lint, `tsc --noEmit`, vitest, `cargo test --lib`).
2. `cargo clippy --all-targets -- -D warnings` green (macOS; Windows CI leg for cfg changes).
3. `pnpm build` for frontend chunks; `pnpm tauri build --debug --no-bundle` for chunks touching Tauri wiring/plugins.
4. Anchor/notification/autostart chunks: run the manual live checklist.
5. Branch-per-chunk, verify green, merge to main. No AI watermark in commits.

## 10. Risks & open items

- **Claude web anchor may be infeasible** (Cloudflare / device-id / body). Mitigated by the §6.1 spike-gate + view-only fallback. **Highest uncertainty.**
- **Config migration is the top silent-data-loss risk** — Chunk 0 fixtures gate it; nothing reshapes config before they exist.
- **Unsigned app**: autostart + notifications work but may show Gatekeeper/permission prompts; auto-update stays notifier-only (no silent install).
- **Repo** for the update endpoint: `yakisoba0728/ai-usage-tracker` (confirmed).
- **Full-rewrite scope vs time:** Option A front-loads value; the reconstruction chunks (5–6) are the bulk and can proceed incrementally without blocking the user.
