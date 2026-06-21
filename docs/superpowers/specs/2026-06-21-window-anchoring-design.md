# Usage-Window Anchoring ("send-to-reset") — Design Spec

**Date:** 2026-06-21
**Status:** Approved design (user said "ㄱㄱ"); ready for implementation plan.

## Goal

Let the user control *when* a provider's rolling usage window resets, by sending
a minimal throwaway message that anchors (starts) a new window. Per-account, with
(a) a manual "send now" button and (b) optional auto-fire when the window is
empty/available to re-anchor. This is the app's **first write path** — a
deliberate, user-approved departure from the read-only / "real data only"
principle.

## Scope (per-provider — confirmed from a full live raw dump)

| Provider | Anchorable rolling window? | Feature |
|---|---|---|
| **Claude** | ✅ 5-hour rolling | Manual + auto |
| **Codex** | ✅ 5-hour rolling (+ finite native reset-credits) | Manual + auto — **send mechanism needs a spike** |
| **z.ai** | ✅ 5-hour (no `resets_at`) + Weekly/Monthly | Manual + auto (5-hour anchored via the empty signal) |
| **Copilot** | ❌ monthly calendar quota | **"Not supported"** (shown, disabled, with reason) |
| **Gemini** | ❌ daily calendar quota | **"Not supported"** |
| **Cursor** | ❌ monthly billing + no send path | **"Not supported"** |

"Not supported" providers must be **shown and clearly marked** (the app's honest
state principle), not silently hidden.

## Trigger (auto mode)

The watched window is each provider's **5-hour** window (label `"5-hour"`).

**Auto-fire condition:** the 5-hour window is *empty / available to re-anchor* —
i.e. `used_percent == 0` (equivalently 100% remaining). Rationale:

