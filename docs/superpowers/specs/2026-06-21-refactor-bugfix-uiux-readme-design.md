# Refactor / Bugfix / UI-UX Polish / README — Design Spec

Date: 2026-06-21
Status: Approved (scope + ordering + decisions confirmed)

## Goal

Improve the existing `ai-usage-tracker` codebase across four axes, derived from a
parallel read-only investigation (4 agents: Rust, frontend-logic, UI/UX, README):

1. Fix confirmed bugs (behavior-correcting).
2. Remove code duplication and dead code (behavior-preserving).
3. Refactor over-large / tangled units (behavior-preserving).
4. Incremental UI/UX polish (a11y, contrast, consistency — **no redesign**).
5. Bring `README.md` fully up to date with the current code.

## Locked decisions

- **UI/UX = incremental polish only.** Keep current layout + information architecture.
  A prior full visual redesign ("Cockpit") was rejected — do NOT re-attempt one.
- **Bug scope = confirmed bugs only.** Refactor/dedup must be behavior-preserving.
- **Strategy = parallel investigation (done) → sequential verified chunks.**
- **Phantom `⌘K` badge → wire it up** (focus the search input on ⌘K), not remove.
- **Do all 6 chunks, in the order below.**

## Workflow (per repo conventions)

- One branch per chunk. Commits carry **no AI/Co-Authored-By watermark**.
- Verify each chunk before merging:
  - Rust: `cargo fmt --check` + `cargo clippy --all-targets -- -D warnings` +
    `cargo test --lib` (via `--manifest-path src-tauri/Cargo.toml`).
  - Frontend: `pnpm exec tsc --noEmit` + `pnpm test` + `pnpm build`.
  - UI changes: before/after screenshots via the Vite demo path (`pnpm dev` on :1420,
    set `localStorage['ait-lang']` to `en`/`ko` to check both locales).
- Merge each green chunk to `main` (fast-forward), delete the branch.
- Final gate: one batch of parallel, multi-angle read-only review agents.

## Verified negatives (do NOT "fix" these — they are correct as-is)

- No panic on malformed provider JSON (all `unwrap`/index are `#[cfg(test)]`-only;
  production normalization uses `Option`/`serde(default)`/checked parses).
- Claude 401-vs-429 refresh logic is correct (`claude.rs:544` refreshes only on 401,
  never on 429/403 — a quota signal must not burn the rotating refresh token).
- Codex stored-account expiry guard is fine (Codex add-account uses browser-OAuth which
  sets `expires_at`; the `expires_at:0` device-code path is wired only to Copilot, whose
  tokens never expire).
- `useSnapshot.ts` `loadingRef`-vs-state pattern is correct (the ref prevents an
  out-of-React event race) — do NOT "simplify" to plain state.
- `Dashboard.tsx` threshold-crossing toast logic (`148-178`) is correct.
- `ProviderConfig.enabled` IS consumed by the backend (`commands.rs:41-44`
  `build_providers` filters via `cfg.enabled_array()`) — the toggle works; not dead UI.
- `Dashboard.tsx` is already decomposed (441 LOC); the "~1520-line god component" memory
  is stale (git `bf5793f` already split it).

---

## Chunk 1 — Rust bug fixes (behavior-correcting)

All in `src-tauri/src/`. Add tests where a test gap enabled the bug.

1. **`store.rs:140-168` (`update`) — keychain-write-failure desync [med-high].**
   `update` persists `accounts.json` metadata even when the keychain secret write fails,
   and returns `found` (id-exists) rather than write-success. On a failed token rotation
   this advances `expires_at` in the file while the keychain keeps the old secret →
   permanent desync; `fetch_credential`'s guard (`mod.rs:97,100`) can never observe it.
   → Only `persist_records` after `store_secret` succeeds; return write-success so the
   caller's guard fires. (Stored-account analog of the P0#1 fix already applied to the
   auto-detect paths.)
   - **Test gap:** the in-memory keychain backend (`keychain.rs:62-83`) can't fail, so
     add a failing-backend fixture asserting metadata is NOT advanced on secret-write
     failure and that `update` reports the failure.
2. **`store.rs:107-168` — unlocked read-modify-write of `accounts.json` [low-med].**
   `add`/`update`/`remove` are unsynchronized; an overlapping manual `refresh_now`
   (`commands.rs:104`) + poll loop interleaves rewrites → lost update.
   → Serialize store mutations behind a process-wide `Mutex`, and/or atomic write+rename
   like `config.rs::save`.
