# Window-Anchoring (send-to-reset) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user anchor a provider's rolling usage window by sending a minimal throwaway message — per account, via a manual button and an optional auto-fire when the 5-hour window is empty.

**Architecture:** A new Rust `anchor` module sends a 1-token message to the provider's chat endpoint (sending stays entirely in Rust; tokens never cross IPC). A manual Tauri command drives the button; `refresh_once` evaluates an auto-trigger after each poll. Config gains a per-`service_id` `auto_anchor` map. The frontend adds a per-account toggle + confirmed manual button in the detail settings tab, and marks unsupported providers.

**Tech Stack:** Rust (Tauri 2, reqwest, tokio, serde, wiremock for tests), React 19 + TypeScript + Tailwind, react-i18next, vitest.

## Global Constraints

- Commit messages **must NOT** contain any AI/Claude watermark (no `Co-Authored-By`, no "Generated with").
- Branch per task → verify green (`cargo fmt` + `cargo clippy --lib --all-targets` + `cargo test --lib`; `pnpm exec tsc --noEmit` + `pnpm test` for frontend tasks) → fast-forward merge to `main` → delete branch.
- `main` is **local-only** — do NOT push without an explicit ask.
- Tokens stay in Rust; only masked metadata + usage snapshots cross IPC (P0). Never print token values to the terminal/transcript.
- `panic = "abort"` must NOT be set.
- Replies to the user in Korean.
- Supported providers: **Claude, Codex, z.ai**. Unsupported (marked, never silently hidden): **Copilot, Gemini, Cursor**.
- Auto-trigger rule (unified, user-approved): fire when the **5-hour** window's `used_percent == 0` (100% remaining), guarded by a 600 s per-`service_id` cooldown.
- Run all `cargo` commands from `src-tauri/` (the crate root). Run `pnpm` from the repo root.

## File Structure

- **Create** `src-tauri/src/anchor.rs` — credential resolution by `service_id`, per-provider minimal-message send, `supported()`, cooldown guard (`try_begin`/`clear`).
- **Modify** `src-tauri/src/lib.rs` — `pub mod anchor;` + register the `send_anchor_now` command.
- **Modify** `src-tauri/src/commands.rs` — `send_anchor_now` command + auto-trigger evaluation inside `refresh_once`.
- **Modify** `src-tauri/src/config.rs` — `AppConfig.auto_anchor: HashMap<String, bool>`.
- **Modify** `src/lib/types.ts` — `AppConfig.auto_anchor`; `src/lib/types.test.ts` — key assertion; `src/lib/ipc.ts` — default + `sendAnchorNow` wrapper; `src/lib/providers.ts` — `setAutoAnchor` helper.
- **Create** `src/components/ui/ConfirmDialog.tsx` — reusable confirm modal (none exists).
- **Modify** `src/components/dashboard/detail/InspectorSettings.tsx` — toggle + manual button + unsupported row.
- **Modify** `src/locales/en.json` + `src/locales/ko.json` — new copy.
- **Modify** `README.md` — document the feature.

---

### Task 1: `anchor` module + z.ai manual send + command (thinnest end-to-end slice)

Proves the riskiest assumption ("can we send at all?") on the easiest provider (z.ai stored, Bearer API key, standard chat completions), end-to-end via a manual command, before building config/trigger/UI.

**Files:**
- Create: `src-tauri/src/anchor.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod anchor;` near the other `pub mod` lines; add `commands::send_anchor_now` to the `tauri::generate_handler!` list)
- Modify: `src-tauri/src/commands.rs` (add the `send_anchor_now` command)

**Interfaces:**
- Produces:
  - `anchor::send(service_id: &str) -> Result<(), crate::providers::ProviderError>`
  - `anchor::supported(provider: crate::model::Provider) -> bool`
  - `anchor::anchor_body(model: &str) -> serde_json::Value` (pure, testable)
  - `anchor::send_zai(http: &reqwest::Client, api_key: &str, url: &str) -> Result<(), ProviderError>`
  - command `send_anchor_now(service_id: String) -> Result<(), String>`
