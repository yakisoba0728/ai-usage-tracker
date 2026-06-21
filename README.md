# AI Usage Tracker

A Tauri 2 desktop app that shows **live, real** subscription usage for AI coding
services in your menu bar. Clicking the menu-bar icon drops down a native macOS
menu listing each provider's remaining headroom; the full dashboard opens from
it. It reuses the credential each service's official CLI already stores locally
and calls that provider's usage API directly — **no estimation, no proxy server,
tokens stay on-device**.

> The app supports two ways to connect a provider:
>
> 1. **Auto-detect** the local CLI's stored credential (Claude Code, Codex CLI,
>    Gemini CLI, Copilot CLI, Cursor) and use it as-is.
> 2. **Add account** in-app — paste an API key / session key, or complete an
>    OAuth device-code / browser-callback flow using the CLI's *public*
>    `client_id`.

## Providers

| Provider | Credential source | Usage endpoint | Refresh |
|---|---|---|---|
| **Claude** | Claude Code — macOS Keychain `Claude Code-credentials` / `~/.claude/.credentials.json` (auto); or in-app **session-key** paste (manual) | `api.anthropic.com/api/oauth/usage` + `/api/oauth/profile`; session-key accounts use `claude.ai/api/organizations[/{uuid}/usage]` | OAuth: self-refresh via `platform.claude.com/v1/oauth/token` (fallback `api.anthropic.com/v1/oauth/token`) reusing the public Claude Code client_id; rotated tokens are written back. The usage call is only re-refreshed on `401` — a `429` is a real quota signal and must not burn the rotating refresh token. |
| **Codex** (ChatGPT) | Codex CLI — `~/.codex/auth.json` (`CODEX_HOME`) (auto); or in-app **browser OAuth** (manual) | `chatgpt.com/backend-api/wham/usage` with `ChatGPT-Account-Id` + a `codex_cli_rs/` User-Agent | OAuth: self-refresh via `auth.openai.com/oauth/token` reusing the public Codex CLI client_id; rotated tokens written back to `auth.json`. |
| **Gemini** | Gemini CLI — `~/.gemini/oauth_creds.json` (auto); or in-app **browser OAuth** (manual) | Google Code Assist `loadCodeAssist` / `retrieveUserQuota` | OAuth: self-refresh via `oauth2.googleapis.com/token` reusing the Gemini CLI's public client_id/secret. In-app login uses Authorization Code + loopback redirect (the same flow `gemini` uses); Google's installed-app client_id does NOT support the device-code grant. |
| **GitHub Copilot** | Copilot CLI — macOS Keychain `copilot-cli` / `~/.copilot/config.json` (`COPILOT_HOME`) (auto); or **in-app GitHub device-code OAuth** (`gho_` token) **or** a pasted token (manual) | `api.github.com/copilot_internal/user` with `Editor-Version`, `Editor-Plugin-Version`, `Copilot-Integration-Id` headers | No refresh — GitHub OAuth/PAT tokens are non-expiring. Accepted token types: `gho_` (OAuth), `ghu_` (GitHub App user), `github_pat_` (fine-grained PAT with the **Copilot Requests** account permission). Classic `ghp_` PATs are **not** supported. |
| **Cursor** (experimental) | `state.vscdb` → `ItemTable[cursorAuth/accessToken]` (auto-only — no add flow) | Connect-RPC `api2.cursor.sh/aiserver.v1.DashboardService/GetCurrentPeriodUsage` | No public refresh path. |
| **z.ai** (GLM Coding Plan) | `ZAI_API_KEY` env (auto); or a pasted **API key** (manual) | `api.z.ai/api/monitor/usage/quota/limit` (community-documented; returns 5h + weekly limits) | No refresh — long-lived API key. |

### Design principles

- **Real data only.** A window is omitted rather than guessed. Provider cards
  are honest about state: a missing/expired token or an access-restricted
  endpoint shows the cause + the CLI/key to run, never fabricated numbers.
