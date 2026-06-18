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
each service's official CLI** (Claude Code, Codex CLI, Gemini CLI) and calling
each provider's internal/unofficial usage endpoints directly. The app **never
registers its own OAuth client** and **never estimates usage from local token
logs**. Token refresh (where implemented) reuses each CLI's *already-stored,
publicly visible* client metadata — it does not invent new credentials.

## 2. Goals & Non-Goals

### Goals
- Display real server-side usage for Claude, Codex, and Gemini subscriptions.
- Reuse existing CLI OAuth tokens (read stored token files / OS keychain).
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
| Token refresh | **Per-provider** (not cross-cutting) | Claude/Codex = read-only + hint on expiry (matches proven `claude-meter`); Gemini = self-refresh (matches `gemini-cli-usage`). |

## 4. Reference Implementations (authoritative API spec, ported to Rust)

| Provider | Reference | License | Used as source of truth for |
|---|---|---|---|
| Claude | `m13v/claude-meter` (`src/oauth.rs`, `src/models.rs`) — **Rust** | MIT | Keychain read, `/api/oauth/usage` + `/api/oauth/profile`, `UsageResponse` shape, no-refresh model. **Closely modeled.** |
| Gemini | `wakamex/gemini-cli-usage` (`src/gemini_cli_usage/__init__.py`) — Python | MIT | OAuth refresh, Code Assist `loadCodeAssist`/`retrieveUserQuota`, bucket parsing. |
| Codex | `openai/codex` (`codex-rs/login`) — Rust | Apache-2.0 | `~/.codex/auth.json` token shape, JWT claims, `CLIENT_ID`/refresh-URL locations (for future self-refresh). |

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
                          │     └─ each provider owns its token-read + refresh + HTTP
                          ├─ secrets     (cross-platform credential file/keychain read)
                          └─ http        (reqwest + rustls, shared client, per-provider headers)
```
- **Secrets boundary:** access/refresh tokens are read and used **inside Rust
  only**. They are never serialized to the frontend. Only non-secret usage
  snapshots cross IPC.
- **Refresh is per-provider:** Claude/Codex read the current token and surface a
  hint on expiry; Gemini refreshes its token itself (see §6.3).

### 5.3 IPC
- Commands (frontend → backend): `get_usage`, `refresh_now`, `get_config`,
  `set_config`.
- Events (backend → frontend): `usage-updated` (full snapshot).

## 6. Providers (source-verified contracts)

### 6.1 Claude (via Claude Code) — modeled on `claude-meter` (MIT)

- **Token source**
  - macOS: **Keychain**, service `Claude Code-credentials`, read via the `keyring`
    crate (claude-meter shells out to `/usr/bin/security`; we use `keyring` for
    cross-platform cleanliness). First read may prompt macOS for keychain access;
    user clicks *Always Allow*.
  - Linux/Windows: `~/.claude/.credentials.json` (mode `0600`), or
    `$CLAUDE_CONFIG_DIR/.credentials.json`.
- **Credential shape** (the keychain value / file content is this JSON):
  ```json
  {"claudeAiOauth": {
    "accessToken": "sk-ant-oat01-…",
    "refreshToken": "sk-ant-ort01-…",
    "expiresAt": 1778299177154,
    "scopes": ["user:profile", "user:inference", …],
    "subscriptionType": "max",
    "rateLimitTier": "default_claude_max_20x"
  }}
  ```
- **Endpoints** (base `https://api.anthropic.com`), headers `Authorization: Bearer
  <accessToken>`, `Accept: application/json`, `anthropic-version: 2023-06-01`:
  - `GET /api/oauth/usage` → rolling-window quotas + `extra_usage`.
  - `GET /api/oauth/profile` → `{account:{email}, organization:{uuid}}`.
- **Response model** (`UsageResponse`):
  ```
  five_hour, seven_day, seven_day_sonnet, seven_day_opus,
  seven_day_oauth_apps, seven_day_omelette, seven_day_cowork
      : Option<Window{ utilization: f64, resets_at: Option<DateTime<Utc>> }>
  extra_usage : Option<{ is_enabled, monthly_limit, used_credits, utilization, currency }>
  ```
- **Refresh:** **none.** The running Claude Code CLI rotates the token; we read
  whatever is stored. If `expiresAt < now`, bail with a hint to run `claude` once.
- **Plan display:** `subscriptionType` (pro / max / …) + `account.email`.

### 6.2 Codex (via Codex CLI)

- **Token source:** `~/.codex/auth.json` (or OS keyring when
  `cli_auth_credentials_store` is set).
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
- **Identity/plan:** decode `id_token` JWT payload (metadata only; no signature
  verification) → `email`, ChatGPT plan, `chatgpt_account_id`.
- **Usage endpoint:** `GET https://chatgpt.com/backend-api/codex/usage`,
  `Authorization: Bearer <access_token>`. Exact response shape **[confirm against
  a live response at implementation]**; parse defensively (capture raw JSON,
  extract remaining/used credits + reset window where present).
- **Refresh:** **none in v1** (read-only + hint "run `codex`"). Future enhancement:
  self-refresh via the `CLIENT_ID` constant in `codex-rs/login/src/auth/` and
  `https://auth.openai.com/oauth/token` (`grant_type=refresh_token`).

### 6.3 Gemini (via Gemini CLI) — ported from `gemini-cli-usage` (MIT)

- **Token source:** `~/.gemini/oauth_creds.json`
  (`{access_token, refresh_token, expiry_date(ms), client_id?, client_secret?, …}`).