3. **`scheduler.rs:13-34` — restart race [low].** `restart`→`start` runs an immediate
   `refresh_once` with no generation check; the superseded task may still be mid-fetch.
   → Check generation before the immediate fetch / guard the snapshot+store write to the
   current generation only.
4. **`oauth_login.rs:92-101,120-123,235-251` — cancel-flag lifecycle [low].** A second
   login overwrites `ACTIVE` without cancelling the first (orphan holds port 1455 to its
   300s timeout); `ACTIVE` is never cleared on success/error.
   → On `start`, cancel the previous flag before replacing; clear `ACTIVE` in `run_server`
   on exit.

---

## Chunk 2 — Rust dedup / refactor (behavior-preserving)

1. **OAuth refresh triplication [med].** `claude.rs:293-315`, `codex.rs:401-423`,
   `gemini.rs:308-334` are near-identical POST-token-and-parse bodies.
   → Shared `oauth::post_token_refresh(http, url, body, content_type) -> Result<Value, ProviderError>`;
   each provider maps its own fields. Extend the `oauth_login.rs::spec_for` table pattern
   to carry refresh endpoint/client metadata.
2. **`build_refreshed_cred` / `apply_refresh` shape [med].** `codex.rs:376-399`,
   `gemini.rs:433-452`, `claude.rs:347-366` reconstruct `StoredCredential` the same way.
   → One generic `apply_refresh(cred, new_access, new_refresh, new_id_token, expires_at)`;
   each provider only computes `expires_at` (JWT-exp vs `expires_in`).
3. **`capitalize` ×3 [low].** `codex.rs:234-240`, `zai.rs:313-319`, nested in
   `claude.rs:112-118`. → Single `crate::util::capitalize`.
4. **HTTP POST boilerplate [low].** Add `http::post_json(client, url, headers, body)`
   mirroring `get_json`; collapse manual `post(...).header(...).send()...send_for_json`
   in cursor/copilot/zai/gemini/claude-web.
5. **Codex id_token extraction dup [low].** `oauth_login.rs:316-340` vs
   `login.rs:226-244`. → Shared `codex_identity_from_id_token(jwt)`.
6. **gemini post dup [low].** `gemini.rs:74-92` method vs `gemini.rs:359-375` free fn —
   have the method delegate (or drop it).
7. **`fetch_with` 4-signature unification [med].** The four incompatible signatures
   (`mod.rs:115-141`) are the real "add-a-provider-touches-many-arms" cost.
   → One `fetch_stored(http, &StoredCredential) -> Result<ServiceUsage, ProviderError>`
   per provider; `fetch_credential` dispatch collapses to a table/trait method. Keep
   `Cursor => Err(NotLoggedIn)` as the explicit escape hatch.
   - **Test gap:** `fetch_credential` (sole consumer of the divergent signatures) has no
     test — add coverage before/with the refactor so it isn't "flying blind."
8. **`refresh_stored` 6-arm collapse [low].** Once (1) lands, the three no-op providers
   (cursor/copilot/zai) become a default; the three real ones use the shared helper.
9. **`claude.rs` (965 LOC) split [med].** Extract the claude.ai web/session-key client
   (`588-729`) → `claude/web.rs`; the macOS `/usr/bin/security` keychain write
   (`411-476`) → `claude/keychain_write.rs`.
10. **`oauth_login.rs:147-159 run_server` (10 positional args) [low].** Pass an owned
    `RunCtx` struct (already half-assembled at `124-127`).

---

## Chunk 3 — Frontend dead-code + dedup (behavior-preserving)

