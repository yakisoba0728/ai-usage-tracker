# AI Usage Tracker

A Tauri 2 desktop app that shows **live, real** subscription usage for AI coding
services in your menu bar / a click-to-open dashboard. It **parses the OAuth
tokens each service's official CLI already stores** and calls that provider's
usage API directly — **no estimation, no separate login, no app-side OAuth, no
self-issued OAuth app**.

> **Token-parsing only.** The app does **not** perform any OAuth itself — no token
> refresh, no login flow. It reads whatever token the relevant CLI last wrote and
> uses it. Token rotation is the CLI's job; if a token is expired the card says so
> (e.g. "run `claude` once to refresh"). This matches how `claude-meter` works.

## Supported providers

| Provider | Source of token | Usage API | Notes |
|---|---|---|---|
| **Claude** | Claude Code — macOS Keychain (`Claude Code-credentials`) / `~/.claude/.credentials.json` | `api.anthropic.com/api/oauth/usage` (+ `/profile`) | Token-parsing only; the Claude Code CLI rotates the token. On expiry: "run `claude` once". |
| **Codex** | Codex CLI — `~/.codex/auth.json` (`$CODEX_HOME` honored) | `chatgpt.com/backend-api/codex/usage` | Endpoint is WAF-protected; when blocked the card still shows your plan/account (from the `id_token`) with an honest note. No WAF circumvention. |
| **Gemini** | Gemini CLI — `~/.gemini/oauth_creds.json` | Google Code Assist `loadCodeAssist` / `retrieveUserQuota` | Token-parsing only; the Gemini CLI rotates the token. On expiry: "run `gemini` once". |
| **GitHub Copilot** | `gh` CLI token (`gh auth token`) + user from `~/.config/gh/hosts.yml` | `api.github.com/.../billing/ai_credit/usage` (official) | Needs the `user` billing scope; if missing the card says "run `gh auth refresh -h github.com -s user`". |
| **Cursor** (experimental) | Cursor `state.vscdb` (SQLite globalStorage) | `api2.cursor.sh/...GetCurrentPeriodUsage` (Connect-RPC) | Undocumented; marked experimental. No WAF circumvention. |

### Design principles

- **Token-parsing only.** No app-side OAuth (no refresh, no login); tokens are read
  from where each CLI stores them, and rotated by that CLI.
- **Real data only.** A window is omitted rather than guessed.
- **Per-provider isolation.** One failing provider never breaks the others or the
  scheduler.
- **Tokens stay in Rust.** Only non-secret usage snapshots cross IPC to the UI.
- **No telemetry.** The only outbound calls are to each provider's own endpoints.

## Prerequisites

Install and sign in to the CLIs for the services you want to track (they own token
rotation):

```bash
# Claude    — `claude` then /login   (re-run when the token expires)
# Codex     — `codex login`
# Gemini    — `gemini` (Google login) (re-run when the token expires)
# Copilot   — `gh auth login`  (then `gh auth refresh -h github.com -s user` for billing scope)
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
                                            └─ secrets     (Keychain via /usr/bin/security + JSON files)
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
- Codex — `openai/codex` `codex-rs/login` (Apache-2.0)
- Copilot — GitHub REST billing API (official)

## Status / limitations

- v1 ships live current values only (no history/trends).
- **No app-side OAuth.** The app parses tokens only; it does not refresh or log in.
  Each CLI owns token rotation. (A prior attempt at Claude self-refresh was removed:
  `console.anthropic.com` is Cloudflare-bot-managed, and `claude-meter` itself
  delegates to the CLI for the same reason.)
- Codex/Cursor usage availability depends on each provider's WAF; the app degrades
  honestly (shows account/plan + a note) rather than circumventing bot protection.
- macOS is the primary dev/test platform; Linux/Windows file paths are handled but
  not CI-verified.