- **Faithful to the reference projects.** Each provider's URL, headers, and
  response parsing mirror the actual reference implementation (claude-meter,
  gemini-cli-usage, openai/codex `backend-client`, opencode-mystatus,
  ClearMeasureLabs/cursor-usage-status, community `quotas` crate).
- **Per-provider isolation.** One failing provider never breaks the others or
  the scheduler (`panic = "abort"` is deliberately NOT set so a provider panic
  unwinds at the task boundary instead of killing the menu-bar process).
- **Tokens stay in Rust and never cross IPC.** Only non-secret usage snapshots +
  a masked `{id, provider, label}` account list cross IPC to the UI;
  access/refresh tokens are never serialized to the frontend (P0 invariant).
  User-added account credentials are persisted to a local `accounts.json` in the
  app's private config dir (plaintext at rest — the OS-keychain backing was
  dropped because the unsigned build re-prompted for the login password on every
  poll). The webview CSP locks `connect-src` to IPC, so all provider HTTP happens
  in Rust, never the webview.
- **Reuse-only client_ids.** The app uses each CLI's *public* `client_id`
  (shipped inside every installed CLI) for any in-app OAuth / refresh. It does
  not register or ship its own OAuth client.

## Prerequisites

Auto-detection requires the relevant CLI to be signed in (it owns the local
credential). For providers you don't have a CLI for, use **Add account** in the
dashboard:

```bash
# Auto-detected if installed:
# Claude    — `claude` then /login
# Codex     — `codex login`
# Gemini    — `gemini` (Google login)
# Copilot   — `copilot login`   (the Copilot CLI — NOT `gh`)
# Cursor    — sign in to the Cursor app
# z.ai      — export ZAI_API_KEY=...   (GLM Coding Plan or pay-as-you-go key)
```

## Develop

```bash
pnpm install
pnpm tauri dev      # launches the app + tray (Vite HMR)
```

Tests (parsing / normalize / isolation / refresh + frontend formatters /
inspector model — the testable surface):

```bash
cd src-tauri && cargo test --lib    # Rust unit tests
pnpm test                           # frontend unit tests (vitest)
pnpm exec tsc --noEmit              # type-check
```

CI (`.github/workflows/`) runs the frontend type-check + vitest on Linux and
`cargo test --lib` across a **macOS + Windows + Linux** matrix on every push and
PR (with `cargo fmt --check` / `clippy` on Linux); `build-smoke.yml` does a debug
`tauri build` on the same three OSes. (Release bundling / signing / notarization is a separate, not-
yet-configured pipeline.)

## Build

```bash
pnpm tauri build        # -> src-tauri/target/release/bundle/{macos/AI Usage Tracker.app, dmg/AI Usage Tracker_0.1.0_aarch64.dmg}
```

The `tauri` npm script strips the ambient `CI` env var (which otherwise makes
the Tauri CLI reject `--ci`). If `.dmg` creation fails in some sandboxes
(`hdiutil`), distribute the `.app` directly.

## Architecture

```
React + TS + Tailwind + shadcn/ui  ──IPC──▶  Rust (Tauri 2)
                                            ├─ commands    (the #[tauri::command] surface + refresh_once)
                                            ├─ scheduler   (tokio timer loop, generation-guarded; parallel fetch_all)
                                            ├─ providers   (ProviderApi trait + claude/codex/gemini/copilot/cursor/zai)
                                            ├─ login       (device-code OAuth: Codex / Copilot)
                                            ├─ oauth_login (browser + localhost-callback OAuth: Codex / Gemini)
                                            ├─ store       (accounts.json — full credentials for user-added accounts)
                                            ├─ secrets     (read other CLIs' creds: Keychain via /usr/bin/security + JSON + SQLite)
                                            ├─ http        (shared reqwest client + sanitizing JSON helpers)
                                            ├─ jwt         (unverified JWT payload decode: plan / email / exp)
                                            └─ config      (AppConfig persistence)
```

