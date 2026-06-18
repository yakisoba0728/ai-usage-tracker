# AI Usage Tracker — Design Spec

- **Date:** 2026-06-18
- **Status:** Draft (pending user review)
- **Owner:** TBD

## 1. Overview

A cross-platform desktop application built on **Tauri 2** that shows **live, real
(not estimated)** subscription usage for three AI services — **Claude, Codex
(ChatGPT), and Gemini** — in a system-tray-resident app with a click-to-open
dashboard.

Usage data is obtained by reusing the **OAuth tokens already stored locally by
each service's official CLI** (Claude Code, Codex CLI, Gemini CLI), refreshing
them with their published client metadata, and calling each provider's
internal/unofficial usage endpoints directly. The app **never registers its own
OAuth client** and **never estimates usage from local token logs**.

## 2. Goals & Non-Goals

### Goals
- Display real server-side usage for Claude, Codex, and Gemini subscriptions.
- Reuse existing CLI OAuth tokens (read stored token files / OS keychain);
  refresh tokens with the official CLI client metadata (no self-issued OAuth app).
- Tray-resident + dashboard hybrid UX; always-current tray indicator.
- Background auto-polling (default 5 min, user-configurable).
- Public distribution on macOS, Linux, and Windows.
- Per-provider isolation: one failing provider never breaks the others.

### Non-Goals
- Historical persistence / trend charts (current values only).
- API-key-based usage (subscription usage only).
- Registering/shipping our own OAuth client credentials.
- Estimating usage from `~/.claude` JSONL logs or similar.

## 3. Decisions Log (from brainstorming)

| Decision | Choice | Rationale |
|---|---|---|
| Token acquisition | Reuse official CLI tokens (read stored files / keychain) | User requirement; no own OAuth app. |
| Architecture | **Rust-native backend** (Approach A) | macOS Keychain access + CORS avoidance + token non-exposure + background polling all require native Rust. |
| App form | Tray + dashboard hybrid | Always-visible tray + rich dashboard. |
| Distribution | Public | Reuse-only tokens keeps this safe; tokens never leave backend. |
| Data scope | Live current values only | No persistence layer / DB. |
| Refresh | Background auto-polling (5 min, configurable) | Tray must stay current without user action. |
| Frontend | React 19 + TS + Tailwind + shadcn/ui | Mainstream, strong dashboard/chart ecosystem, fast for Tauri. |
| Reuse strategy | Port reference repos' **API contracts** to Rust; do **not** vendor Python/TS code | Bundling Python is impractical in Tauri; TS-in-frontend fails on CORS + keychain + security. |

## 4. Reference Repositories (authoritative API spec)

These are used as the source of truth for **endpoints, auth headers, refresh
flows, and response field shapes**. Their code is ported to Rust, not bundled.

- **Claude** — `m13v/claude-meter`, `eddmann/ClaudeMeter`: server-side usage
  endpoints; fields `five_hour`, `seven_day`, `seven_day_sonnet/opus/oauth_apps`,
  `extra_usage.used_credits`.
- **Codex** — OpenAI Codex CLI source (`codex-rs/login`) + community usage tools:
  `GET https://chatgpt.com/backend-api/codex/usage` with `~/.codex/auth.json`
  bearer token.
- **Gemini** — `wakamex/gemini-cli-usage`: mirrors Gemini CLI's Google Code
  Assist flow (`loadCodeAssist`, `retrieveUserQuota`) using
  `~/.gemini/oauth_creds.json`.

## 5. Architecture

### 5.1 Process model
- A single Tauri 2 process stays **always alive**. Closing the dashboard window
  hides it to the tray (close-to-tray); it does not quit.
- A background scheduler (`tokio` interval) runs regardless of window visibility,
  so the tray indicator stays current.

### 5.2 Layering
```
React (webview)  ──IPC──▶  Rust core
                          ├─ scheduler   (tokio interval, parallel provider fetch)
                          ├─ providers   (Provider trait + Claude/Codex/Gemini)
                          ├─ secrets     (fs + keychain credential read)
                          ├─ refresh     (OAuth refresh w/ official client metadata)
                          └─ http        (reqwest + rustls, per-provider headers)
```
- **Secrets boundary:** access/refresh tokens are read and used **inside Rust
  only**. They are never serialized to the frontend. Only non-secret usage
  snapshots cross IPC.

### 5.3 IPC
- Commands (frontend → backend): `get_usage`, `refresh_now`, `get_config`,
  `set_config`.
