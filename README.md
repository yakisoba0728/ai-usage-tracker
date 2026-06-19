# AI Usage Tracker

A Tauri 2 desktop app that shows **live, real** subscription usage for AI coding
services in your menu bar / a click-to-open dashboard. It **parses the OAuth
tokens each service's official CLI already stores** and calls that provider's
usage API directly — **no estimation, no separate login, no app-side OAuth, no
self-issued OAuth app**.

> **Token-parsing only.** The app does **not** perform any OAuth itself — no token
> refresh, no login flow. It reads whatever token the relevant CLI last wrote and
> uses it. Token rotation is the CLI's job; if a token is expired the card says so
> (e.g. "run `claude` once to refresh").

## Supported providers

| Provider | Source of token | Usage API | Notes |
|---|---|---|---|
| **Claude** | Claude Code — macOS Keychain (`Claude Code-credentials`) / `~/.claude/.credentials.json` | `api.anthropic.com/api/oauth/usage` (+ `/profile`) | Modeled on `claude-meter`. `utilization` is already 0–100 (no scaling); `extra_usage` credits are cents→dollars. CLI rotates the token. |
| **Codex** | Codex CLI — `~/.codex/auth.json` (`$CODEX_HOME` honored) | `chatgpt.com/backend-api/wham/usage` | **The real endpoint the codex CLI polls.** Sent with the `codex_cli_rs/` User-Agent (Cloudflare allow-lists the CLI UA) + `ChatGPT-Account-Id`. Returns `rate_limit.primary_window` (5h) + `secondary_window` (weekly) `used_percent`. Verified live (HTTP 200). |
| **Gemini** | Gemini CLI — `~/.gemini/oauth_creds.json` | Google Code Assist `loadCodeAssist` / `retrieveUserQuota` | Per-bucket `used_pct = (1 − remainingFraction) × 100`; `limit = remainingAmount / remainingFraction`. CLI rotates the token. |
| **GitHub Copilot** | **Copilot CLI** token — macOS Keychain `copilot-cli` / `~/.copilot/config.json` | `api.github.com/copilot_internal/user` | The `gh` CLI token does **not** work (wrong client, no Copilot scope) — run `copilot login`. Returns `quota_snapshots.premium_interactions` (entitlement/remaining/percent) + reset date. |
| **Cursor** (experimental) | `state.vscdb` → `ItemTable[cursorAuth/accessToken]` (raw JWT) | Connect-RPC `api2.cursor.sh/aiserver.v1.DashboardService/GetCurrentPeriodUsage` | POST with `Connect-Protocol-Version: 1`, body `{}`. Response `planUsage` is USD cents → dollars. Experimental. |

### Design principles

- **Token-parsing only.** No app-side OAuth (no refresh, no login); tokens are read
  from where each CLI stores them, and rotated by that CLI.
- **Real data only.** A window is omitted rather than guessed.
- **Faithful to the reference projects.** Each provider's URL, headers, and
  response parsing mirror the actual reference implementation (claude-meter,
  gemini-cli-usage, openai/codex `backend-client`, opencode-mystatus,
  ClearMeasureLabs/cursor-usage-status).
- **Per-provider isolation.** One failing provider never breaks the others or the
  scheduler.
- **Tokens stay in Rust.** Only non-secret usage snapshots cross IPC to the UI.

## Prerequisites

Install and sign in to the CLIs for the services you want to track (they own token
rotation):

```bash
# Claude    — `claude` then /login   (re-run when the token expires)
# Codex     — `codex login`
# Gemini    — `gemini` (Google login) (re-run when the token expires)
# Copilot   — `copilot login`   (the Copilot CLI — NOT `gh`)
# Cursor    — sign in to the Cursor app
```

## Develop

```bash
pnpm install
pnpm tauri dev      # launches the app + tray (Vite HMR)
```

Rust unit tests (parsing/normalize/isolation — the testable surface):

```bash
cd src-tauri && cargo test --lib
```

## Build

```bash
CI= pnpm tauri build     # produces src-tauri/target/release/bundle/macos/AI Usage Tracker.app
```

(The `.app` is the runnable deliverable. The `.dmg` step uses `hdiutil` and may
fail in some sandboxes; if so, distribute the `.app` directly.)

## Architecture

```
React + TS + Tailwind + shadcn/ui  ──IPC──▶  Rust (Tauri 2)
                                            ├─ scheduler   (tokio interval, parallel fetch_all)
                                            ├─ providers   (Provider trait + claude/codex/gemini/copilot/cursor)
                                            └─ secrets     (Keychain via /usr/bin/security + JSON files + SQLite)
```

- Tray shows the highest usage % across all windows; left-click toggles the
  dashboard; closing the window hides it to the tray (the app keeps running and
  polling every 5 minutes by default).
- Each provider parses its stored token, calls its usage API, and normalizes the
  response into a unified `ServiceUsage`, so the dashboard renders all providers
  uniformly while preserving per-provider detail.

## Config

Poll interval and per-provider enable flags are exposed via the `get_config` /
`set_config` IPC commands (`AppConfig { poll_seconds ≥ 30, enabled[5] }` in the
order `[Claude, Codex, Gemini, Copilot, Cursor]`).

## References (API contracts ported to Rust)

- Claude — `m13v/claude-meter` (MIT, Rust)
- Gemini — `wakamex/gemini-cli-usage` (MIT, Python)
- Codex — `openai/codex` `codex-rs/backend-client` (Apache-2.0)
- Copilot — `vbgate/opencode-mystatus` (MIT) + GitHub Copilot extension headers
- Cursor — `ClearMeasureLabs/cursor-usage-status` (MIT)

## Status / limitations

- v1 ships live current values only (no history/trends).
- **No app-side OAuth.** The app parses tokens only; it does not refresh or log in.
  Each CLI owns token rotation. (Claude's refresh endpoint `console.anthropic.com`
  is Cloudflare-bot-managed, and `claude-meter` delegates to the CLI for the same
  reason — so an expired Claude token needs `claude` once.)
- Provider cards are honest about state: a missing/expired token or an
  access-restricted endpoint shows the cause + the CLI to run, never fabricated
  numbers.
- macOS is the primary dev/test platform; Linux/Windows file paths are handled but
  not CI-verified.
