# Chunk 2 — Rust Dedup / Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans (inline, per repo workflow). Behavior-preserving throughout; the existing 90-test suite + the Rust compiler are the regression gate. One branch (`fix/chunk2-rust-refactor`), one commit per task, verify green per task, ff-merge to `main`.

**Goal:** Remove the Rust backend's code duplication and shrink/clarify its largest tangled units, with zero behavior change (one deliberate, benign exception noted in Task C).

**Tech Stack:** Rust 2021, Tauri 2, tokio, reqwest, serde. Verify: `cargo fmt --manifest-path src-tauri/Cargo.toml --check` + `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` + `cargo test --lib --manifest-path src-tauri/Cargo.toml`.

## Global Constraints

- No AI/Co-Authored-By watermark in commits. One branch per chunk. ff-merge to `main`, delete branch when green.
- **Behavior-preserving.** The only allowed behavior change: Task C routes OAuth-refresh errors through the existing `http::send_for_json` sanitizer, so HTML error bodies become the friendly status message instead of raw truncated HTML (consistent with every other HTTP path; refresh errors are either swallowed via `.ok()?` or surfaced as `Expired`). JSON error bodies are unchanged.
- Don't set `panic="abort"`.

## Scope decisions (vs spec Chunk 2)

- **Do:** capitalize→util (A), codex id_token helper (B), OAuth refresh dedup (C), `rotate_credential` builder (D), gemini POST merge + `http::post_json` (E), `fetch_stored` unification (F, with a guard test first), claude.rs split (G), `run_server` RunCtx (H).
- **Skip (YAGNI):** spec item #8 "collapse `refresh_stored` 6-arm match into a trait method." The match (`mod.rs:172-180`) is already clear; adding trait machinery is *more* complexity than the match. Leave as-is.

---

## Task A: `capitalize` → `crate::util`

**Files:** Create `src-tauri/src/util.rs`; modify `src-tauri/src/lib.rs` (add `pub mod util;`), `providers/codex.rs:234-240`, `providers/zai.rs:313-319`, `providers/claude.rs:111-118` (the nested `cap` in `format_plan`).

- [ ] Create `util.rs` with `pub fn capitalize(s: &str) -> String` (same body as the existing impls) + a unit test (`capitalize("pro") == "Pro"`, `capitalize("") == ""`).
- [ ] Add `pub mod util;` to lib.rs.
- [ ] codex.rs: delete local `capitalize`, replace its one call (`codex.rs:322` `u.plan_type.as_deref().map(capitalize)`) with `.map(crate::util::capitalize)`.
- [ ] zai.rs: delete local `capitalize`, replace its call (`plan_from_level_str:327` `Some(capitalize(level))`) with `crate::util::capitalize(level)`.
- [ ] claude.rs: delete the nested `fn cap`, replace the two `sub.as_deref().map(cap)` (lines 131, 141) with `.map(crate::util::capitalize)`.
- [ ] Verify (fmt/clippy/test) + commit `refactor(util): extract shared capitalize helper`.

## Task B: codex id_token identity → `crate::jwt::codex_identity`

**Files:** modify `src-tauri/src/jwt.rs` (add helper + test), `oauth_login.rs:316-331` (the `(Provider::Codex, Some(jwt))` arm), `login.rs:226-244`.

- [ ] Add to `jwt.rs`: `pub fn codex_identity(id_token: &str) -> (Option<String>, Option<String>)` returning `(email, chatgpt_account_id)` — body = the claim extraction currently duplicated (email at `c["email"]`, account at `c["https://api.openai.com/auth"]["chatgpt_account_id"]`). Add a unit test with a fake JWT.
- [ ] oauth_login.rs `build_credential`: in the `(Provider::Codex, Some(jwt))` arm, replace the inline email/acct extraction with `let (email, acct) = crate::jwt::codex_identity(jwt);` (keep the `email.unwrap_or_else(|| "Codex account".into())` label fallback).
- [ ] login.rs `poll_codex` (226-244): replace the inline `.map(|t| {...})` extraction with `crate::jwt::codex_identity(t)`.
- [ ] Verify + commit `refactor(jwt): share codex id_token identity extraction`.

## Task C: OAuth refresh dedup via `http::send_for_json`

**Files:** `providers/codex.rs:401-423` (`refresh_oauth`), `providers/gemini.rs:308-334` (`refresh_gemini_token`), `providers/claude.rs:293-315` (`post_refresh`).