- Events (backend → frontend): `usage-updated` (full snapshot), `provider-error`
  (per-provider, with actionable hint).

## 6. Providers (detailed contracts)

> Reverse-engineered endpoints shift over time. Items marked **[confirm]** must be
> verified against the reference repo / official CLI source during implementation;
> the overall flow and auth model are fixed by this spec.

### 6.1 Claude (via Claude Code)

- **Token source**
  - macOS: **Keychain** (read via `keyring` crate). Service/account name
    **[confirm]** against Claude Code source.
  - Linux/Windows: `~/.claude/.credentials.json` (mode `0600`), or
    `$CLAUDE_CONFIG_DIR/.credentials.json`.
- **Credential shape**
  ```json
  {
    "claudeAiOauth": {
      "accessToken": "sk-ant-oat01-…",
      "refreshToken": "sk-ant-ort01-…",
      "expiresAt": 1775906657084,
      "scopes": ["user:inference", …],
      "subscriptionType": "max",
      "rateLimitTier": "default_claude_max_5x"
    }
  }
  ```
  (Older flat shape `{accessToken, refreshToken, expiresAt, …}` must also be
  tolerated.)
- **Refresh:** `POST` Anthropic OAuth token endpoint with
  `grant_type=refresh_token` + Claude Code's public `client_id` **[confirm
  endpoint + client_id]**.
- **Usage endpoint:** `GET https://api.anthropic.com/api/oauth/usage`
  (preferred; fallback `claude.ai/api/organizations/{org}/usage`).
  Header `Authorization: Bearer <accessToken>`.
- **Usage fields → windows:**
  - `five_hour` → "5시간" window
  - `seven_day` → "7일" window
  - `seven_day_sonnet` / `seven_day_opus` / `seven_day_oauth_apps` → sub-windows
  - `extra_usage.used_credits` → extra-usage indicator
  Each with `used_percentage` and `resets_at`.
- **Plan display:** `subscriptionType` (pro / max / …).

### 6.2 Codex (via Codex CLI)

- **Token source:** `~/.codex/auth.json`, or OS keyring when
  `cli_auth_credentials_store` is set.
- **Credential shape**
  ```json
  {
    "auth_mode": "chatgpt",
    "tokens": {
      "id_token": "eyJ…",        // JWT: email, plan, chatgpt_user_id, chatgpt_account_id
      "access_token": "eyJ…",
      "refresh_token": "…",
      "account_id": "…"
    },
    "last_refresh": "2026-06-18T12:34:56.789Z"
  }
  ```
- **Plan/identity:** decode `id_token` JWT payload (no verification of signature;
  metadata only) for `email`, ChatGPT plan, `chatgpt_account_id`.
- **Refresh:** `POST` OpenAI OAuth token endpoint with `refresh_token` + Codex's
  public `client_id` **[confirm endpoint + client_id]**.
- **Usage endpoint:** `GET https://chatgpt.com/backend-api/codex/usage`
  with `Authorization: Bearer <access_token>`. May require Codex-CLI-style
  `User-Agent` / device headers **[confirm]**.
- **Usage fields → windows:** remaining/used credits + reset window
  **[confirm exact shape]**; normalize to `LimitWindow`.

### 6.3 Gemini (via Gemini CLI)

- **Token source:** `~/.gemini/oauth_creds.json` (access/refresh tokens, expiry).
- **Refresh:** Google OAuth token endpoint with `refresh_token` + Gemini CLI's
  installed client metadata. Fallback env vars `GEMINI_OAUTH_CLIENT_ID` /
  `GEMINI_OAUTH_CLIENT_SECRET`.
- **Usage flow:** mirror Gemini CLI's internal Code Assist API:
  1. `loadCodeAssist` → resolve internal user/project ID.
  2. `retrieveUserQuota` → per-model quota: `used_percent`, reset time, optional
     `remainingAmount`.
  - Base endpoint **[confirm]** (e.g. `cloudcode-pa.googleapis.com` internal).
- **Usage fields → windows:** one `LimitWindow` per model
  (e.g. `gemini-2.5-pro`, `gemini-2.5-flash-lite`) with used % and reset.

## 7. Unified Data Model