- Consumes: `crate::http::{shared, post_json}`, `crate::store::list`, `crate::model::{Provider, stored_service_id}`, `crate::secrets`.

- [ ] **Step 1: Write the failing tests** in `src-tauri/src/anchor.rs` (create the file with only the test module + stubs that fail to compile is fine; or stub signatures returning `unimplemented!()`). Add:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Provider;

    #[test]
    fn supported_only_claude_codex_zai() {
        assert!(supported(Provider::Claude));
        assert!(supported(Provider::Codex));
        assert!(supported(Provider::Zai));
        assert!(!supported(Provider::Copilot));
        assert!(!supported(Provider::Gemini));
        assert!(!supported(Provider::Cursor));
    }

    #[test]
    fn anchor_body_is_one_token_user_message() {
        let b = anchor_body("glm-4-flash");
        assert_eq!(b["model"], "glm-4-flash");
        assert_eq!(b["max_tokens"], 1);
        assert_eq!(b["messages"][0]["role"], "user");
        assert!(b["messages"][0]["content"].is_string());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn send_zai_posts_bearer_and_one_token_body() {
        use wiremock::matchers::{body_partial_json, header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/paas/v4/chat/completions"))
            .and(header("authorization", "Bearer zk-test"))
            .and(body_partial_json(serde_json::json!({"max_tokens": 1})))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id":"x"})))
            .mount(&server)
            .await;
        let client = crate::http::build_client();
        let url = format!("{}/api/paas/v4/chat/completions", server.uri());
        send_zai(&client, "zk-test", &url).await.unwrap();
    }
}
```

- [ ] **Step 2: Run tests, verify they fail.** Run: `cargo test --lib anchor::` — Expected: compile error / FAIL (functions not defined).

- [ ] **Step 3: Implement `src-tauri/src/anchor.rs`** (above the test module):

```rust
//! Window anchoring: send a minimal throwaway message so a provider's rolling
//! usage window starts at a predictable time. The app's only write path — kept
//! entirely in Rust (tokens never cross IPC). Supported: Claude, Codex, z.ai.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::http;
use crate::model::{stored_service_id, Provider};
use crate::providers::ProviderError;

const ZAI_CHAT_URL: &str = "https://api.z.ai/api/paas/v4/chat/completions";
const ZAI_MODEL: &str = "glm-4-flash";

/// Cooldown so the auto-trigger sends at most once per window per service.
const COOLDOWN_SECS: i64 = 600;

/// Providers where anchoring is meaningful AND a send path exists.
pub fn supported(provider: Provider) -> bool {
    matches!(provider, Provider::Claude | Provider::Codex | Provider::Zai)
}

/// A minimal 1-token user message body (shared by the chat-completions shapes).
pub fn anchor_body(model: &str) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "max_tokens": 1,
        "messages": [{ "role": "user", "content": "." }]
    })
}

pub async fn send_zai(
    http: &reqwest::Client,
    api_key: &str,
    url: &str,
) -> Result<(), ProviderError> {
    let _: serde_json::Value = http::post_json(http, api_key, url, &anchor_body(ZAI_MODEL)).await?;
    Ok(())
}

/// Resolve the stored credential whose UI id is `service_id` (`stored:<id>`).
fn resolve_stored(service_id: &str) -> Option<crate::store::StoredCredential> {
    crate::store::list()
        .into_iter()
        .find(|c| stored_service_id(&c.id) == service_id)
}

/// Send an anchor message for the given UI `service_id`.
pub async fn send(service_id: &str) -> Result<(), ProviderError> {
    let http = http::shared();
    if service_id.starts_with("stored:") {
        let cred = resolve_stored(service_id)
            .ok_or_else(|| ProviderError::NotLoggedIn(format!("no stored account {service_id}")))?;
        if !supported(cred.provider) {
            return Err(ProviderError::NotLoggedIn(format!(
                "{:?} does not support anchoring",
                cred.provider
            )));
        }
        return match cred.provider {
            Provider::Zai => send_zai(&http, &cred.access_token, ZAI_CHAT_URL).await,
            // Claude/Codex stored sends are added in later tasks.
            other => Err(ProviderError::NotLoggedIn(format!(
                "anchoring for stored {other:?} not implemented yet"
            ))),
        };
    }
    Err(ProviderError::NotLoggedIn(format!(
        "anchoring not implemented for {service_id}"
    )))
}