- A window that just expired/reset reads `used == 0`; sending then anchors a
  fresh window so the reset cadence becomes predictable (the user's "윈도우 만료
  감지 → 리셋" intent).
- z.ai's 5-hour window carries **no `resets_at`**, so `used == 0` is the only
  available signal — the user explicitly asked for "100%일 때 한 번 보내기".
- Using one rule for all three is simplest and robust to the uncertain
  `resets_at` semantics of Claude/Codex when a window has lapsed.

**Guard ("한 번"):** an in-memory per-`service_id` cooldown (default **600 s**).
After a successful send, record the timestamp; skip re-sending within the
cooldown. (After sending, `used > 0`, so the condition naturally clears on the
next poll; the cooldown only covers API-lag double-reads.)

`resets_at` is still *displayed* everywhere; we just don't gate the trigger on it.

## Architecture (sending stays entirely in Rust → P0 preserved)

```
refresh_once (commands.rs)
  └─ after snapshot built, for each service:
       if auto_anchor[service.id] && service.connected && supported(provider)
          && five_hour(service).used_percent == 0
          && cooldown_ok(service.id):
            spawn anchor::send(service.id)   // non-blocking; isolated
```

### New module `src-tauri/src/anchor.rs`
- `pub async fn send(service_id: &str) -> Result<(), ProviderError>`
  - Resolves the credential the same way fetch does: `auto:*` via the provider's
    auto-detect (`secrets`/provider path), `stored:*` via `store::list()`.
  - Dispatches to a per-provider minimal-message POST (see below).
  - `supported(provider)` returns false for Copilot/Gemini/Cursor.
- `cooldown_ok(service_id)` / `mark_sent(service_id)` over a
  `static LAST_ANCHOR: Mutex<HashMap<String, i64>>`.
- Failures are isolated (a failed send never breaks the poll/scheduler).

### Per-provider send (endpoints — exact model/beta header verified in the spike)
- **Claude:** `POST https://api.anthropic.com/v1/messages`, `Authorization:
  Bearer <oauth>`, `anthropic-version`, `anthropic-beta: <oauth beta>`; body
  `{ model: <cheapest, e.g. claude-3-5-haiku>, max_tokens: 1, messages:[{role:
  "user", content:"."}] }`. (Reuses the Claude Code OAuth token the usage path
  already loads.)
- **z.ai:** `POST https://api.z.ai/api/paas/v4/chat/completions`, `Authorization:
  Bearer <api_key>`; body `{ model: <cheapest GLM>, max_tokens: 1, messages:[…] }`.
- **Codex:** **SPIKE** — determine whether a minimal message can be sent via the
  (unofficial) ChatGPT backend. The native `rate_limit_reset_credits`
  (`available_count: 2`) is **finite and force-resets** (different from
  anchoring), so it is unsuitable for auto mode. Decision after spike:
  (a) backend message → manual + auto; or (b) if infeasible, Codex becomes
  **manual-only** using a reset-credit, or **"not supported"** like the others.

### Config (`config.rs` `AppConfig`)
- Add `auto_anchor: HashMap<String, bool>` (key = `service_id`, default empty =
  all OFF). Persisted in `config.json`.
- Update the **bidirectional IPC contract** (model/config ↔ `src/lib/types.ts`
  + its contract test) so the new field stays in sync.

### Commands (`commands.rs`)
- `send_anchor_now(service_id) -> Result<(), String>` — manual button path.
- `auto_anchor` flows through existing `get_config` / `set_config`.

### Frontend
- **Detail dialog → settings tab** (`InspectorSettings.tsx`), per account:
  - Supported providers: an **"Auto-anchor window"** toggle (default OFF) + a
    manual **"Send now / 지금 윈도우 앵커링"** button.
  - Unsupported providers: a disabled row reading **"지원 안 함"** + the reason
    (calendar quota / no send path).
- **Confirmation dialog** on the manual button: warns it sends a real message,
  consumes quota, and may violate provider ToS.
- i18n keys (en/ko) for all new copy.
- Optional: a toast when an auto-send fires (`anchor-sent` event).

## Safety rails (the principle break is deliberate — carry guardrails)
- Default **OFF**; per-account explicit opt-in.
- Manual button always confirms (ToS / quota-consumption warning).
- Tokens never cross IPC; the send happens entirely in Rust.
- **First implementation step = a guarded single test-send per provider** to
  confirm endpoint feasibility (especially Claude's beta header and the Codex
  mechanism) before wiring auto mode.

## Error handling
- `anchor::send` returns `Result`; on failure → toast + do **not** `mark_sent`
  (so it retries next cycle). One provider's failure never aborts the batch or
  the scheduler (isolation invariant, same as `fetch_all`).

## Testing
- Pure-logic: trigger predicate (`used == 0`, supported-provider gate, cooldown).
- `anchor::send` request-shape tests per provider via **wiremock** (URL, headers,
  body: `max_tokens:1`, minimal message).
- Config roundtrip for `auto_anchor` + IPC contract guard.
- No live sends in the suite (the test-send is a manual implementation step).

## Decomposition
- **Spec A (display correctness):** essentially complete. All 6 providers'
  displayed values verified correct vs raw. The one requested change — z.ai card
  shows **both** Weekly + 5-hour — is **done** (commit `81bdeea`). The two
  remaining cosmetic gaps (Cursor monthly reset date, Gemini tier name) are
  **skipped (YAGNI)** unless requested.
- **Spec B (this document):** the send-to-reset feature.

## Open risks / to resolve during implementation
1. **Codex send mechanism** — spike (backend message vs reset-credit vs
   not-supported).
2. **Claude `/v1/messages` with the Claude Code OAuth token** — confirm the
   required `anthropic-beta` header + a valid cheap model via the guarded
   test-send.
3. **z.ai chat-completions model name** — confirm the cheapest valid GLM model
   for the account's plan.
4. **`used == 0` trigger semantics for Claude/Codex** — confirm via observation
   that an unused/just-reset window reads 0 and that anchoring behaves as
   expected (no over-fire beyond the cooldown).

## Global constraints (apply to every task)
- Commit messages **must NOT** contain any AI/Claude watermark.
- Branch per chunk → verify green (fmt/clippy/`cargo test` + tsc/vitest) →
  fast-forward merge to `main` → delete branch.
- `main` is **local-only** — do not push without an explicit ask.
- Tokens stay in Rust; only masked metadata + usage snapshots cross IPC (P0).
- `panic = "abort"` must NOT be set (provider isolation).
- Never print token values to the terminal/transcript.
- Replies in Korean.