```rust
pub enum Provider { Claude, Codex, Gemini }

pub struct ServiceUsage {
    pub provider: Provider,
    pub connected: bool,
    pub plan: Option<String>,
    pub error: Option<String>,
    pub windows: Vec<LimitWindow>,
}

pub struct LimitWindow {
    pub label: String,             // "5시간" | "7일" | "gemini-2.5-pro" | "Codex credits"
    pub used_percent: Option<f32>, // only when server provides it
    pub resets_at: Option<i64>,    // epoch seconds
    pub used: Option<f64>,         // absolute value when available
    pub limit: Option<f64>,
}

pub struct UsageSnapshot {
    pub fetched_at: i64,           // epoch seconds
    pub services: Vec<ServiceUsage>,
}
```
- Each provider normalizes its raw response into `ServiceUsage` so the UI renders
  all three uniformly while preserving provider-specific detail.

## 8. Frontend

- **Tray:** icon + dynamic label = highest usage % across all windows. Left-click
  toggles dashboard; right-click menu: Open / Refresh now / Quit.
- **Dashboard window:** three sections (Claude / Codex / Gemini).
  - Per section: connection Badge (with plan), one `Progress` gauge per
    `LimitWindow`, reset countdown, per-provider error hint (e.g.
    *"Run `claude` then /login"*).
  - Header: last-updated time, manual Refresh button, polling-interval display.
- **State:** hydrate from `get_usage` on mount; subscribe to `usage-updated`.
- **Stack:** React 19 + Vite + TS + Tailwind + shadcn/ui (Card, Progress, Badge,
  Tooltip, Switch).

## 9. Error Handling & Security

- **Isolation:** each provider runs in its own fallible path; errors are captured
  per-provider and do not abort the scheduler or other providers.
- **Refresh failure:** mark provider `connected=false`, surface a hint, keep last
  known snapshot, apply backoff on repeated auth/rate-limit (401/429) responses.
- **Secret handling:** tokens read on demand in Rust; never logged, never sent to
  the frontend, never transmitted except to the provider's own endpoints.
- **No telemetry:** the only outbound network calls are to the three providers'
  own usage endpoints (and their OAuth token endpoints for refresh).
- **Distribution:** code-signed builds per platform; no secret bundling.

## 10. Testing

- **Rust unit tests (the testable surface):**
  - Per-provider response parsing from fixture JSON (real shapes captured from
    reference repos / live responses) → `ServiceUsage` normalization.
  - Credential file parsing (modern nested + legacy flat Claude shape; Codex
    shape) and keychain mock.
  - OAuth refresh request construction (correct endpoint, client_id,
    grant_type) without network.
  - Scheduler backoff / error-isolation behavior with stub providers.
- **Manual / integration (implementation phase):** validate live fetch with real
  authenticated accounts for each provider on macOS (keychain path) and at least
  one file-based path.

## 11. Tech Stack

- **Shell:** Tauri 2 (stable).
- **Rust:** `reqwest` (rustls), `keyring`, `serde`/`serde_json`, `tokio`,
  `thiserror`, `tracing`. Optional plugins: `tauri-plugin-autostart` (launch at
  login), `tauri-plugin-store` (settings).
- **Frontend:** React 19, Vite, TypeScript, Tailwind CSS, shadcn/ui.

## 12. Proposed File Layout

```
src-tauri/
  src/
    main.rs                 # Tauri app bootstrap, tray, commands registration
    commands.rs             # IPC commands (get_usage, refresh_now, get/set_config)
    scheduler.rs            # tokio interval, parallel fetch, event emission
    providers/
      mod.rs                # Provider trait, ServiceUsage/UsageSnapshot types
      claude.rs
      codex.rs
      gemini.rs
    secrets.rs              # fs + keychain credential reading
    refresh.rs              # OAuth refresh per provider (official client metadata)
    http.rs                 # shared reqwest client + per-provider header policy
    config.rs               # settings (poll interval, autostart, enabled providers)
src/                        # React frontend
  App.tsx
  components/               # Dashboard, ServiceCard, Gauge, Header, Tray hook
  hooks/                    # useUsage (command + event listener)
  lib/                      # ipc wrappers, types (mirroring Rust model)
docs/superpowers/specs/     # this spec
```

## 13. Open Questions (to resolve during implementation)

1. Exact macOS Keychain service/account name for Claude Code credentials.
2. Exact OAuth token-refresh endpoints + public `client_id` values for Claude
   and Codex (extract from official CLI source).
3. Exact Codex usage response shape and any required device headers.
4. Exact Gemini Code Assist internal base endpoint + request payloads.
5. Whether to expose poll-interval + autostart in a Settings UI in v1 or ship
   defaults first.

## 14. Out of Scope / Future

- Historical trends (would add SQLite + snapshotting).
- Additional providers (Cursor, Copilot, etc.).
- Notifications on approaching limits.
- In-app onboarding that triggers each CLI's login.