// ── Cooldown guard ──────────────────────────────────────────────────────────
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
```

- [ ] **Step 4: Add the command** to `src-tauri/src/commands.rs` (after `remove_account`):

```rust
/// Send a minimal "anchor" message for one service to start its usage window.
/// Tokens stay in Rust; the frontend only passes the masked service id.
#[tauri::command]
pub async fn send_anchor_now(service_id: String) -> Result<(), String> {
    crate::anchor::send(&service_id).await.map_err(|e| e.to_string())
}
```

- [ ] **Step 5: Register the module + command.** In `src-tauri/src/lib.rs` add `pub mod anchor;` alongside the other `pub mod` lines, and add `commands::send_anchor_now` to the existing `tauri::generate_handler![...]` list (find the macro; append `, commands::send_anchor_now`).

- [ ] **Step 6: Run checks.** Run: `cargo fmt && cargo clippy --lib --all-targets && cargo test --lib anchor::` — Expected: PASS (3 anchor tests).

- [ ] **Step 7: Run the full suite.** Run: `cargo test --lib` — Expected: all pass (no regressions).

- [ ] **Step 8 (guarded live verification — REQUIRES USER CONFIRMATION):** This sends a real message and consumes z.ai quota. Confirm with the user first. Then, with a z.ai account stored, write a throwaway `#[ignore]` integration test (or reuse the manual command via the app) that calls `anchor::send("stored:<zai-id>")` and asserts `Ok(())`. Delete the throwaway test after. If it returns `ProviderError::Status`, inspect the message and adjust `ZAI_MODEL` (the account's plan may require a different GLM model name) until it returns 200, then re-commit.

- [ ] **Step 9: Commit.**

```bash
git checkout -b feat/anchor-zai-manual
git add -A
git commit -m "feat(anchor): module + z.ai manual send + send_anchor_now command"
git checkout main && git merge --ff-only feat/anchor-zai-manual && git branch -d feat/anchor-zai-manual
```

---

### Task 2: Claude send (auto + stored) via `/v1/messages`

**Files:**
- Modify: `src-tauri/src/anchor.rs`

**Interfaces:**
- Produces: `anchor::send_claude(http, token, url) -> Result<(), ProviderError>`; `send()` now resolves `auto:claude` and stored Claude accounts.
- Consumes: `crate::secrets::read_claude_creds_json`.

