# AI Usage Tracker

A Tauri 2 desktop app that shows **live, real** subscription usage for AI coding
services in your menu bar / a click-to-open dashboard. It reuses the credential
each service's official CLI already stores locally and calls that provider's
usage API directly — **no estimation, no proxy server, tokens stay on-device**.

> The app supports two ways to connect a provider:
>
> 1. **Auto-detect** the local CLI's stored credential (Claude Code, Codex CLI,
>    Gemini CLI, Copilot CLI, Cursor) and use it as-is.
> 2. **Add account** in-app — paste an API key / session key, or complete an
>    OAuth device-code / browser-callback flow using the CLI's *public*
>    `client_id`. Stored accounts live in `accounts.json` and are refreshed
>    in-app when they expire.

## Supported providers

| Provider | Source of credential | Usage API | Refresh |
|---|---|---|---|
| **Claude** | Claude Code — macOS Keychain (`Claude Code-credentials`) / `~/.claude/.credentials.json` (auto); or a pasted `claude.ai` **session key** (manual) | `api.anthropic.com/api/oauth/usage` (+ `/profile`) for OAuth; `claude.ai/api/organizations/{uuid}/usage` for session-key | OAuth: self-refresh via `console.anthropic.com/v1/oauth/token` (rotates the refresh_token; writes back so the CLI stays in sync). Session key: not refreshable. Modeled on `claude-meter`. |
| **Codex** | Codex CLI — `~/.codex/auth.json` (`$CODEX_HOME` honored) (auto); or in-app **browser OAuth** (manual) | `chatgpt.com/backend-api/wham/usage` with `codex_cli_rs/` UA + `ChatGPT-Account-Id` | OAuth: self-refresh via `auth.openai.com/oauth/token` (`grant_type=refresh_token`, client_id `app_EMoamEEZ73f0CkXaXp7hrann`). |
| **Gemini** | Gemini CLI — `~/.gemini/oauth_creds.json` (auto); or in-app **device-code OAuth** (manual) | Google Code Assist `loadCodeAssist` / `retrieveUserQuota` | OAuth: self-refresh via `oauth2.googleapis.com/token` reusing the Gemini CLI's public client_id/secret. |
| **GitHub Copilot** | Copilot CLI — macOS Keychain `copilot-cli` / `~/.copilot/config.json` (auto); or a pasted **PAT / OAuth token** (manual) | `api.github.com/copilot_internal/user` with `Editor-Version`, `Editor-Plugin-Version`, `Copilot-Integration-Id` headers | No refresh — GitHub OAuth/PAT tokens are non-expiring. |
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
  the scheduler.
- **Tokens stay in Rust.** Only non-secret usage snapshots + a masked
  `{id, provider, label}` account list cross IPC to the UI. Access/refresh
  tokens are never serialized to the frontend (P0 invariant).
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

Rust unit tests (parsing/normalize/isolation/refresh — the testable surface):

```bash
cd src-tauri && cargo test --lib          # 50 tests
```

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
                                            ├─ scheduler   (tokio interval, parallel fetch_all)
                                            ├─ providers   (Provider trait + claude/codex/gemini/copilot/cursor/zai)
                                            ├─ login       (device-code OAuth: Codex/Gemini/Copilot)
                                            ├─ oauth_login (browser+localhost-callback OAuth: Codex)
                                            ├─ store       (accounts.json for user-added accounts)
                                            └─ secrets     (Keychain via /usr/bin/security + JSON files + SQLite)
```

- **Frontend** is provider-agnostic: a single `ServiceUsage` shape renders
  every provider uniformly. Adding a provider is a TypeScript union entry plus
  one inline SVG mark — no component changes.
- **Tray** shows the highest usage % across all windows; left-click toggles
  the dashboard; closing the window hides it to the tray (the app keeps
  running and polling every 5 minutes by default).
- **Stored accounts** (`accounts.json`) hold the user-added credentials.
  `fetch_credential` checks `expires_at` before each poll and refreshes in-app
  when the access token is expired, persisting the rotated tokens back.
- **`list_accounts`** masks to `{id, provider, label}` — secrets never cross
  IPC.

## Config

Poll interval and per-provider enable flags are exposed via the `get_config` /
`set_config` IPC commands (`AppConfig { poll_seconds ≥ 30, enabled[6] }` in
the order `[Claude, Codex, Gemini, Copilot, Cursor, z.ai]`).

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
- Claude's `console.anthropic.com` refresh endpoint can be Cloudflare-rate-
  limited under heavy load — the app falls back to `platform.claude.com/v1/oauth/token`.
- macOS is the primary dev/test platform; Linux/Windows file paths are handled
  but not CI-verified.
- The z.ai usage endpoint is undocumented and may change without notice.