- **Frontend** is provider-agnostic: a single `ServiceUsage` shape renders
  every provider uniformly, and the dashboard groups / filters / sorts multiple
  accounts. Adding a provider is a TypeScript union entry plus one inline SVG
  mark — no component changes.
- **i18n** — English + Korean via react-i18next; the locale is auto-detected,
  persisted to `localStorage` (`ait-lang`), and toggled from the header.
- **Tray** is a native macOS `NSMenu` (built in Rust, rebuilt on each refresh):
  a row per connected provider showing its **remaining headroom** (`100 − used`,
  so a fresh plan reads 100%), then Refresh now / Show dashboard / Quit. The
  dashboard, detail windows, and menu all display remaining (severity/colors stay
  keyed off used). Closing the window hides it to the tray (the app keeps running
  and polling every 5 minutes by default).
- **Stored accounts** hold the user-added credentials in `accounts.json`.
  `fetch_credential` checks `expires_at` before each poll and refreshes in-app
  when the access token is expired, persisting the rotated tokens back (the file
  is rewritten atomically, so a rotated token and its metadata can't desync).
- **Window anchoring (opt-in, off by default).** For providers with a rolling
  5-hour window, the app can start a fresh window so its reset time is
  predictable. **Claude, z.ai, and Codex** all send a minimal message — via a
  per-account auto toggle (fires when the 5-hour window is empty) or a confirmed
  manual button. The action happens entirely in Rust (tokens never cross IPC).
  Copilot/Gemini (calendar quotas) and Cursor (no send path) are shown as
  **not supported**. These actions consume real quota/credits and may be subject
  to each provider's terms.
- **`list_accounts`** masks to `{id, provider, label}` — secrets never cross IPC.

## Config

`get_config` / `set_config` expose `AppConfig { poll_seconds (≥ 30), providers:
[ProviderConfig; 6] }` in the canonical order
`[Claude, Codex, Gemini, Copilot, Cursor, z.ai]`. Each `ProviderConfig` carries:

- `enabled` — whether the provider is fetched,
- `custom_name` — a display-name override,
- `notify_thresholds` — usage-% levels that fire an **in-app toast** when crossed
  (in-app only; there is no OS notification),
- `primary_window` — pin which window is the card headline,
- `sort_index` — grid ordering.

## References (API contracts ported to Rust)

- Claude — `m13v/claude-meter` (MIT, Rust)
- Gemini — `wakamex/gemini-cli-usage` (MIT, Python) + Gemini CLI upstream `oauth2.ts`
- Codex — `openai/codex` `codex-rs/{backend-client,login}` (Apache-2.0)
- Copilot — `vbgate/opencode-mystatus` (MIT) + GitHub Copilot extension headers
- Cursor — `ClearMeasureLabs/cursor-usage-status` (MIT)
- z.ai — community `quotas` crate + `vscode-zai-usage` extension (endpoint is undocumented; `docs.z.ai/api-reference/api-code` for error codes 1308/1310)

## Status / limitations

- v1 ships live current values only (no history/trends).
- Each CLI still owns *initial* token issuance; the app's in-app OAuth flow
  uses the same public `client_id` the CLI ships, so a manually-added account
  is treated as "logged in via that CLI" for usage purposes.
- Claude's primary refresh endpoint is `platform.claude.com/v1/oauth/token`,
  with `api.anthropic.com/v1/oauth/token` as a fallback on network / non-2xx
  errors (a 2xx parse error is not retried, to avoid burning a rotated token).
- macOS is the primary dev/test platform; Linux/Windows are CI-built and
  unit-tested, but the menu-bar/tray placement is macOS-tuned and the non-macOS
  credential paths are exercised in CI only (not on real hardware).
- The z.ai usage endpoint is undocumented and may change without notice.