- [ ] **Step 1: Write the failing test** in `anchor.rs` test module:

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn send_claude_posts_oauth_bearer_version_and_one_token() {
    use wiremock::matchers::{body_partial_json, header, header_exists, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("authorization", "Bearer oat-test"))
        .and(header("anthropic-version", "2023-06-01"))
        .and(header_exists("anthropic-beta"))
        .and(body_partial_json(serde_json::json!({"max_tokens": 1})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id":"msg_x"})))
        .mount(&server)
        .await;
    let client = crate::http::build_client();
    let url = format!("{}/v1/messages", server.uri());
    send_claude(&client, "oat-test", &url).await.unwrap();
}
```

- [ ] **Step 2: Run, verify fail.** Run: `cargo test --lib anchor::send_claude` — Expected: FAIL (`send_claude` not defined).

- [ ] **Step 3: Implement.** Add to `anchor.rs` (constants near the top, functions in the body):

```rust
const CLAUDE_MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
const CLAUDE_VERSION: &str = "2023-06-01";
// Claude Code's OAuth tokens require the OAuth beta flag on the Messages API.
// Verify the exact tag with the guarded test-send (Step 8) and adjust if 401/400.
const CLAUDE_OAUTH_BETA: &str = "oauth-2025-04-20";
const CLAUDE_MODEL: &str = "claude-3-5-haiku-20241022";

pub async fn send_claude(
    http: &reqwest::Client,
    token: &str,
    url: &str,
) -> Result<(), ProviderError> {
    let resp = http
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .header("anthropic-version", CLAUDE_VERSION)
        .header("anthropic-beta", CLAUDE_OAUTH_BETA)
        .header("Content-Type", "application/json")
        .json(&anchor_body(CLAUDE_MODEL))
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    let _ = http::send_for_json(resp, "claude anchor").await?;
    Ok(())
}

/// Extract the current Claude access token from the Claude Code credential store.
fn resolve_claude_auto() -> Result<String, ProviderError> {
    let v = crate::secrets::read_claude_creds_json()?;
    for key in ["claudeAiOauth", "claude_ai_oauth", "oauth"] {
        if let Some(obj) = v.get(key).and_then(|o| o.as_object()) {
            if let Some(tok) = obj
                .get("accessToken")
                .or_else(|| obj.get("access_token"))
                .and_then(|s| s.as_str())
            {
                return Ok(tok.to_string());
            }
        }
    }
    Err(ProviderError::NotLoggedIn("claude creds: no accessToken".into()))
}
```

(`crate::secrets::read_claude_creds_json()` returns `Result<Value, SecretsError>`; `?` works because `ProviderError: From<SecretsError>` already exists in `providers/mod.rs`.)

- [ ] **Step 4: Wire Claude into `send()`.** In the stored branch, add `Provider::Claude => send_claude(&http, &cred.access_token, CLAUDE_MESSAGES_URL).await,`. Replace the final `Err(...)` (auto branch) with:

```rust
    match service_id {
        "auto:claude" => send_claude(&http, &resolve_claude_auto()?, CLAUDE_MESSAGES_URL).await,
        other => Err(ProviderError::NotLoggedIn(format!(
            "anchoring not implemented for {other}"
        ))),
    }
```

- [ ] **Step 5: Run checks.** Run: `cargo fmt && cargo clippy --lib --all-targets && cargo test --lib anchor::` — Expected: PASS.

- [ ] **Step 6 (guarded live verification — REQUIRES USER CONFIRMATION):** Sends a real Claude message (consumes 5h-window quota). Confirm with the user. Call `anchor::send("auto:claude")`. If it returns `Status { status: 400/401 }`, the `anthropic-beta` tag is wrong — try the value Claude Code currently sends (capture from the spec's open-risk note / a quick web check of the Claude Code OAuth beta header) and/or confirm `CLAUDE_MODEL` is a currently-served model; adjust the constant until 200.

- [ ] **Step 7: Commit.**

```bash
git checkout -b feat/anchor-claude
git add -A && git commit -m "feat(anchor): Claude /v1/messages send (auto + stored)"
git checkout main && git merge --ff-only feat/anchor-claude && git branch -d feat/anchor-claude
```

---

### Task 3: Codex SPIKE — decide send mechanism

The native `rate_limit_reset_credits` is finite (force-reset, not anchoring) → unsuitable for auto. Sending via the (unofficial) ChatGPT backend is uncertain. Resolve here.

**Files:**
- Modify: `src-tauri/src/anchor.rs`

- [ ] **Step 1: Investigate** (read-only): check `openai/codex` references + `src-tauri/src/providers/codex.rs` for any message/conversation POST shape usable with the existing `auth.json` Bearer + `ChatGPT-Account-Id`. Determine if a minimal completion can be POSTed (e.g. `chatgpt.com/backend-api/...`).

- [ ] **Step 2: Decide & record** the outcome inline in `anchor.rs` doc comments:
  - **(a) Feasible** → implement `send_codex(http, token, account_id, url)` mirroring `send_claude` with the codex headers (`ChatGPT-Account-Id`, `codex_cli_rs/` UA), add `resolve_codex_auto()` (read `tokens.access_token` + `tokens.account_id` from `secrets::codex_auth_path()`), wire `auto:codex` + stored Codex into `send()`, add a wiremock request-shape test like Task 2.
  - **(b) Infeasible** → make Codex **not supported for now**: keep `supported(Provider::Codex) == true` only if (a); otherwise change `supported()` to exclude Codex and update the Task-1 test (`supported_only_claude_codex_zai` → `supported_only_claude_zai`) + this plan's Global Constraints note. Record why.

- [ ] **Step 3: Run checks.** Run: `cargo fmt && cargo clippy --lib --all-targets && cargo test --lib anchor::` — Expected: PASS.

- [ ] **Step 4 (guarded live verification if (a) — REQUIRES USER CONFIRMATION):** `anchor::send("auto:codex")` → expect `Ok`; adjust endpoint/headers until 2xx or fall back to (b).

- [ ] **Step 5: Commit.**

```bash
git checkout -b feat/anchor-codex
git add -A && git commit -m "feat(anchor): codex send mechanism (spike outcome)"
git checkout main && git merge --ff-only feat/anchor-codex && git branch -d feat/anchor-codex
```

---

### Task 4: `auto_anchor` config field + IPC contract

**Files:**
- Modify: `src-tauri/src/config.rs`
- Modify: `src/lib/types.ts`, `src/lib/types.test.ts`, `src/lib/ipc.ts`

**Interfaces:**
- Produces: `AppConfig.auto_anchor: HashMap<String, bool>` (Rust) / `Record<string, boolean>` (TS), default empty.

- [ ] **Step 1: Write the failing Rust test** in `src-tauri/src/config.rs` tests module:

```rust
#[test]
fn auto_anchor_defaults_empty_and_roundtrips() {
    let mut c = AppConfig::default();
    assert!(c.auto_anchor.is_empty());
    c.auto_anchor.insert("stored:zai-1".into(), true);
    let json = serde_json::to_string(&c).unwrap();
    let back: AppConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.auto_anchor.get("stored:zai-1"), Some(&true));
    // Old configs without the field still load.
    let old: AppConfig =
        serde_json::from_str(r#"{"poll_seconds":300,"providers":[]}"#).unwrap_or_default();
    assert!(old.auto_anchor.is_empty());
}
```

- [ ] **Step 2: Run, verify fail.** Run: `cargo test --lib config::tests::auto_anchor` — Expected: FAIL (no field `auto_anchor`).

- [ ] **Step 3: Implement** in `src-tauri/src/config.rs`: add the import `use std::collections::HashMap;` and the field to `AppConfig`:

```rust
    /// Per-`service_id` opt-in for auto window-anchoring (default: empty = OFF).
    #[serde(default)]
    pub auto_anchor: HashMap<String, bool>,
```

Add `auto_anchor: HashMap::new(),` to the `impl Default for AppConfig`. (The `{"providers":[]}` branch in the test uses `unwrap_or_default`, so a too-short providers array is fine for that assertion.)

- [ ] **Step 4: Run, verify pass.** Run: `cargo test --lib config::` — Expected: PASS (incl. existing config tests).

- [ ] **Step 5: Update TS types.** In `src/lib/types.ts` `AppConfig`:

```ts
export interface AppConfig {
  poll_seconds: number;
  providers: [ /* …6… */ ];
  /** Per-service_id opt-in for auto window-anchoring. */
  auto_anchor: Record<string, boolean>;
}
```

In `src/lib/ipc.ts` `createDefaultConfig()` add `auto_anchor: {},` to the returned object.

- [ ] **Step 6: Update the contract test.** In `src/lib/types.test.ts`, change the AppConfig key assertion to include the new key:

```ts
expect(Object.keys(config).sort()).toEqual(["auto_anchor", "poll_seconds", "providers"]);
```

- [ ] **Step 7: Run frontend checks.** Run: `pnpm exec tsc --noEmit && pnpm test` — Expected: PASS.

- [ ] **Step 8: Commit.**

```bash
git checkout -b feat/anchor-config
git add -A && git commit -m "feat(config): per-account auto_anchor opt-in map (Rust + TS contract)"
git checkout main && git merge --ff-only feat/anchor-config && git branch -d feat/anchor-config
```

---

### Task 5: Auto-trigger in `refresh_once` + cooldown

**Files:**
- Modify: `src-tauri/src/commands.rs`

**Interfaces:**
- Consumes: `anchor::{send, supported, try_begin, clear}`, `AppConfig.auto_anchor`, `ServiceUsage.windows`.
- Produces: a `five_hour_used(&ServiceUsage) -> Option<f32>` helper (pure, testable).

- [ ] **Step 1: Write the failing test** in `src-tauri/src/commands.rs` tests module:

```rust
#[test]
fn five_hour_used_reads_the_5h_window_from_either_list() {
    let mk = |label: &str, p: f32| crate::model::LimitWindow {
        label: label.into(), used_percent: Some(p), resets_at: None, used: None, limit: None,
    };
    let mut s = svc("auto:zai", Provider::Zai, ServiceSource::Auto, true, None);
    s.windows = vec![mk("Weekly", 70.0), mk("5-hour", 0.0)];
    assert_eq!(super::five_hour_used(&s), Some(0.0));
    let mut s2 = svc("auto:claude", Provider::Claude, ServiceSource::Auto, true, None);
    s2.detail_windows = vec![mk("5-hour", 12.0)];
    assert_eq!(super::five_hour_used(&s2), Some(12.0));
    let s3 = svc("auto:cursor", Provider::Cursor, ServiceSource::Auto, true, None);
    assert_eq!(super::five_hour_used(&s3), None);
}
```

- [ ] **Step 2: Run, verify fail.** Run: `cargo test --lib commands::tests::five_hour_used` — Expected: FAIL (`five_hour_used` not defined).

- [ ] **Step 3: Implement the helper** in `commands.rs`:

```rust
/// The `used_percent` of the provider's 5-hour window (card or detail list).
fn five_hour_used(s: &crate::model::ServiceUsage) -> Option<f32> {
    s.windows
        .iter()
        .chain(s.detail_windows.iter())
        .find(|w| w.label == "5-hour")
        .and_then(|w| w.used_percent)
}
```

- [ ] **Step 4: Run, verify pass.** Run: `cargo test --lib commands::tests::five_hour_used` — Expected: PASS.

- [ ] **Step 5: Wire the trigger** into `refresh_once`, immediately AFTER `let _ = app.emit("usage-updated", &snapshot);` and BEFORE the final `snapshot` return:

```rust
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
            let app2 = app.clone();
            tauri::async_runtime::spawn(async move {
                match crate::anchor::send(&id).await {
                    Ok(()) => {
                        let _ = app2.emit("anchor-sent", &id);
                    }
                    Err(e) => {
                        eprintln!("anchor send {id}: {e}");
                        crate::anchor::clear(&id); // allow retry next poll
                    }
                }
            });
        }
    }