Each currently does: send → `status`/`text` → `if !success { Status{truncate 200} }` → `from_str`. Replace the status/text/truncate/parse tail with `http::send_for_json(resp, ctx)` then `serde_json::from_value::<Refreshed>(v)`.

- [ ] codex `refresh_oauth`: keep the request build (POST form body); `let v = http::send_for_json(resp, "codex refresh").await?; serde_json::from_value::<Refreshed>(v).map_err(|e| ProviderError::Parse(format!("codex refresh: {e}")))`.
- [ ] gemini `refresh_gemini_token`: same pattern, ctx `"gemini refresh"`.
- [ ] claude `post_refresh`: same pattern, ctx `"claude refresh"`. (Keep the platform→fallback retry in `refresh_oauth` unchanged — only `post_refresh`'s tail changes.)
- [ ] Verify + commit `refactor(providers): route oauth refresh through the shared sanitizing json decoder`.

## Task D: `store::rotate_credential` shared builder

**Files:** `src-tauri/src/store.rs` (add fn + test); rewrite `codex::build_refreshed_cred`, `gemini::build_refreshed_cred`, `claude::apply_refresh` as thin wrappers.

Add to store.rs:
```rust
/// Build a rotated credential, preserving id/provider/label/account_id. A `None`
/// `refresh_token`/`id_token` keeps the existing one (providers rotate these
/// inconsistently); callers pass the already-computed `expires_at`.
pub fn rotate_credential(
    cred: &StoredCredential,
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
    expires_at: i64,
) -> StoredCredential {
    StoredCredential {
        id: cred.id.clone(),
        provider: cred.provider,
        label: cred.label.clone(),
        access_token,
        refresh_token: refresh_token.or_else(|| cred.refresh_token.clone()),
        expires_at,
        id_token: id_token.or_else(|| cred.id_token.clone()),
        account_id: cred.account_id.clone(),
    }
}
```
- [ ] Add the fn + a unit test (None keeps old refresh/id_token; Some rotates).
- [ ] codex `build_refreshed_cred`: compute `new_access`/`new_id_token`/`expires_at` as today, then `crate::store::rotate_credential(cred, new_access, fresh.refresh_token.clone(), fresh.id_token.clone(), expires_at)`. (codex wants fresh-or-old id_token: pass `fresh.id_token.clone()`, builder keeps old when None ✓.)
- [ ] gemini `build_refreshed_cred`: `rotate_credential(cred, fresh.access_token.clone(), fresh.refresh_token.clone(), None, expires_at)`.
- [ ] claude `apply_refresh`: `rotate_credential(cred, fresh.access_token, Some(fresh.refresh_token), None, expires_at)`.
- [ ] The existing per-provider builder tests must still pass unchanged. Verify + commit `refactor(store): share rotated-credential construction across providers`.

## Task E: gemini POST merge + `http::post_json`

**Files:** `src-tauri/src/http.rs` (add `post_json` + a wiremock test), `providers/gemini.rs` (`post_internal` method → delegate; `post_code_assist` → use `http::post_json`).

- [ ] Add to http.rs (mirrors `get_json`): `pub async fn post_json<T: DeserializeOwned>(client, token: &str, url: &str, body: &serde_json::Value) -> Result<T, ProviderError>` — POST with `Authorization: Bearer`, `Content-Type: application/json`, `.json(body)`, then `decode_json`. Add a wiremock test (POST 200 + bearer).
- [ ] gemini `post_code_assist`: reimplement as `http::post_json(http, token, &format!("{CODE_ASSIST_BASE}:{method}"), &payload).await`.
- [ ] gemini `post_internal` (method): delegate → `post_code_assist(&self.http, token, method, payload).await` (removes the duplicate body).
- [ ] Verify + commit `refactor(http): add post_json and dedupe gemini code-assist POSTs`.

## Task F: `fetch_stored(&StoredCredential)` unification

**Files:** `providers/{codex,gemini,claude,copilot,zai}.rs` (add `fetch_stored`), `providers/mod.rs:114-141` (dispatch), plus a new guard test in `mod.rs`.

The 4 incompatible `fetch_with` signatures are the dispatch cost. Add one `fetch_stored` per provider that reads what it needs from the cred and calls the existing `fetch_with`:
- codex: `pub(crate) async fn fetch_stored(http, cred) { fetch_with(http, &cred.access_token, cred.account_id.as_deref(), &cred.id_token, Some(&cred.label)).await }`
- gemini: `fetch_with(http, &cred.access_token, Some(&cred.label))`
- claude: `fetch_with_session_key(http, &cred.access_token)`
- copilot: `fetch_with(http, &cred.access_token)`
- zai: `fetch_with(http, &cred.access_token, Some(&cred.label))`

- [ ] **First**, add a guard test to `mod.rs` for the dispatch invariant the agent flagged as untested: assert `fetch_credential` for `Provider::Cursor` returns a disconnected `ServiceUsage` with error code `not_logged_in` and `source == Stored` (no network; Cursor is the one explicit `Err` arm). This pins the dispatch behavior before the refactor.
- [ ] Add the 5 `fetch_stored` fns (keep existing `fetch_with` as-is; `fetch_stored` is a thin adapter).
- [ ] Rewrite `mod.rs::fetch_credential` dispatch (115-141) to: `match active.provider { Cursor => Err(NotLoggedIn(...)), Codex => codex::fetch_stored(&http,&active).await, ... }` — each arm now one uniform call.
- [ ] Verify (compiler proves the adapters; guard test + fetch_all isolation test stay green) + commit `refactor(providers): unify stored-account fetch behind fetch_stored`.

## Task G: split `claude.rs` (965 LOC) into a module

**Files:** `providers/claude.rs` → `providers/claude/mod.rs` + `providers/claude/web.rs` + `providers/claude/keychain_write.rs`.

- [ ] `git mv src-tauri/src/providers/claude.rs src-tauri/src/providers/claude/mod.rs`.
- [ ] Move the claude.ai web/session-key client (`WebOrg`/`WebMembership`/`WebMemberAccount`/`WebWindow`/`WebUsage`, `fetch_with_session_key`, `session_key_cookie_value`, the `web_orgs_parse_tier_and_email` + `normalizes_pasted_session_key_cookie_values` tests) → `web.rs`; expose `pub(crate) use web::fetch_with_session_key;` (or `pub(super)` + re-export). `web.rs` needs `use super::format_plan;` and the shared imports.
- [ ] Move the macOS keychain write (`write_creds`, `existing_keychain_account`) → `keychain_write.rs` as `pub(super)`; `mod.rs::write_back` calls `keychain_write::write_creds`.
- [ ] Add `mod web; mod keychain_write;` to `claude/mod.rs`. Keep the public surface identical (`fetch_with`, `fetch_with_session_key`, `refresh_stored`, `ClaudeProvider`).
- [ ] Verify (compiler + all claude tests, now split across files, stay green) + commit `refactor(claude): split the web + keychain-write surfaces into submodules`.

## Task H: `run_server` 10 args → `RunCtx`

**Files:** `src-tauri/src/oauth_login.rs` (`start`, `run_server`).

- [ ] Define `struct RunCtx { app, provider, client_id, client_secret: Option<String>, token_url, redirect_uri, verifier, expected_state, cancelled }` (server stays a separate param — it isn't `Clone`).
- [ ] `run_server(server: Server, ctx: RunCtx)`; replace the 10 positional fields with `ctx.field`. Drop `#[allow(clippy::too_many_arguments)]`.
- [ ] `start`: build the `RunCtx` and `std::thread::spawn(move || run_server(server, ctx))`.
- [ ] Verify + commit `refactor(oauth): bundle run_server params into RunCtx`.

---

## Finalize

- [ ] Final fmt/clippy/test (all green; test count = 90 + new helper tests A/B/D/E/F ≈ 95+).
- [ ] `git checkout main && git merge --ff-only fix/chunk2-rust-refactor && git branch -d fix/chunk2-rust-refactor`.

## Self-review

- Spec coverage: A=#3, B=#5, C=#1, D=#2, E=#4+#6, F=#7, G=#9, H=#10; #8 explicitly skipped (YAGNI). ✓
- Behavior change: only Task C's error-sanitization, documented + benign. ✓
- Type consistency: `capitalize(&str)->String`, `codex_identity(&str)->(Option<String>,Option<String>)`, `rotate_credential(&cred,String,Option<String>,Option<String>,i64)->StoredCredential`, `post_json<T>(client,&str,&str,&Value)`, `fetch_stored(http,&StoredCredential)->Result<ServiceUsage,ProviderError>` used consistently. ✓
