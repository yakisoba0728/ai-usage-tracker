# AI-Usage-Tracker — OAuth Credential & Refresh Research (Source-Verified)

Scope: how to (A) read each provider's stored credential, (B) **self-refresh** an expired
access token using the official CLI's *public* `client_id`, and (C) perform a from-scratch
login (device-code / auth-code+PKCE) with that same public `client_id`, and (D) ToS/risk notes.

Verification tiers used below:
- **[SRC]** — read directly from the CLI's source code on GitHub/npm (primary).
- **[DOC]** — official provider documentation.
- **[COMM]** — community/reverse-engineered reference (corroborated, not officially blessed).

Priority providers (**Claude** and **Codex**) are the two whose tokens currently require a
manual CLI run to refresh; both are now fully self-refreshable with source-verified values.

> Rule of thumb across all providers: a `client_id` is **not** a secret (it is shipped inside
> every installed CLI / public app). The `refresh_token`, `access_token`, `authorization_code`,
> and `device_code` **are** secrets — never log or transmit them beyond the OAuth endpoints.

---

## 1. Claude — Claude Code (Anthropic)

### A. Credential storage  **[SRC]** code.claude.com/docs + existing `src-tauri/src/secrets.rs`

| OS | Backend | Location / key |
|----|---------|----------------|
| **macOS** | Keychain (generic password) | service = **`Claude Code-credentials`**, account = **current OS username** (NOT empty — the `keyring` crate's `Entry::new(service, "")` does **not** work; must enumerate accounts via `security find-generic-password -s "Claude Code-credentials"`). **[SRC]** `secrets.rs` |
| **Linux** | JSON file, mode `0600` | `~/.claude/.credentials.json` |
| **Windows** | JSON file (user ACL) | `%USERPROFILE%\.claude\.credentials.json` |
| Any (override) | JSON file | `$CLAUDE_CONFIG_DIR/.credentials.json` if `CLAUDE_CONFIG_DIR` is set (Linux/Windows) |

**JSON schema** (keychain value == file content) **[COMM + existing `claude.rs`]**:

```jsonc
{
  "claudeAiOauth": {
    "accessToken":  "sk-ant-oat...",          // short-lived bearer (secret)
    "refreshToken": "sk-ant-ort...",          // long-lived (secret) — used for self-refresh
    "expiresAt":    1748592684497,            // Unix **milliseconds**
    "scopes":       ["user:inference", "user:profile"]
  }
}
```
Legacy/flat fallback the parser should also accept: top-level `accessToken` / `expiresAt` /
`subscriptionType` (already handled in `claude.rs::resolve_creds`).

CI/headless alternative (no refresh needed): `claude setup-token` prints a ~1-year token to
export as `CLAUDE_CODE_OAUTH_TOKEN`. **[DOC]** code.claude.com/docs/en/authentication

**Robust read order on macOS**: `$CLAUDE_CONFIG_DIR/.credentials.json` →
`security find-generic-password -s 'Claude Code-credentials' -w` (try each account) →
`~/.claude/.credentials.json`. Linux/Windows: `CLAUDE_CONFIG_DIR` dir → `~/.claude` / `%USERPROFILE%\.claude`.

### B. OAuth token REFRESH  **[COMM]** (Anthropic does not publish this; values corroborated by multiple refs incl. gist cedws)

- **Token endpoint:** `POST https://console.anthropic.com/v1/oauth/token`
  (newer Claude Code builds reportedly also accept `https://platform.claude.com/v1/oauth/token`; try console first, fall back to platform.) **[COMM]**
- **Content-Type:** `application/json` (form-encoded also accepted by most deployments).
- **Body:**
  ```jsonc
  { "grant_type": "refresh_token",
    "refresh_token": "<refreshToken>",
    "client_id":     "9d1c250a-e61b-44d9-88ed-5944d1962f5e" }
  ```
- **LITERAL public client_id:** **`9d1c250a-e61b-44d9-88ed-5944d1962f5e`** **[COMM]** (extracted from the Claude Code CLI; same value appears in its authorize URL and redirect config).
- **No `client_secret`** (public/installed client). No PKCE on the refresh grant (PKCE only on the initial auth-code exchange).
- **Response:**
  ```jsonc
  { "access_token":  "sk-ant-oat...",
    "refresh_token": "sk-ant-ort...",   // ROTATED — see below
    "expires_in":    28800,
    "token_type":    "Bearer" }
  ```
- **Refresh-token rotation: YES.** Claude Code rotates `refresh_token` on each successful
  refresh. Always persist the *new* `refresh_token` (and `expiresAt = now + expires_in*1000`)
  back to the keychain/file; reusing the old refresh token fails. Concurrent refreshers race. **[COMM]**
- **Headers:** none required beyond `Content-Type`. A `User-Agent` is not enforced.

### C. Device-code / interactive LOGIN  **[COMM]** gist.github.com/cedws/3a2496...

Claude Code uses an **authorization-code + PKCE (S256)** browser flow — **not** RFC 8628
device-code. Exact constants:

```
authorize:   https://claude.ai/oauth/authorize
             ?response_type=code
             &client_id=9d1c250a-e61b-44d9-88ed-5944d1962f5e
             &redirect_uri=https://console.anthropic.com/oauth/code/callback
             &scope=org:create_api_key user:profile user:inference
             &code_challenge_method=S256
             &code_challenge=<base64url_no_pad(SHA256(verifier))>
             &state=<random>
token:       POST https://console.anthropic.com/v1/oauth/token
             grant_type=authorization_code
             code=<code>  code_verifier=<verifier>
             client_id=9d1c250a-e61b-44d9-88ed-5944d1962f5e
             redirect_uri=https://console.anthropic.com/oauth/code/callback
```
PKCE: `code_verifier` = 43–128 url-safe-random; `code_challenge` = `BASE64URL_NOPAD(SHA256(verifier))`.
After exchange, persist the returned `{access_token, refresh_token, expires_in}` into the
Claude credential store (same schema as §A). There is **no** documented device-code flow, so
from-scratch login requires a browser/loopback handler.

### D. ToS / risk note
Anthropic does not publish the subscription-OAuth endpoints or the `client_id` as a stable
public API; the values above are reverse-engineered from the shipped CLI. Reusing the CLI's
public `client_id` for standard OAuth `refresh_token` / `authorization_code` grants is
legitimate OAuth (RFC 6749/7636) and uses no anti-bot/WAF circumvention, but it is
**undocumented and can break** without notice (e.g. the `console.anthropic.com` →
`platform.claude.com` endpoint shift already observed). Prefer `claude setup-token`
(`CLAUDE_CODE_OAUTH_TOKEN`) for headless use where acceptable. Keep refresh tokens secret and
rate-limit refresh attempts.

---

## 2. Codex — OpenAI Codex CLI  (all **[SRC]** openai/codex `codex-rs/login`)

### A. Credential storage

**Config dir:** `$CODEX_HOME` (default `~/.codex`). **[SRC]** `secrets.rs`, config docs.

**`auth.json`** at `$CODEX_HOME/auth.json` — pretty-printed JSON, mode `0600`. **[SRC]** `auth/storage.rs` (`AuthDotJson`, `FileAuthStorage`):

```jsonc
{
  "auth_mode": "chatgpt",                 // AuthMode: chatgpt | apiKey | chatgptAuthTokens | agentIdentity | personalAccessToken | bedrockApiKey
  "OPENAI_API_KEY": null,                 // present only for API-key login
  "tokens": {
    "access_token":  "<jwt>",             // ChatGPT access JWT  (secret)
    "refresh_token": "<opaque>",          // OAuth refresh token (secret)
    "id_token":      "<jwt>",             // serialized as the raw JWT string (see token_data.rs)
    "account_id":    "<chatgpt_account_id>"
  },
  "last_refresh": "2026-06-18T12:00:00Z",
  "agent_identity": null,                 // JWT string or AgentIdentityAuthRecord
  "personal_access_token": null,
  "bedrock_api_key": null
}
```
`id_token`/`access_token` are JWTs; useful claims live under the namespaced object
`https://api.openai.com/auth` → `chatgpt_plan_type`, `chatgpt_user_id`, `chatgpt_account_id`,
`chatgpt_account_is_fedramp`; plus `email` / `https://api.openai.com/profile.email`. **[SRC]** `token_data.rs`.

**Keychain backend** (when `cli_auth_credentials_store = keyring` or `auto` falls back) **[SRC]** `auth/storage.rs`:
- **Service:** `Codex Auth`
- **Account (key):** `cli|{first 16 hex of SHA-256(canonical CODEX_HOME path)}` — e.g. `cli|a1b2c3d4e5f67890`
- **Value:** the JSON-serialized `AuthDotJson` (same shape above)
- Backed by the `keyring` crate → **macOS Keychain** generic password, **Linux Secret Service** (GNOME Keyring/KWallet via D-Bus), **Windows Credential Manager**.

**`cli_auth_credentials_store` modes** (in `$CODEX_HOME/config.toml`) **[DOC]** openai-codex.mintlify.app/configuration/basic:
`"auto"` (default; keyring→file fallback) · `"file"` (auth.json) · `"keyring"` (fail if unavailable) · `"ephemeral"` (memory only).

**Robust read order:** check ephemeral → env (`CODEX_ACCESS_TOKEN`, `OPENAI_API_KEY`) →
keyring account `cli|{…}` for the resolved `CODEX_HOME` → fall back to `$CODEX_HOME/auth.json`.

### B. OAuth token REFRESH  **[SRC]** `auth/manager.rs`

- **LITERAL `CLIENT_ID`:** `pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";`  (overridable via env `CODEX_APP_SERVER_LOGIN_CLIENT_ID`; accessor `oauth_client_id()`).
- **Token endpoint:** `const REFRESH_TOKEN_URL = "https://auth.openai.com/oauth/token";` (overridable via `CODEX_REFRESH_TOKEN_URL_OVERRIDE`). Revoke: `https://auth.openai.com/oauth/revoke`.
- **Method:** `POST`, **Content-Type: `application/json`**, body serialized via `RefreshRequest`:
  ```jsonc
  { "client_id":     "app_EMoamEEZ73f0CkXaXp7hrann",
    "grant_type":    "refresh_token",
    "refresh_token": "<refresh_token>" }
  ```
  (No `client_secret`, no `scope`, no PKCE on refresh — public client.)
- **Response (`RefreshResponse`):**
  ```jsonc
  { "id_token":      "<jwt>?",     // optional
    "access_token":  "<jwt>?",     // optional
    "refresh_token": "<opaque>?" } // optional — see rotation
  ```
- **Refresh-token rotation: YES.** The server returns a fresh `refresh_token`; the old one is
  single-use. Failure codes classified in source: `refresh_token_expired`, `refresh_token_reused`
  ("already used"), `refresh_token_invalidated` (revoked). On `refresh_token_reused` you must
  re-login — persisting the rotated token is mandatory. **[SRC]** `classify_refresh_token_failure`.
- **Persistence after refresh** (`persist_tokens`): rewrite `tokens.{id,access,refresh}_token`
  and bump `last_refresh = now`, back to whichever store (file or keyring) the creds came from.
- **Headers:** use the shared Codex client (`default_headers()`) which sets `originator: codex_cli_rs`
  and a `User-Agent` like `codex_cli_rs/<version> (<os> <ver>; <arch>) <terminal-ua>`. Mimicking
  these is optional but improves first-party identification; **not** a WAF requirement.

### C. Device-code / interactive LOGIN

Codex supports **both** an RFC-8628-style device flow **and** a localhost auth-code+PKCE flow,
both against issuer `https://auth.openai.com`.

**(1) Device-code flow** **[SRC]** `device_code_auth.rs`:
1. `POST https://auth.openai.com/api/accounts/deviceauth/usercode`
   body `{"client_id":"app_EMoamEEZ73f0CkXaXp7hrann"}` →
   `{"device_auth_id","user_code","interval"}` (interval is a string of seconds).
2. User opens **`https://auth.openai.com/codex/device`** and enters `user_code` (15-min TTL).
3. Poll `POST https://auth.openai.com/api/accounts/deviceauth/token`
   body `{"device_auth_id","user_code"}`. `200` →
   `{"authorization_code","code_challenge","code_verifier"}` (server-side PKCE); `403`/`404` →
   sleep `interval`s and retry (max 15 min).
4. Exchange at `POST https://auth.openai.com/oauth/token` (see (2) below) using
   `redirect_uri = https://auth.openai.com/deviceauth/callback` and the returned PKCE pair.

**(2) Browser auth-code + PKCE flow** **[SRC]** `server.rs`:
- Local callback server on `127.0.0.1:1455` (fallback `1457`); `redirect_uri = http://localhost:{port}/auth/callback`.
- **Authorize URL:** `https://auth.openai.com/oauth/authorize?` with:
  `response_type=code`, `client_id=app_EMoamEEZ73f0CkXaXp7hrann`,
  `redirect_uri=http://localhost:{port}/auth/callback`,
  `scope=openid profile email offline_access api.connectors.read api.connectors.invoke`,
  `code_challenge_method=S256`, `code_challenge=<…>`,
  `id_token_add_organizations=true`, `codex_cli_simplified_flow=true`, `state=<rand>`,
  `originator=codex_cli_rs`.
- PKCE **[SRC]** `pkce.rs`: `code_verifier = URL_SAFE_NO_PAD(rand 64 bytes)`; `code_challenge = URL_SAFE_NO_PAD(SHA256(verifier))`.
- **Exchange:** `POST https://auth.openai.com/oauth/token`, **Content-Type `application/x-www-form-urlencoded`**, body:
  `grant_type=authorization_code&code=…&redirect_uri=…&client_id=app_EMoamEEZ73f0CkXaXp7hrann&code_verifier=…`
  → `{id_token, access_token, refresh_token}`. (Note: refresh uses JSON, exchange uses form-encoded.)

### D. ToS / risk note
`app_EMoamEEZ73f0CkXaXp7hrann` is Codex CLI's own public client identifier, reused here only for
standard OAuth `refresh_token`/`authorization_code`/device grants (RFC 6749/7636/8628) against
the documented `auth.openai.com` endpoints — no browser/TLS impersonation, no WAF bypass. Risk is
operational not contractual: the `chatgpt.com/backend-api/codex/usage` **usage** endpoint
(unchanged by this work) remains behind bot-management and may reject non-browser clients; the
refresh/login endpoints themselves are stable first-party OAuth and are what Codex CLI calls
itself. Rotated refresh tokens must be persisted or the user is forced to re-login.

---

## 3. Gemini — Google Gemini CLI  (all **[SRC]** google-gemini/gemini-cli `packages/core/src/code_assist/oauth2.ts`)

### A. Credential storage

**File (default):** `~/.gemini/oauth_creds.json` (mode `0600`), path from `Storage.getOAuthCredsPath()`.
Also reads `$GOOGLE_APPLICATION_CREDENTIALS`. Optional encrypted-file store via
`FORCE_ENCRYPTED_FILE_ENV_VAR=true` + `OAuthCredentialStorage`. **[SRC]** `oauth2.ts` (`fetchCachedCredentials`, `cacheCredentials`).

The file is a `google-auth-library` `Credentials` object:
```jsonc
{
  "access_token":  "ya29...",                 // short-lived (secret)
  "refresh_token": "1//...",                  // long-lived (secret)
  "expiry_date":   1718700000000,             // Unix ms
  "token_type":    "Bearer",
  "scope":         "https://www.googleapis.com/auth/cloud-platform ...",
  "id_token":      "<jwt>?"
}
```
No OS keychain by default — file only (the CLI delegates refresh to `OAuth2Client`).

### B. OAuth token REFRESH  **[SRC]** oauth2.ts + Google Identity docs

- **LITERAL client_id:** `681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com`
- **LITERAL client_secret:** `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` *(source comment: "It's ok to save this … installed application … the client secret is obviously not treated as a secret.")*
- **Token endpoint:** `POST https://oauth2.googleapis.com/token` (form-encoded).
- **Form params:** `client_id`, `client_secret`, `refresh_token`, `grant_type=refresh_token`.
- **Response:** `{access_token, expires_in, refresh_token?, scope, token_type, id_token?}`.
- **Rotation:** Google does **not** rotate the refresh token on refresh by default (same refresh token remains valid until revoked). `expires_in` ≈ 3600.
- The CLI actually lets `OAuth2Client.refreshAccessToken()` build the request (the `client.on('tokens')` handler re-caches via `cacheCredentials`). The existing `gemini.rs::refresh_form` already builds this form correctly; just ensure it sends `client_secret` from the constants above when the creds file omits it.

### C. Device-code / interactive LOGIN

Gemini CLI uses **auth-code + PKCE (S256)**, **not** device-code:

- **Web/loopback flow:** binds `http://127.0.0.1:{port}` (port via `OAUTH_CALLBACK_PORT` or ephemeral; host via `OAUTH_CALLBACK_HOST`), `redirect_uri = http://127.0.0.1:{port}/oauth2callback`.
  Authorize via `client.generateAuthUrl({ redirect_uri, access_type:'offline', scope:OAUTH_SCOPE })` at `https://accounts.google.com/o/oauth2/v2/auth`, exchange code with `client.getToken({code, redirect_uri})`.
- **No-browser / manual flow** (`NO_BROWSER`): `redirect_uri = https://codeassist.google.com/authcode`, `code_challenge_method=S256`, user pastes the code back.
- **Scopes:** `https://www.googleapis.com/auth/cloud-platform`, `…/auth/userinfo.email`, `…/auth/userinfo.profile`; `access_type=offline` (to get a refresh_token).
- Post-auth fetches account email from `https://www.googleapis.com/oauth2/v2/userinfo`.

### D. ToS / risk note
These are Google's own published "Desktop app" OAuth credentials shipped in the open-source
CLI; Google's OAuth policies explicitly treat the client_secret of an installed app as
non-secret. Standard OAuth use is compliant. The Code Assist usage API
(`https://cloudcode-pa.googleapis.com/v1internal`) is internal/undocumented and may change;
that is unchanged by this work (already used by the existing provider).

---

## 4. Copilot — GitHub `gh` CLI  (all **[SRC]** cli/cli `internal/authflow/flow.go` + GitHub OAuth docs)

### A. Credential storage
- **File:** `~/.config/gh/hosts.yml` (or `$GH_CONFIG_DIR/hosts.yml` / `$XDG_CONFIG_HOME/gh/hosts.yml`), keyed by host (e.g. `github.com`):
  ```yaml
  github.com:
      user: <login>
      oauth_token: gho_...        # long-lived user token (secret)
      git_protocol: https
  ```
- **No OS keychain by default.** `gh` stores tokens in `hosts.yml` in plaintext; it does **not** use macOS Keychain / Secret Service / Credential Manager. (`gh auth token` reads from this file / `$GH_TOKEN` / `$GITHUB_TOKEN`.)
- Env overrides take precedence: `$GH_TOKEN` then `$GITHUB_TOKEN`.

### B. Token REFRESH
- **GitHub OAuth user tokens do not expire and have no refresh grant.** There is no refresh
  endpoint to call — a valid `gho_…` token persists until revoked. Self-refresh is therefore a
  no-op for Copilot; if the token is revoked/missing, fall straight to re-login (§C).
- (Tokens minted by GitHub Apps *can* expire, but the `gh` device-flow token above does not.)

### C. Device-code LOGIN (RFC 8628)  **[SRC]** flow.go + GitHub docs
- **LITERAL client_id:** `178c6fc778ccc68e1d6a` (the "GitHub CLI" OAuth app); secret `34ddeff2b558a23d38fba8a6de74f086ede1cc0b` (**not required** for the device flow).
- **Request codes:** `POST https://github.com/login/device/code`
  body `client_id=178c6fc778ccc68e1d6a&scope=repo read:org gist` →
  `{device_code, user_code, verification_uri, expires_in, interval}`.
- **User authorizes** at `https://github.com/login/device` (enter `user_code`).
- **Poll:** `POST https://github.com/login/oauth/access_token`
  body `client_id=178c6fc778ccc68e1d6a&device_code=…&grant_type=urn:ietf:params:oauth:grant-type:device_code`
  → on success `{access_token, token_type, scope}`; on pending `error:"authorization_pending"`/`slow_down` (honor `interval`).
- Default scopes: `repo read:org gist` (the billing/usage endpoint needs `read:org`+`user`-ish scopes — surface a hint if missing, as `copilot.rs` does).
- Non-device fallback: web app flow with loopback callback `http://127.0.0.1/callback` (Enterprise hosts use `http://localhost/`).

### D. ToS / risk note
`178c6fc778ccc68e1d6a` is GitHub's own first-party CLI OAuth app, explicitly flagged in-source as
safe to embed in version control; GitHub documents the device-code flow publicly. Using it is
compliant and is exactly what `gh auth login --web` does. No refresh needed; no anti-bot
circumvention involved.

---

## 5. Cursor — WorkOS-backed app account  **[COMM/DOC]** (no public client_id)

### A. Credential storage
- **SQLite DB** (VS Code-style global state), `ItemTable(key, value)`:
  - macOS: `~/Library/Application Support/Cursor/User/globalStorage/state.vscdb`
  - Linux: `~/.config/Cursor/User/globalStorage/state.vscdb`
  - Windows: `%APPDATA%\Cursor\User\globalStorage\state.vscdb`
- Relevant keys: `cursorAuth/accessToken`, `cursorAuth/refreshToken`, `cursorAuth/cachedEmail`
  (and `WorkosCursorSessionToken`-style session values). **[COMM]** DeepWiki/Open VSX refs.

### B. Token REFRESH
- Community/Docker refs mention endpoints `https://api2.cursor.sh/oauth/token`, `/auth/poll`, `/auth/usage`, but Cursor's auth is **WorkOS-based and its `client_id` is NOT public/published.**
- **[INFERENCE]** Unlike Claude/Codex/Gemini/Copilot, there is no reusable public `client_id` to
  drive a standard OAuth refresh, so the app cannot fully self-refresh Cursor from scratch. Best
  effort: reuse `cursorAuth/accessToken` while valid; if expired, attempt `POST
  https://api2.cursor.sh/oauth/token` with `grant_type=refresh_token` +
  `cursorAuth/refreshToken` **[COMM]** (may fail without Cursor's private client id). Keep
  Cursor marked "unstable" and degrade honestly — do not fabricate usage.

### C. Device-code / interactive LOGIN
- Cursor account login is a **browser/WorkOS** flow; no RFC 8628 device flow is documented for
  the Cursor account itself. From-scratch login is out of scope without Cursor's private auth
  client. (Cursor does support API-key auth for its CLI — `docs.cursor.com/en/cli/reference/authentication`.)

### D. ToS / risk note
Cursor's account-auth endpoints are undocumented/private and `api2.cursor.sh` usage calls are
WAF-protected. Reusing the local access token for the documented usage surface is low-risk, but
driving OAuth refresh/login without a public client_id is unreliable and may conflict with
Cursor ToS; do not impersonate the Cursor app or spoof its client identity. Treat as
best-effort/experimental.

---

## Cross-provider implementation summary for the tracker

| Provider | Self-refresh possible? | Endpoint | client_id (literal) | Rotates refresh_token? |
|----------|------------------------|----------|---------------------|------------------------|
| **Claude** | ✅ | `console.anthropic.com/v1/oauth/token` | `9d1c250a-e61b-44d9-88ed-5944d1962f5e` | **Yes** |
| **Codex**  | ✅ | `auth.openai.com/oauth/token` (JSON body) | `app_EMoamEEZ73f0CkXaXp7hrann` | **Yes** |
| **Gemini** | ✅ | `oauth2.googleapis.com/token` | `681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j…apps.googleusercontent.com` (+secret) | No |
| **Copilot**| n/a (tokens don't expire) | — | `178c6fc778ccc68e1d6a` (login only) | n/a |
| **Cursor** | ⚠️ partial (no public client_id) | `api2.cursor.sh/oauth/token` **[COMM]** | not published | unknown |

**Concrete elimination of the "run `claude`/`codex` once" UX:** for Claude and Codex, when the
stored `accessToken` is within the refresh window (Codex refreshes ~5 min before expiry; Claude
on expiry), the tracker POSTs the refresh grant above with the CLI's public `client_id`,
persists the rotated `refresh_token` + new `access_token` back into the **same** store it read
from (Claude keychain/`.credentials.json`; Codex `auth.json` or `Codex Auth` keyring), then
calls the usage API — no manual CLI run required. Only when the refresh returns a permanent
error (`refresh_token_reused`/`expired`/`invalidated` for Codex; `invalid_grant` for Claude)
should the UI prompt a re-login (device flow for Codex; browser/PKCE for Claude).

---

### Source index (primary, all read directly)
- Gemini: `https://raw.githubusercontent.com/google-gemini/gemini-cli/main/packages/core/src/code_assist/oauth2.ts` (client id/secret, scopes, PKCE flows); Google OAuth `https://developers.google.com/identity/protocols/oauth2/web-server`.
- Codex: `https://raw.githubusercontent.com/openai/codex/main/codex-rs/login/src/{lib.rs, server.rs, device_code_auth.rs, pkce.rs, token_data.rs, auth/{mod.rs, storage.rs, manager.rs, default_client.rs}}`; config `https://openai-codex.mintlify.app/configuration/basic`.
- Copilot/gh: `https://github.com/cli/cli/blob/trunk/internal/authflow/flow.go`; GitHub device-flow docs `https://docs.github.com/apps/authorizing-oauth-apps/building-oauth-apps/authorizing-oauth-apps#device-flow`.
- Claude: `https://code.claude.com/docs/en/authentication`; community `https://gist.github.com/cedws/3a2496c7569bb610e24aa90dd217d9f2`.
- Cursor: `https://docs.cursor.com/en/cli/reference/authentication`; `https://docs.docker.com/ai/sandboxes/agents/cursor/`; DeepWiki cursor-auto-icloud auth-management.
- Local tracker baseline: `src-tauri/src/{secrets.rs, providers/{claude,codex,gemini,copilot,cursor}.rs}`.