```

(`refresh_once` already has `cfg: &ConfigStore` and `app: &AppHandle` in scope. `Emitter` is already imported.)

- [ ] **Step 6: Run checks.** Run: `cargo fmt && cargo clippy --lib --all-targets && cargo test --lib` — Expected: PASS (no regressions).

- [ ] **Step 7: Commit.**

```bash
git checkout -b feat/anchor-trigger
git add -A && git commit -m "feat(anchor): auto-fire on empty 5-hour window after each poll (cooldown-guarded)"
git checkout main && git merge --ff-only feat/anchor-trigger && git branch -d feat/anchor-trigger
```

---

### Task 6: Frontend — confirm dialog, settings toggle + manual button, unsupported marking, i18n

**Files:**
- Create: `src/components/ui/ConfirmDialog.tsx`
- Modify: `src/lib/ipc.ts` (`sendAnchorNow` wrapper), `src/lib/providers.ts` (`setAutoAnchor` helper)
- Modify: `src/components/dashboard/detail/InspectorSettings.tsx`
- Modify: `src/locales/en.json`, `src/locales/ko.json`

**Interfaces:**
- Consumes: `invoke("send_anchor_now", { serviceId })`; `AppConfig.auto_anchor`; `InspectorSettings` props `service`, `config`, `onConfigChange`.
- Produces: `ipc.sendAnchorNow(serviceId)`, `providers.setAutoAnchor(config, serviceId, enabled)`, `<ConfirmDialog>`.

- [ ] **Step 1: Add the ipc wrapper** to `src/lib/ipc.ts` (mirroring `removeAccount`):

```ts
export function sendAnchorNow(serviceId: string): Promise<void> {
  if (!hasTauriRuntime()) {
    void serviceId;
    return Promise.resolve();
  }
  return invoke<void>("send_anchor_now", { serviceId });
}
```

- [ ] **Step 2: Add the config helper** to `src/lib/providers.ts`:

```ts
/** Immutably set the per-service auto-anchor opt-in flag. */
export function setAutoAnchor(
  config: AppConfig,
  serviceId: string,
  enabled: boolean,
): AppConfig {
  return { ...config, auto_anchor: { ...config.auto_anchor, [serviceId]: enabled } };
}
```

- [ ] **Step 3: Write the failing test** `src/lib/providers.test.ts` (add to the existing file or create it):

```ts
import { describe, expect, it } from "vitest";
import { setAutoAnchor } from "@/lib/providers";
import { createDefaultConfig } from "@/lib/ipc"; // if not exported, inline a minimal AppConfig literal