- **Self-refresh** (token is short-lived):
  1. Resolve client metadata, in priority:
     env `GEMINI_OAUTH_CLIENT_ID`/`GEMINI_OAUTH_CLIENT_SECRET` → the creds file's
     `client_id`/`client_secret` → extracted from the installed Gemini CLI's
     `code_assist/oauth2.js` (`const OAUTH_CLIENT_ID = '…';`).
  2. `POST https://oauth2.googleapis.com/token` form-encoded
     `grant_type=refresh_token&refresh_token=…&client_id=…&client_secret=…`.
  3. Update `access_token`/`expiry_date` in memory (best-effort write-back to the
     file). Refresh proactively when `now >= expiry_date - 60s`.
- **Quota flow** (base `https://cloudcode-pa.googleapis.com/v1internal`, headers
  `Authorization: Bearer <access_token>`, `Content-Type: application/json`):
  1. `POST …/v1internal:loadCodeAssist`
     `{cloudaicompanionProject: <project_id?>, metadata:{ideType:"IDE_UNSPECIFIED",platform:"PLATFORM_UNSPECIFIED",pluginType:"GEMINI"}}`
     → `{cloudaicompanionProject, currentTier, paidTier}`.
  2. `POST …/v1internal:retrieveUserQuota` `{project: <project_id>}`
     → `{buckets:[{modelId, remainingFraction, remainingAmount?, resetTime, tokenType}]}`.
  3. Per bucket → `used_pct = (1 - remainingFraction) * 100`; `limit =
     remainingAmount / remainingFraction` when both present.
- **Auth gating:** only `oauth-personal` (Google login) auth supports quota;
  other Gemini auth types → "unsupported" state.

## 7. Unified Data Model

```rust
pub enum Provider { Claude, Codex, Gemini }

pub struct ServiceUsage {
    pub provider: Provider,
    pub connected: bool,
    pub plan: Option<String>,
    pub account: Option<String>,    // email / account id, for display only
    pub error: Option<String>,
    pub windows: Vec<LimitWindow>,
}

pub struct LimitWindow {
    pub label: String,             // "5시간" | "7일" | "gemini-2.5-pro" | "Codex credits"
    pub used_percent: Option<f32>, // only when the server provides it
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
- **Auth/expiry:** Claude/Codex return a structured "token expired / not logged
  in" error with an actionable hint (run the CLI). Gemini refreshes; if refresh
  fails, same hint treatment. Repeated 401/429 → backoff in the scheduler, last
  snapshot retained.
- **Secret handling:** tokens read on demand in Rust; never logged, never sent to
  the frontend, never transmitted except to the provider's own endpoints.
- **No telemetry:** the only outbound network calls are to the three providers'
  own usage/token endpoints.
- **Distribution:** code-signed builds per platform; no secret bundling.

## 10. Testing

- **Rust unit tests (the testable surface):**
  - Per-provider response parsing from fixture JSON (captured from reference
    repos / live responses) → `ServiceUsage` normalization.
  - Claude keychain-blob + file parsing (modern nested shape).
  - Codex `auth.json` parsing + JWT-claims decode (expiry, plan, email).
  - Gemini OAuth refresh request construction + `loadCodeAssist`/`retrieveUserQuota`
    response parsing + bucket math, **without** network.
  - Scheduler backoff / error-isolation behavior with stub providers.
- **Manual / integration (implementation phase):** validate live fetch with real
  authenticated accounts per provider on macOS (keychain path) and at least one
  file-based path.

## 11. Tech Stack

- **Shell:** Tauri 2 (stable).
- **Rust:** `reqwest` (rustls), `keyring`, `serde`/`serde_json`, `tokio`,
  `thiserror`, `tracing`, `jsonwebtoken` (JWT decode, no-verify), `chrono`.
  Optional plugins: `tauri-plugin-autostart` (launch at login),
  `tauri-plugin-store` (settings).
- **Frontend:** React 19, Vite, TypeScript, Tailwind CSS, shadcn/ui.

## 12. Proposed File Layout

```
src-tauri/
  src/
    main.rs                 # Tauri app bootstrap, tray, commands registration
    commands.rs             # IPC commands (get_usage, refresh_now, get/set_config)
    scheduler.rs            # tokio interval, parallel fetch, event emission
    model.rs                # Provider/ServiceUsage/UsageSnapshot/LimitWindow
    http.rs                 # shared reqwest client + per-provider header policy
    secrets.rs              # cross-platform keychain/file credential read
    providers/
      mod.rs                # Provider trait
      claude.rs             # keychain/file read + /api/oauth/usage (no refresh)
      codex.rs              # auth.json read + id_token decode + codex/usage (no refresh)
      gemini.rs             # oauth_creds read + self-refresh + Code Assist quota
    config.rs               # settings (poll interval, autostart, enabled providers)
src/                        # React frontend
  App.tsx
  components/               # Dashboard, ServiceCard, Gauge, Header
  hooks/                    # useUsage (command + event listener)
  lib/                      # ipc wrappers, types (mirroring Rust model)
docs/superpowers/specs/     # this spec
docs/superpowers/plans/     # implementation plan
```

## 13. Open Questions (to resolve during implementation)

1. Exact Codex `/backend-api/codex/usage` response shape (parse defensively; pin
   fields from a live response).
2. macOS keychain permission UX on first read (expected *Always Allow* prompt);
   confirm `keyring` crate reads the `Claude Code-credentials` service without
   issue on macOS.
3. Whether to expose poll-interval + autostart in a Settings UI in v1 or ship
   defaults first.

## 14. Future Enhancements (explicitly out of v1)

- Codex self-refresh (via codex `CLIENT_ID` + `auth.openai.com/oauth/token`).
- Historical trends (would add SQLite + snapshotting).
- Additional providers (Cursor, Copilot, etc.).
- Notifications on approaching limits.
- In-app onboarding that triggers each CLI's login.