Dead exports (TS doesn't flag unused exports; `tsc` stays green) — delete + trim the
associated tests:

| Symbol | Location | Test to trim |
|---|---|---|
| `summarizeServices` + `StatusSummary` | `lib/status.ts:7-18,40` | `status.test.ts:56-81` |
| `cardWindows` | `lib/providers.ts:110` | — |
| `severityBarClass` (dup of `statusFillClass`) | `lib/format.ts:16` | — |
| `providerOrder` (fn) | `lib/providers.ts:49` | `providers.test.ts:97-110` |

Dead fields:
- `inspectorModel.ts:27,136` `AccountRow.resetLabel` (always `null`, never read) → remove.
- `inspectorModel.ts:47-50,109-117` unrendered `InspectorSummary` fields
  (`overallPercent`/`resetLabel`/`primaryUsedLimit`/`metricCards`, fed the removed
  Overview tab) → remove + trim `inspectorModel.test.ts:90-101`.
- `inspectorModel.ts:23,132,172` `AccountRow.providerName` duplicates `title` → use
  `title` in `rowMatchesQuery`, drop the field.

Dedup → shared helpers:
- **`<UsageBar percent tone size>` component** — the progress-bar markup is reimplemented
  ~4× (`AccountCard.tsx:150-157,224-231`, `PopoverDashboard.tsx:116-130`,
  `AccountDetailDialog.tsx:152-156,554-569`). Behavior identical; height/duration via prop.
- **`patchProviderConfig(config, provider, patch): AppConfig`** in `providers.ts` —
  collapses the duplicated tuple-splice immutable update in `SettingsDialog.tsx:78-84`
  and `AccountDetailDialog.tsx:434-439`, and centralizes the load-bearing
  `as AppConfig["providers"]` cast (the only genuine production type holes,
  `SettingsDialog.tsx:81` + `AccountDetailDialog.tsx:436`).
- **`windowTone(window, nowMs)`** helper for the repeated
  `severityToStatus(percentSeverity(pct))` + `formatResetShort(...)` derivation (4 sites).
- **`allServiceWindows(service)`** in `helpers.ts` — the `[...windows, ...detail_windows]`
  built twice in `AccountDetailDialog.tsx:160,424`.
- **`displayAccountId(service)`** in `helpers.ts` — centralize `auto:`/`stored:` prefix
  stripping (`AccountCard.tsx:112`).

---

## Chunk 4 — Frontend structural refactor (pure file moves)

Split `AccountDetailDialog.tsx` (615 LOC) into `src/components/dashboard/detail/`:
- `LimitsTab.tsx` ← `LimitsTab` (278-301) + `WindowRow` (303-336)
- `SessionsTab.tsx` ← `SessionsTab` (338-393)
- `RawTab.tsx` ← `RawTab` (395-409)
- `InspectorSettings.tsx` ← `InspectorSettings` (411-552)
- `primitives.tsx` ← `MiniBar` (554-569), `InfoLine` (571-586), `MenuItem` (588-615)
- `AccountDetailDialog` + `DetailPanelContent` stay as the orchestrator (drops to ~220 LOC).

Each component is already self-contained with explicit props → no logic change.
(Skip the optional `DetailContext` for the 14-prop drill — YAGNI for now.)

---

## Chunk 5 — UI/UX incremental polish

### a11y
- **Progressbar ARIA** on card + detail bars (match the popover which already has it):
  `AccountCard.tsx:150-157,224-231`, `AccountDetailDialog.tsx` MiniBar (`326,554-568`).
  Add `role="progressbar"` + `aria-valuemin/max/now` + `aria-label={window.label}`.
- **Kebab/more-actions menu** (`AccountDetailDialog.tsx:198-228`): add `role="menu"` +
  `role="menuitem"` (`601-614`), focus first item on open, Up/Down/Home/End + **Esc
  (closes only the menu, returns focus to trigger)** + outside-`pointerdown` dismiss.
  (Minimal acceptable floor: `role`s + Esc + outside-click.)
- **Inspector tabs** (`AccountDetailDialog.tsx:232-248`): `role="tablist"`/`tab` +
  `aria-selected` + `aria-controls` + Left/Right nav.
- **Segmented sort** (`SettingsDialog.tsx:308-336`): `role="radiogroup"`/`radio` +
  `aria-checked`.
- **Localize the dialog Close label** (`ui/dialog.tsx:42-44`, hardcoded English "Close")
  → add `common.close` to en/ko; wire via `aria-label` from callers or `useTranslation`.
- **Status dots**: `aria-hidden` on decorative dots; sr-only status text on
  `ProviderIconTile.tsx:26-28` so card status isn't color-only.
- **Polite live region** (single visually-hidden node) announcing significant usage
  changes only (not every 1s tick).

### contrast / tokens (`src/index.css`)
- **`--text-faint #737780` fails AA** for 11-12px body text on all surfaces (~3.0-3.9:1).
  → bump to ~`#969aa2` (verify ≥4.5:1 vs the lightest surface `--surface-2 #2a2e33`).
- **Drop `/70`** on "· Resets in {t}" lines (`AccountCard.tsx:135,215`,
  `PopoverDashboard.tsx:135`) — worst contrast (~2.6:1).
- **Selected segment/toggle accent `#2c84d8`** behind white 11px text ~3.4:1 → raise accent
  luminance until white clears 4.5:1.
- **Token consolidation**: introduce one `--accent` (the three ad-hoc blues `#73b8f4`/
  `#4b9bea`/`#2c84d8`) and reuse surface tokens for the four ad-hoc modal grays
  (`#1b1d20`/`#202225`/`#1a1d20`/`#25272b`).

### consistency / micro-interactions / UX
- Unify horizontal padding rhythm (the `px-5` count row/footer vs `px-4` elsewhere).
- Replace inline `style={{fontSize}}` with the `text-*` token classes (AddAccountDialog,
  EmptyState, ErrorState, Toaster) so line-heights stay consistent.
- Guard `PopoverShell.tsx:13-21` WAAPI animate with
  `prefers-reduced-motion` (CSS block doesn't cover WAAPI).
- Refresh overlay (`Dashboard.tsx:203-207`) blanks good data on every background poll →
  only show the blur on user-initiated refresh.
- `title=` tooltips on truncated text (`AccountCard.tsx:104,111`, `AddAccountDialog.tsx:356`,
  `AccountDetailDialog.tsx:170`).
- Disabled "Remove account" for auto accounts (`AccountDetailDialog.tsx:219-224`) → add an
  explanatory `title` (new locale key).
- Footer "Updated" check icon (`Dashboard.tsx:245-248`) → suppress / warn glyph when
  `error != null`.
- `group-focus-visible:opacity-100` on the card "View details" hint (`AccountCard.tsx:172`).
- **Wire `⌘K`** (`Dashboard.tsx:382-384`): window keydown (metaKey+K) → focus the search
  input (add an input ref).

### OUT OF SCOPE (noted, not now)
Skeleton loading cards; swapping the kebab/segmented/tabs to Radix primitives; settings
left-nav scrollspy; app-wide cursor unification. These are redesigns/restructures.

---

## Chunk 6 — README + docs update

Reflects the final post-refactor state, so it runs last.

1. **Fix the structurally-corrupted provider table (`README:8-17`)** — the blockquote is
   cut off mid-sentence, there is no table header/separator, and the **Claude and Codex
   rows are missing entirely**. Rebuild a proper table with a header + all 6 rows
   (Claude, Codex, Gemini, Copilot, Cursor, z.ai), values from the providers/secrets code.
2. **Test counts**: frontend `37 → 42` (`pnpm test`); Rust `82` is correct, leave as-is.
3. **Architecture diagram (`README:79-99`)**: add the omitted modules (`keychain`,
   `config`, `commands`, `jwt`, `http`); rename "Provider trait" → `ProviderApi`.
4. **Tray (`README:92`)**: left-click opens the **popover** (not the dashboard); document
   `PopoverDashboard`/`PopoverShell`.
5. **Config (`README:105`)**: `AppConfig { poll_seconds, providers: [ProviderConfig; 6] }`,
   each `ProviderConfig` carrying `enabled`/`custom_name`/`notify_thresholds`/
   `primary_window`/`sort_index`.
6. **Add missing sections**: i18n (en/ko, react-i18next, `ait-lang`, header toggle);
   CI (3-OS matrix `ci.yml` + `build-smoke.yml`) — and fix `README:125` which now wrongly
   says Linux/Windows are not CI-verified; at-rest security (OS keychain via keyring v3,
   `accounts.json` metadata-only, CSP); in-app threshold alerts (`notify_thresholds`,
   **in-app toast only — not OS notifications**); multi-account inspector UI.
7. Minor: Claude refresh primary/fallback wording (`README:122-123`) is inverted vs code
   (`claude.rs:266-267` uses `platform.claude.com` PRIMARY, `api.anthropic.com` FALLBACK).

---

## Open follow-ups (not in this round)

- Concurrency tests for scheduler generation + overlapping store mutations (added minimally
  in Chunk 1; broader coverage later).
- Cross-OS tray positioning (`tauri-plugin-positioner`), packaging/signing — separate round.