describe("setAutoAnchor", () => {
  it("sets and overrides the flag immutably", () => {
    const base = { poll_seconds: 300, providers: [] as never, auto_anchor: {} } as never;
    const a = setAutoAnchor(base, "stored:zai-1", true);
    expect(a.auto_anchor["stored:zai-1"]).toBe(true);
    const b = setAutoAnchor(a, "stored:zai-1", false);
    expect(b.auto_anchor["stored:zai-1"]).toBe(false);
    expect(a.auto_anchor["stored:zai-1"]).toBe(true); // original untouched
  });
});
```

- [ ] **Step 4: Run, verify fail.** Run: `pnpm test providers` — Expected: FAIL (no `setAutoAnchor`). Then re-run after Step 2 lands → PASS.

- [ ] **Step 5: Create `src/components/ui/ConfirmDialog.tsx`** (compose from existing `ui/dialog.tsx`):

```tsx
import { useTranslation } from "react-i18next";
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";

export function ConfirmDialog({
  open, title, body, confirmLabel, onConfirm, onOpenChange,
}: {
  open: boolean;
  title: string;
  body: string;
  confirmLabel: string;
  onConfirm: () => void;
  onOpenChange: (open: boolean) => void;
}) {
  const { t } = useTranslation();
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{body}</DialogDescription>
        </DialogHeader>
        <div className="mt-4 flex justify-end gap-2">
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            {t("common.close")}
          </Button>
          <Button
            onClick={() => {
              onConfirm();
              onOpenChange(false);
            }}
          >
            {confirmLabel}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
```

- [ ] **Step 6: Add the anchoring section to `InspectorSettings.tsx`.** Import at top:

```tsx
import { useState } from "react";
import { sendAnchorNow } from "@/lib/ipc";
import { setAutoAnchor } from "@/lib/providers";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
```

Inside the component (after the existing `patch`/`commitName` helpers), add the supported-set + handlers:

```tsx
  const ANCHOR_SUPPORTED = new Set(["claude", "codex", "zai"]);
  const anchorSupported = ANCHOR_SUPPORTED.has(service.provider);
  const autoOn = config?.auto_anchor?.[service.id] ?? false;
  const [confirmOpen, setConfirmOpen] = useState(false);

  function toggleAuto(next: boolean) {
    if (!config) return;
    onConfigChange(setAutoAnchor(config, service.id, next));
  }
```

Render a new block (place it above the Danger zone):

```tsx
      <section className="space-y-2">
        <h4 className="text-xs font-semibold text-text-faint">
          {t("detail.anchor.title")}
        </h4>
        {anchorSupported ? (
          <>
            <label className="flex items-center justify-between gap-3 text-sm">
              <span>{t("detail.anchor.auto")}</span>
              <input
                type="checkbox"
                checked={autoOn}
                onChange={(e) => toggleAuto(e.target.checked)}
              />
            </label>
            <Button variant="ghost" onClick={() => setConfirmOpen(true)}>
              {t("detail.anchor.sendNow")}
            </Button>
            <ConfirmDialog
              open={confirmOpen}
              title={t("detail.anchor.confirmTitle")}
              body={t("detail.anchor.confirmBody")}
              confirmLabel={t("detail.anchor.sendNow")}
              onConfirm={() => void sendAnchorNow(service.id)}
              onOpenChange={setConfirmOpen}
            />
          </>
        ) : (
          <p className="text-xs text-text-faint">{t("detail.anchor.unsupported")}</p>
        )}
      </section>
```

- [ ] **Step 7: Add i18n keys** to BOTH `src/locales/en.json` and `src/locales/ko.json` under `detail`:

en.json:
```json
    "anchor": {
      "title": "Window anchoring",
      "auto": "Auto-anchor when the 5-hour window is empty",
      "sendNow": "Send a message now",
      "unsupported": "Not supported for this provider (calendar quota / no send path).",
      "confirmTitle": "Send an anchor message?",
      "confirmBody": "This sends a real 1-token message, consumes quota, and may violate the provider's terms. Continue?"
    },
```

ko.json:
```json
    "anchor": {
      "title": "윈도우 앵커링",
      "auto": "5시간 윈도우가 비면 자동으로 앵커링",
      "sendNow": "지금 메시지 보내기",
      "unsupported": "이 제공자는 지원하지 않음 (달력형 쿼터 / 전송 경로 없음).",
      "confirmTitle": "앵커 메시지를 보낼까요?",
      "confirmBody": "실제 1-토큰 메시지를 전송하며 쿼터를 소모하고 제공자 약관에 위배될 수 있습니다. 계속할까요?"
    },
```

- [ ] **Step 8: Run frontend checks.** Run: `pnpm exec tsc --noEmit && pnpm test` — Expected: PASS. (The `locales.test.ts` parity guard requires both catalogs to carry the new keys.)

- [ ] **Step 9: Commit.**

```bash
git checkout -b feat/anchor-frontend
git add -A && git commit -m "feat(anchor): per-account toggle + confirmed manual send + unsupported marking"
git checkout main && git merge --ff-only feat/anchor-frontend && git branch -d feat/anchor-frontend
```

---

### Task 7: Document the feature in README

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Add a "Window anchoring" subsection** under the relevant area (near the Tray/Stored-accounts bullets), e.g.:

```markdown
- **Window anchoring (opt-in, off by default).** For providers with a rolling
  5-hour window (Claude, Codex, z.ai), the app can send a minimal 1-token
  message to anchor a fresh window so its reset time is predictable — via a
  per-account toggle (auto-fire when the 5-hour window is empty) or a confirmed
  manual button in the detail dialog. The send happens entirely in Rust (tokens
  never cross IPC). Copilot/Gemini (calendar quotas) and Cursor (no send path)
  are shown as **not supported**. Sending consumes real quota and may be subject
  to each provider's terms.
```

- [ ] **Step 2: Verify markdown** renders (no broken table). Run: `git diff README.md` and eyeball.

- [ ] **Step 3: Commit.**

```bash
git checkout -b docs/anchor-readme
git add README.md && git commit -m "docs(readme): document window-anchoring feature"
git checkout main && git merge --ff-only docs/anchor-readme && git branch -d docs/anchor-readme
```

---

## Self-Review

**Spec coverage:** scope/marking (Tasks 1-3,6) ✓; trigger used==0 + cooldown (Task 5) ✓; per-account config (Task 4) ✓; manual button + confirm (Task 6) ✓; safety rails / guarded test-sends (Steps 8/6/4) ✓; Codex spike (Task 3) ✓; tokens-in-Rust (anchor.rs, Task 1) ✓; README (Task 7) ✓. Spec A z.ai-both-windows already shipped (`81bdeea`).

**Placeholder scan:** The only deferred specifics are the Claude `anthropic-beta` tag, the z.ai/Claude model names, and the Codex mechanism — all flagged as guarded test-send verifications with explicit fallback steps (authorized by the spec's open-risks). No silent TODOs.

**Type consistency:** `anchor::send/supported/try_begin/clear/anchor_body/send_zai/send_claude`, `send_anchor_now(service_id)` ↔ `invoke("send_anchor_now",{serviceId})`, `AppConfig.auto_anchor` (Rust HashMap ↔ TS Record), `setAutoAnchor`, `five_hour_used`, `"5-hour"` label — consistent across tasks.
