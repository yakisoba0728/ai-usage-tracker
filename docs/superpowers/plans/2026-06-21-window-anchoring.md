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
- Supported: **Claude, z.ai** = message-anchor (auto toggle + manual send). **Codex** = manual reset-credit only (official `consume` endpoint; finite credits → no auto). Unsupported (marked, never silently hidden): **Copilot, Gemini, Cursor**. `anchor::supported()` (the auto/message gate) = Claude + z.ai only.
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

### Task 3: Codex manual reset-credit (spike decided: A — manual-only, official endpoint)

**Spike outcome (user-approved):** auto-anchor for Codex is DROPPED — the message path (`/backend-api/codex/responses`) is SSE-only, model-name-sensitive, and unofficial/fragile. Codex instead gets a **manual** "use a reset credit" action via the official `POST https://chatgpt.com/backend-api/wham/rate-limit-reset-credits/consume` (confirmed from openai/codex source + tests). Reset credits are **finite** (`available_count`), so this is manual-only, never auto. `supported()` (the auto/message-anchor gate) therefore drops Codex.

**Files:**
- Modify: `src-tauri/src/anchor.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`

**Interfaces:**
- `anchor::supported(provider)` becomes `matches!(provider, Provider::Claude | Provider::Zai)` (Codex removed).
- Produces: `anchor::reset_codex(http, access_token, account_id, url) -> Result<String, ProviderError>` (returns the response `code`); `anchor::reset_codex_for(service_id) -> Result<String, ProviderError>`; command `reset_codex_now(service_id: String) -> Result<String, String>`.
- Consumes: `crate::secrets::{read_json_file, codex_auth_path}`, `crate::http::{shared, send_for_json, build_client}`, `resolve_stored` (from Task 1).

- [ ] **Step 1: Update the Task-1 `supported` test** in `anchor.rs` — replace `supported_only_claude_codex_zai` with:

```rust
    #[test]
    fn supported_is_claude_and_zai_only() {
        use crate::model::Provider;
        assert!(supported(Provider::Claude));
        assert!(supported(Provider::Zai));
        // Codex is manual reset-credit only (finite credits) — no auto/message anchor.
        assert!(!supported(Provider::Codex));
        assert!(!supported(Provider::Copilot));
        assert!(!supported(Provider::Gemini));
        assert!(!supported(Provider::Cursor));
    }
```

- [ ] **Step 2: Run, verify fail.** Run: `cargo test --lib anchor::tests::supported` — Expected: FAIL (`supported(Codex)` still true).

- [ ] **Step 3: Change `supported()`** body to `matches!(provider, Provider::Claude | Provider::Zai)`.

- [ ] **Step 4: Write the failing reset test** in the `anchor.rs` test module:

```rust
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reset_codex_posts_redeem_id_and_returns_code() {
        use wiremock::matchers::{body_partial_json, header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/backend-api/wham/rate-limit-reset-credits/consume"))
            .and(header("authorization", "Bearer cx-test"))
            .and(header("chatgpt-account-id", "acc-1"))
            .and(body_partial_json(serde_json::json!({})))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"code":"reset","windows_reset":2})),
            )
            .mount(&server)
            .await;
        let client = crate::http::build_client();
        let url = format!(
            "{}/backend-api/wham/rate-limit-reset-credits/consume",
            server.uri()
        );
        let code = reset_codex(&client, "cx-test", Some("acc-1"), &url).await.unwrap();
        assert_eq!(code, "reset");
    }
```

- [ ] **Step 5: Run, verify fail.** Run: `cargo test --lib anchor::tests::reset_codex` — Expected: FAIL (`reset_codex` not defined).

- [ ] **Step 6: Implement** in `anchor.rs` (constants near the top, functions in the body):

```rust
const CODEX_RESET_URL: &str = "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits/consume";
const CODEX_UA: &str = "codex_cli_rs/0.141.0 (ai-usage-tracker)";

/// Consume one Codex rate-limit-reset credit (the official action the Codex CLI
/// uses). Returns the response `code` ("reset" | "nothing_to_reset" | "no_credit"
/// | "already_redeemed"). A fresh idempotency key per call so each deliberate
/// click attempts a reset.
pub async fn reset_codex(
    http: &reqwest::Client,
    access_token: &str,
    account_id: Option<&str>,
    url: &str,
) -> Result<String, ProviderError> {
    let redeem = format!("ait-{}", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));
    let mut req = http
        .post(url)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("User-Agent", CODEX_UA)
        .header("Content-Type", "application/json");
    if let Some(acc) = account_id {
        req = req.header("ChatGPT-Account-Id", acc);
    }
    let resp = req
        .json(&serde_json::json!({ "redeem_request_id": redeem }))
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    let v = http::send_for_json(resp, "codex reset-credit").await?;
    Ok(v.get("code").and_then(|c| c.as_str()).unwrap_or("unknown").to_string())
}

fn resolve_codex_auto() -> Result<(String, Option<String>), ProviderError> {
    let v = crate::secrets::read_json_file(&crate::secrets::codex_auth_path())?;
    let tokens = v
        .get("tokens")
        .ok_or_else(|| ProviderError::NotLoggedIn("codex auth.json: no tokens".into()))?;
    let access = tokens
        .get("access_token")
        .and_then(|s| s.as_str())
        .ok_or_else(|| ProviderError::NotLoggedIn("codex: no access_token".into()))?;
    let account_id = tokens.get("account_id").and_then(|s| s.as_str()).map(String::from);
    Ok((access.to_string(), account_id))
}

/// Manual Codex reset-credit for a UI service id (`auto:codex` or `stored:<id>`).
pub async fn reset_codex_for(service_id: &str) -> Result<String, ProviderError> {
    let http = http::shared();
    let (access, account_id) = if service_id.starts_with("stored:") {
        let cred = resolve_stored(service_id)
            .ok_or_else(|| ProviderError::NotLoggedIn(format!("no stored account {service_id}")))?;
        (cred.access_token, cred.account_id)
    } else {
        resolve_codex_auto()?
    };
    reset_codex(&http, &access, account_id.as_deref(), CODEX_RESET_URL).await
}
```

- [ ] **Step 7: Add the command** to `commands.rs`:

```rust
/// Consume a Codex rate-limit-reset credit for one service. Returns the result
/// code string. Tokens stay in Rust.
#[tauri::command]
pub async fn reset_codex_now(service_id: String) -> Result<String, String> {
    crate::anchor::reset_codex_for(&service_id).await.map_err(|e| e.to_string())
}
```

- [ ] **Step 8: Register** `commands::reset_codex_now` in the `lib.rs` `tauri::generate_handler!` list.

- [ ] **Step 9: Run checks.** Run: `cargo fmt && cargo clippy --lib --all-targets && cargo test --lib` — Expected: PASS (all anchor tests + no regressions).

- [ ] **Step 10 (guarded live verification — REQUIRES USER CONFIRMATION; consumes a FINITE reset credit):** deferred — the controller runs this with the user. `anchor::reset_codex_for("auto:codex")` → expect `Ok("reset")` (or `"no_credit"`/`"nothing_to_reset"`).

- [ ] **Step 11: Commit** (branch only; controller merges after review). Message: `feat(anchor): Codex manual reset-credit (official consume endpoint)`.

```bash
git checkout -b feat/anchor-codex-reset
git add -A && git commit -m "feat(anchor): Codex manual reset-credit (official consume endpoint)"
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

/** Codex-only: consume a rate-limit-reset credit. Returns the result code. */
export function resetCodexNow(serviceId: string): Promise<string> {
  if (!hasTauriRuntime()) {
    void serviceId;
    return Promise.resolve("nothing_to_reset");
  }
  return invoke<string>("reset_codex_now", { serviceId });
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
import { sendAnchorNow, resetCodexNow } from "@/lib/ipc";
import { setAutoAnchor } from "@/lib/providers";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
```

Inside the component (after the existing `patch`/`commitName` helpers), add the per-provider classification + handlers. There are three cases: **message-anchor** providers (Claude, z.ai) get an auto toggle + "send now"; **Codex** gets a manual reset-credit button (no auto — finite credits); everything else is unsupported.

```tsx
  // Message-anchor providers (auto toggle + manual 1-token send).
  const MESSAGE_ANCHOR = new Set(["claude", "zai"]);
  const messageAnchor = MESSAGE_ANCHOR.has(service.provider);
  const codexReset = service.provider === "codex";
  const autoOn = config?.auto_anchor?.[service.id] ?? false;
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [codexConfirmOpen, setCodexConfirmOpen] = useState(false);

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
        {messageAnchor ? (
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
        ) : codexReset ? (
          <>
            <p className="text-xs text-text-faint">{t("detail.anchor.codexNote")}</p>
            <Button variant="ghost" onClick={() => setCodexConfirmOpen(true)}>
              {t("detail.anchor.resetCredit")}
            </Button>
            <ConfirmDialog
              open={codexConfirmOpen}
              title={t("detail.anchor.codexConfirmTitle")}
              body={t("detail.anchor.codexConfirmBody")}
              confirmLabel={t("detail.anchor.resetCredit")}
              onConfirm={() => void resetCodexNow(service.id)}
              onOpenChange={setCodexConfirmOpen}
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
      "confirmBody": "This sends a real 1-token message, consumes quota, and may violate the provider's terms. Continue?",
      "codexNote": "Codex resets the 5-hour window with a finite reset credit (manual only).",
      "resetCredit": "Use a reset credit",
      "codexConfirmTitle": "Use a Codex reset credit?",
      "codexConfirmBody": "This consumes one of your finite Codex reset credits to reset the 5-hour window now. Continue?"
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
      "confirmBody": "실제 1-토큰 메시지를 전송하며 쿼터를 소모하고 제공자 약관에 위배될 수 있습니다. 계속할까요?",
      "codexNote": "Codex는 유한한 리셋 크레딧으로 5시간 윈도우를 리셋합니다 (수동 전용).",
      "resetCredit": "리셋 크레딧 사용",
      "codexConfirmTitle": "Codex 리셋 크레딧을 사용할까요?",
      "codexConfirmBody": "유한한 Codex 리셋 크레딧 1개를 소모해 지금 5시간 윈도우를 리셋합니다. 계속할까요?"
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
  5-hour window, the app can start a fresh window so its reset time is
  predictable. **Claude and z.ai** send a minimal 1-token message — via a
  per-account toggle (auto-fire when the 5-hour window is empty) or a confirmed
  manual button. **Codex** instead uses its official, finite *reset credit*
  (manual button only — no auto). The action happens entirely in Rust (tokens
  never cross IPC). Copilot/Gemini (calendar quotas) and Cursor (no send path)
  are shown as **not supported**. These actions consume real quota/credits and
  may be subject to each provider's terms.
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
