# AI Usage Tracker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a Tauri 2 tray+dashboard desktop app that shows live, real Claude/Codex/Gemini subscription usage by reusing each official CLI's stored OAuth token and calling its unofficial usage API.

**Architecture:** Rust-native Tauri 2 backend owns all token reading, refresh, HTTP calls, and a tokio background scheduler; React+TS+Tailwind+shadcn frontend renders usage snapshots received over Tauri IPC. Tokens never leave the backend.

**Tech Stack:** Tauri 2, Rust (reqwest+rustls, keyring, serde, tokio, thiserror, chrono, base64), React 19, Vite, TypeScript, Tailwind, shadcn/ui.

## Global Constraints

- **Never register or ship our own OAuth client.** Reuse tokens already stored by Claude Code / Codex CLI / Gemini CLI.
- **No usage estimation.** Only real server-side values; omit a window rather than guess.
- **Tokens stay in Rust.** They are never serialized to the frontend, never logged.
- **Per-provider isolation.** A failure in one provider must not abort the scheduler or break other providers.
- **Cross-platform:** macOS (keychain for Claude) + Linux/Windows (file paths). Resolve home dir via `dirs` crate, never hardcode `/Users`.
- **Refresh policy (v1):** Claude & Codex = read-only + hint on expiry; Gemini = self-refresh via stored client metadata.
- **No telemetry.** The only outbound calls are to the three providers' own usage/token endpoints.
- Commit after every task (`feat:`/`test:`/`chore:` prefixes).

---

## File Structure

```
src-tauri/
  Cargo.toml
  tauri.conf.json
  build.rs
  src/
    main.rs              # Tauri bootstrap: tray, commands, scheduler wiring
    lib.rs               # re-exports for tests
    model.rs             # Provider/ServiceUsage/LimitWindow/UsageSnapshot
    config.rs            # AppConfig (poll interval, autostart, enabled flags)
    http.rs              # shared reqwest client builder
    secrets.rs           # cross-platform keychain + file credential reads
    jwt.rs               # no-verify JWT payload decode (base64 manual)
    commands.rs          # #[tauri::command] handlers + event emission
    scheduler.rs         # tokio interval; fetch_all over [Provider]
    providers/
      mod.rs             # Provider trait + async fetch_all
      claude.rs          # keychain/file read + /api/oauth/usage (no refresh)
      codex.rs           # auth.json read + id_token decode + codex/usage
      gemini.rs          # oauth_creds read + self-refresh + Code Assist quota
src/
  main.tsx
  App.tsx
  lib/types.ts           # mirrors Rust model
  lib/ipc.ts             # invoke/listen wrappers
  hooks/useUsage.ts
  components/Dashboard.tsx
  components/ServiceCard.tsx
  components/Gauge.tsx
  components/Header.tsx
```

---

## Task 1: Scaffold Tauri 2 + React + TS + Tailwind + shadcn

**Files:**
- Create: project root via `create-tauri-app`, plus `tailwind.config.js`, `src/index.css`, shadcn config.

**Interfaces:**
- Produces: a runnable `pnpm tauri dev` shell with a blank React window.

- [ ] **Step 1: Scaffold the app**

```bash
cd ~/Documents/GitHub/ai-usage-tracker
pnpm create tauri-app . --template react-ts --manager pnpm --yes
```
If the directory is non-empty (docs/), the scaffolder may refuse; if so, scaffold into a temp dir and move files:
```bash
cd /tmp && pnpm create tauri-app aitmp --template react-ts --manager pnpm --yes
cp -R /tmp/aitmp/. ~/Documents/GitHub/ai-usage-tracker/
```
Expected: `src-tauri/`, `src/`, `package.json`, `index.html` present.

- [ ] **Step 2: Add Tailwind + shadcn/ui**

```bash
cd ~/Documents/GitHub/ai-usage-tracker
pnpm install
pnpm add -D tailwindcss postcss autoprefixer
npx tailwindcss init -p
pnpm add tailwindcss-animate class-variance-authority clsx tailwind-merge lucide-react
pnpm add @radix-ui/react-progress @radix-ui/react-tooltip @radix-ui/react-switch @radix-ui/react-slot
npx shadcn@latest init -y   # accept defaults (style=new-york, base color)
```
Expected: `tailwind.config.js`, `postcss.config.js`, `src/components/ui/` seeded.

- [ ] **Step 3: Configure Tailwind content paths**

`tailwind.config.js` → set `content: ["./index.html","./src/**/*.{ts,tsx}"]`.

- [ ] **Step 4: Verify the dev shell runs**

```bash
pnpm tauri dev
```
Expected: a native window opens showing the default Vite React app. Stop it (Ctrl-C).

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "chore: scaffold tauri 2 + react + ts + tailwind + shadcn"
```

---

## Task 2: Core data model + Provider trait

**Files:**
- Create: `src-tauri/src/model.rs`, `src-tauri/src/providers/mod.rs`
- Modify: `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml` (add `serde`, `thiserror`, `chrono`, `async-trait`)

**Interfaces:**
- Produces: `Provider` enum, `ServiceUsage`, `LimitWindow`, `UsageSnapshot`, `ProviderError`, `ProviderApi` trait (async `fetch(&self) -> ServiceUsage`).

- [ ] **Step 1: Add dependencies**

`src-tauri/Cargo.toml` `[dependencies]`:
```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
chrono = { version = "0.4", features = ["serde"] }
async-trait = "0.1"
tokio = { version = "1", features = ["full"] }
```

- [ ] **Step 2: Write the failing test for the model**

`src-tauri/src/model.rs`:
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider { Claude, Codex, Gemini }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitWindow {
    pub label: String,
    pub used_percent: Option<f32>,
    pub resets_at: Option<i64>,
    pub used: Option<f64>,
    pub limit: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceUsage {
    pub provider: Provider,
    pub connected: bool,
    pub plan: Option<String>,
    pub account: Option<String>,
    pub error: Option<String>,
    pub windows: Vec<LimitWindow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSnapshot {
    pub fetched_at: i64,
    pub services: Vec<ServiceUsage>,
}

impl UsageSnapshot {
    /// Highest used_percent across every window of every connected service.
    pub fn max_used_percent(&self) -> Option<f32> {
        self.services.iter()
            .filter(|s| s.connected)
            .flat_map(|s| s.windows.iter())
            .filter_map(|w| w.used_percent)
            .max_by(|a, b| a.partial_cmp(b).unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_max_percent_picks_highest_connected() {
        let snap = UsageSnapshot {
            fetched_at: 0,
            services: vec![
                ServiceUsage { provider: Provider::Claude, connected: true,
                    plan: None, account: None, error: None,
                    windows: vec![LimitWindow { label: "5h".into(), used_percent: Some(40.0),
                        resets_at: None, used: None, limit: None }] },
                ServiceUsage { provider: Provider::Gemini, connected: true,
                    plan: None, account: None, error: None,
                    windows: vec![LimitWindow { label: "pro".into(), used_percent: Some(72.5),
                        resets_at: None, used: None, limit: None }] },
                ServiceUsage { provider: Provider::Codex, connected: false,
                    plan: None, account: None, error: Some("offline".into()), windows: vec![] },
            ],
        };
        assert_eq!(snap.max_used_percent(), Some(72.5));
    }

    #[test]
    fn provider_serializes_lowercase() {
        let s = serde_json::to_string(&Provider::Claude).unwrap();
        assert_eq!(s, "\"claude\"");
    }
}
```

- [ ] **Step 3: Run the test (fails — module not declared)**

```bash
cd src-tauri && cargo test model::tests -- --nocapture
```
Expected: compile error (module not declared in lib.rs).

- [ ] **Step 4: Declare modules + define the Provider trait**

`src-tauri/src/providers/mod.rs`:
```rust
use async_trait::async_trait;
use crate::model::ServiceUsage;
use crate::secrets::SecretsError;

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("credentials not found: {0}")]
    NotLoggedIn(String),
    #[error("token expired: {0}")]
    Expired(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("unexpected response ({status}): {body}")]
    Status { status: u16, body: String },
    #[error("parse error: {0}")]
    Parse(String),
}

impl From<SecretsError> for ProviderError {
    fn from(e: SecretsError) -> Self { ProviderError::NotLoggedIn(e.to_string()) }
}

#[async_trait]
pub trait ProviderApi: Send + Sync {
    fn key(&self) -> crate::model::Provider;
    async fn fetch(&self) -> Result<ServiceUsage, ProviderError>;
}

/// Run every provider, returning one ServiceUsage each (errors downgraded to a
/// disconnected ServiceUsage). A failing provider never panics the batch.
pub async fn fetch_all(providers: &[Box<dyn ProviderApi>]) -> Vec<ServiceUsage> {
    let mut out = Vec::with_capacity(providers.len());
    for p in providers {
        let usage = match p.fetch().await {
            Ok(u) => u,
            Err(e) => ServiceUsage {
                provider: p.key(),
                connected: false,
                plan: None,
                account: None,
                error: Some(e.to_string()),
                windows: vec![],
            },
        };
        out.push(usage);
    }
    out
}
```

`src-tauri/src/lib.rs`:
```rust
pub mod model;
pub mod secrets;   // created in Task 3
pub mod providers;
```
(If `secrets` doesn't exist yet, temporarily stub an empty `secrets.rs` with `SecretsError` — but Task 3 creates it. To keep Task 2 compiling standalone, define a placeholder now and replace in Task 3.)

Placeholder `src-tauri/src/secrets.rs` (replaced fully in Task 3):
```rust
#[derive(Debug, thiserror::Error)]
pub enum SecretsError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("read error: {0}")]
    Read(String),
}
```

- [ ] **Step 5: Run the test (passes)**

```bash
cd src-tauri && cargo test model::tests -- --nocapture
```
Expected: `2 passed`.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat(core): data model + Provider trait + fetch_all"
```

---

## Task 3: Secrets module (keychain + file reads)

**Files:**
- Create: `src-tauri/src/secrets.rs`
- Modify: `src-tauri/Cargo.toml` (add `keyring = "3"`, `dirs = "5"`)

**Interfaces:**
- Consumes: nothing.
- Produces: `SecretsError`, `read_keychain_json(service) -> Result<Value>`, `read_json_file(home_rel) -> Result<Value>`, `claude_token_path() -> PathBuf`, `codex_auth_path() -> PathBuf`, `gemini_creds_path() -> PathBuf`.

- [ ] **Step 1: Add deps**

`src-tauri/Cargo.toml`: `keyring = "3"`, `dirs = "5"`.

- [ ] **Step 2: Write the failing test**

Replace `src-tauri/src/secrets.rs` fully:
```rust
use std::path::PathBuf;
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum SecretsError {
    #[error("credential not found: {0}")]
    NotFound(String),
    #[error("credential read failed: {0}")]
    Read(String),
    #[error("credential parse failed: {0}")]
    Parse(String),
}

pub fn claude_token_path() -> PathBuf {
    let base = std::env::var("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs::home_dir().expect("home dir").join(".claude"));
    base.join(".credentials.json")
}

pub fn codex_auth_path() -> PathBuf {
    dirs::home_dir().expect("home dir").join(".codex").join("auth.json")
}

pub fn gemini_creds_path() -> PathBuf {
    dirs::home_dir().expect("home dir").join(".gemini").join("oauth_creds.json")
}

/// Read a JSON file from disk (used on Linux/Windows and for Codex/Gemini).
pub fn read_json_file(path: &PathBuf) -> Result<Value, SecretsError> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| SecretsError::NotFound(format!("{}: {e}", path.display())))?;
    serde_json::from_str::<Value>(&raw)
        .map_err(|e| SecretsError::Parse(format!("{}: {e}", path.display())))
}

/// Read the macOS Keychain generic-password `service` and JSON-decode its value.
/// On non-macOS this returns NotFound.
pub fn read_keychain_json(service: &str) -> Result<Value, SecretsError> {
    let entry = keyring::Entry::new(service, "").map_err(|e| SecretsError::Read(e.to_string()))?;
    let raw = entry.get_password().map_err(|e| SecretsError::NotFound(format!("{service}: {e}")))?;
    serde_json::from_str::<Value>(&raw).map_err(|e| SecretsError::Parse(format!("{service}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_json_file_parses_and_errors_on_missing() {
        let tmp = std::env::temp_dir().join("ait_secrets_test.json");
        std::fs::write(&tmp, r#"{"k": 1}"#).unwrap();
        let v = read_json_file(&tmp).unwrap();
        assert_eq!(v["k"], 1);

        let missing = std::env::temp_dir().join("ait_does_not_exist_xyz.json");
        assert!(matches!(read_json_file(&missing), Err(SecretsError::NotFound(_))));
    }

    #[test]
    fn paths_are_under_home() {
        assert!(codex_auth_path().ends_with(".codex/auth.json"));
        assert!(gemini_creds_path().ends_with(".gemini/oauth_creds.json"));
    }
}
```

- [ ] **Step 3: Run the test (fails — needs keyring/dirs compiled)**

```bash
cd src-tauri && cargo test secrets::tests -- --nocapture
```
Expected: `2 passed` (after deps resolve). If `keyring` fails to build on Linux CI, gate its use behind the `apple`/platform as needed; the unit tests only touch file paths.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat(secrets): keychain + JSON credential reads"
```

---

## Task 4: JWT no-verify decode helper

**Files:**
- Create: `src-tauri/src/jwt.rs`
- Modify: `src-tauri/Cargo.toml` (add `base64 = "0.22"`), `src-tauri/src/lib.rs`

**Interfaces:**
- Produces: `jwt_payload(token: &str) -> Result<Value, String>` (metadata only; no signature verification).

- [ ] **Step 1: Add dep** — `base64 = "0.22"`.

- [ ] **Step 2: Write the failing test + impl**

`src-tauri/src/jwt.rs`:
```rust
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::Value;

/// Decode a JWT's payload segment WITHOUT verifying the signature.
/// Intended for non-secret metadata only (plan, email, account id, exp).
pub fn jwt_payload(token: &str) -> Result<Value, String> {
    use base64::Engine;
    let segs: Vec<&str> = token.split('.').collect();
    if segs.len() < 2 { return Err("not a JWT".into()); }
    let bytes = URL_SAFE_NO_PAD.decode(segs[1])
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(segs[1]))
        .map_err(|e| format!("base64: {e}"))?;
    serde_json::from_slice::<Value>(&bytes).map_err(|e| format!("json: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_payload() {
        // header.payload( {"plan":"pro","exp":100} ).signature — signature omitted
        let payload = URL_SAFE_NO_PAD.encode(br#"{"plan":"pro","exp":100}"#);
        let tok = format!("header.{payload}.sig");
        let v = jwt_payload(&tok).unwrap();
        assert_eq!(v["plan"], "pro");
        assert_eq!(v["exp"], 100);
    }
}
```
Add `pub mod jwt;` to `lib.rs`.

- [ ] **Step 3: Run the test**

```bash
cd src-tauri && cargo test jwt::tests -- --nocapture
```
Expected: `1 passed`.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat(jwt): no-verify payload decode"
```

---

## Task 5: Claude provider (modeled on claude-meter, MIT)

**Files:**
- Create: `src-tauri/src/providers/claude.rs`, `src-tauri/tests/claude_fixture.json`
- Modify: `src-tauri/src/providers/mod.rs` (`pub mod claude;`)

**Interfaces:**
- Consumes: `secrets::{read_keychain_json, read_json_file, claude_token_path}`, `model::*`, `ProviderApi`.
- Produces: `ClaudeProvider { http: reqwest::Client }` implementing `ProviderApi`.

- [ ] **Step 1: Capture a fixture**

`src-tauri/tests/claude_fixture.json` (shape from claude-meter `models.rs`):
```json
{
  "five_hour": { "utilization": 0.235, "resets_at": "2026-06-18T20:00:00Z" },
  "seven_day": { "utilization": 0.412, "resets_at": "2026-06-24T17:00:00Z" },
  "seven_day_sonnet": { "utilization": 0.30, "resets_at": "2026-06-24T17:00:00Z" },
  "seven_day_opus": null,
  "seven_day_oauth_apps": null,
  "extra_usage": { "is_enabled": true, "monthly_limit": 100, "used_credits": 12.5,
                   "utilization": 0.125, "currency": "USD" }
}
```

- [ ] **Step 2: Write the failing test**

`src-tauri/src/providers/claude.rs`:
```rust
use crate::model::{LimitWindow, Provider, ServiceUsage};
use crate::providers::ProviderError;
use crate::secrets;
use async_trait::async_trait;
use serde::Deserialize;

const KEYCHAIN_SERVICE: &str = "Claude Code-credentials";
const API_BASE: &str = "https://api.anthropic.com";

#[derive(Deserialize)]
struct KeychainBlob {
    #[serde(rename = "claudeAiOauth")]
    oauth: OAuthCreds,
}
#[derive(Deserialize)]
struct OAuthCreds {
    #[serde(rename = "accessToken")] access_token: String,
    #[serde(rename = "expiresAt", default)] expires_at: i64,
    #[serde(rename = "subscriptionType", default)] subscription_type: Option<String>,
}

#[derive(Deserialize)]
struct Window { #[serde(default)] utilization: Option<f64>, resets_at: Option<chrono::DateTime<chrono::Utc>> }
#[derive(Deserialize)]
struct ExtraUsage { is_enabled: Option<bool>, used_credits: Option<f64>,
                    #[serde(default)] utilization: Option<f64> }
#[derive(Deserialize, Default)]
struct UsageResponse {
    #[serde(default)] five_hour: Option<Window>,
    #[serde(default)] seven_day: Option<Window>,
    #[serde(default)] seven_day_sonnet: Option<Window>,
    #[serde(default)] seven_day_opus: Option<Window>,
    #[serde(default)] seven_day_oauth_apps: Option<Window>,
    #[serde(default)] extra_usage: Option<ExtraUsage>,
}
#[derive(Deserialize)] struct Profile { account: OptEmail, organization: Org }
#[derive(Deserialize)] struct OptEmail { email: Option<String> }
#[derive(Deserialize)] struct Org { uuid: String }

pub struct ClaudeProvider { http: reqwest::Client }

impl ClaudeProvider {
    pub fn new() -> Self { Self { http: reqwest::Client::new() } }

    fn read_creds() -> Result<OAuthCreds, ProviderError> {
        // macOS: keychain; elsewhere: credentials file. Try keychain first.
        let blob = if cfg!(target_os = "macos") {
            match secrets::read_keychain_json(KEYCHAIN_SERVICE) {
                Ok(v) => Some(v),
                Err(_) => secrets::read_json_file(&secrets::claude_token_path()).ok(),
            }
        } else {
            secrets::read_json_file(&secrets::claude_token_path()).ok()
        };
        let blob = blob.ok_or_else(|| ProviderError::NotLoggedIn(
            "Claude Code not logged in (run `claude` then /login)".into()))?;
        let parsed: KeychainBlob = serde_json::from_value(blob)
            .map_err(|e| ProviderError::Parse(format!("claude creds: {e}")))?;
        Ok(parsed.oauth)
    }
}

fn window_to_limit(label: &str, w: &Window) -> LimitWindow {
    LimitWindow {
        label: label.into(),
        used_percent: w.utilization.map(|v| (v * 100.0) as f32),
        resets_at: w.resets_at.map(|d| d.timestamp()),
        used: None,
        limit: None,
    }
}

/// Pure normalization (unit-testable without network).
pub fn normalize(raw: &UsageResponse) -> Vec<LimitWindow> {
    let mut ws = Vec::new();
    if let Some(w) = &raw.five_hour { ws.push(window_to_limit("5시간", w)); }
    if let Some(w) = &raw.seven_day { ws.push(window_to_limit("7일", w)); }
    if let Some(w) = &raw.seven_day_sonnet { ws.push(window_to_limit("7일 (Sonnet)", w)); }
    if let Some(w) = &raw.seven_day_opus { ws.push(window_to_limit("7일 (Opus)", w)); }
    if let Some(w) = &raw.seven_day_oauth_apps { ws.push(window_to_limit("7일 (OAuth Apps)", w)); }
    if let Some(e) = &raw.extra_usage {
        if e.is_enabled.unwrap_or(false) {
            ws.push(LimitWindow { label: "Extra usage".into(),
                used_percent: e.utilization.map(|v| (v * 100.0) as f32),
                resets_at: None, used: e.used_credits, limit: None });
        }
    }
    ws
}

#[async_trait]
impl crate::providers::ProviderApi for ClaudeProvider {
    fn key(&self) -> Provider { Provider::Claude }

    async fn fetch(&self) -> Result<ServiceUsage, ProviderError> {
        let creds = Self::read_creds()?;
        let now_ms = chrono::Utc::now().timestamp_millis();
        if creds.expires_at > 0 && creds.expires_at < now_ms {
            return Err(ProviderError::Expired(
                "Claude Code token expired — run `claude` once to refresh".into()));
        }
        let token = creds.access_token.as_str();
        let usage: UsageResponse = crate::http::get_json(&self.http, token,
            &format!("{API_BASE}/api/oauth/usage"),
            &[("anthropic-version", "2023-06-01")]).await?;
        let profile: Profile = crate::http::get_json(&self.http, token,
            &format!("{API_BASE}/api/oauth/profile"),
            &[("anthropic-version", "2023-06-01")]).await?;
        Ok(ServiceUsage {
            provider: Provider::Claude, connected: true,
            plan: creds.subscription_type, account: profile.account.email,
            error: None, windows: normalize(&usage),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalize_fixture() {
        let raw = serde_json::from_str::<UsageResponse>(
            include_str!("../../tests/claude_fixture.json")).unwrap();
        let ws = normalize(&raw);
        let labels: Vec<&str> = ws.iter().map(|w| w.label.as_str()).collect();
        assert!(labels.contains(&"5시간"));
        assert!(labels.contains(&"7일"));
        assert!(labels.contains(&"Extra usage"));
        let five = ws.iter().find(|w| w.label == "5시간").unwrap();
        assert_eq!(five.used_percent, Some(23.5));
    }
}
```

- [ ] **Step 3: Create the `http` helper (Task-5 dependency)**

Create `src-tauri/src/http.rs` and add `pub mod http;` to `lib.rs`:
```rust
use crate::providers::ProviderError;
use reqwest::Client;

pub async fn get_json<T: serde::de::DeserializeOwned>(
    client: &Client, token: &str, url: &str, extra: &[(&str, &str)],
) -> Result<T, ProviderError> {
    let mut req = client.get(url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/json");
    for (k, v) in extra { req = req.header(*k, *v); }
    let resp = req.send().await.map_err(|e| ProviderError::Network(e.to_string()))?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(ProviderError::Status { status: status.as_u16(), body });
    }
    serde_json::from_str::<T>(&body).map_err(|e| ProviderError::Parse(format!("{url}: {e}")))
}
```
Add `reqwest = { version = "0.12", features = ["rustls-tls"], default-features = false }` to `Cargo.toml`.

- [ ] **Step 4: Run the test**

```bash
cd src-tauri && cargo test providers::claude::tests -- --nocapture
```
Expected: `1 passed`.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(claude): provider with usage/profile fetch + normalize"
```

---

## Task 6: Codex provider

**Files:**
- Create: `src-tauri/src/providers/codex.rs`, `src-tauri/tests/codex_auth_fixture.json`
- Modify: `src-tauri/src/providers/mod.rs` (`pub mod codex;`)

**Interfaces:**
- Consumes: `secrets::{read_json_file, codex_auth_path}`, `jwt::jwt_payload`, `http::get_json`, `model::*`.
- Produces: `CodexProvider` implementing `ProviderApi`.

- [ ] **Step 1: Fixtures**

`src-tauri/tests/codex_auth_fixture.json`:
```json
{ "auth_mode": "chatgpt",
  "tokens": { "id_token": "HEADER.eyJlbWFpbCI6Im1lQGV4LmNvbSIsInBsYW4iOiJwbHVzIn0.SIG",
              "access_token": "a.b.c", "refresh_token": "r", "account_id": "acct_1" } }
```
(payload decodes to `{"email":"me@ex.com","plan":"plus"}`)

`src-tauri/tests/codex_usage_fixture.json` (defensive shape — to be pinned to live):
```json
{ "usage": { "remaining_credits": 70.0, "used_credits": 30.0, "reset_at": "2026-06-18T20:00:00Z" } }
```

- [ ] **Step 2: Write the failing test + impl**

`src-tauri/src/providers/codex.rs`:
```rust
use crate::jwt::jwt_payload;
use crate::model::{LimitWindow, Provider, ServiceUsage};
use crate::providers::ProviderError;
use crate::secrets;
use async_trait::async_trait;
use serde::Deserialize;

const USAGE_URL: &str = "https://chatgpt.com/backend-api/codex/usage";

#[derive(Deserialize)]
struct AuthDotJson {
    #[serde(default)] auth_mode: Option<String>,
    tokens: Option<Tokens>,
}
#[derive(Deserialize)]
struct Tokens { access_token: String, #[serde(default)] id_token: Option<String> }

/// Permissive: the Codex usage shape is undocumented; grab known keys if present.
#[derive(Deserialize, Default)]
struct CodexUsage {
    #[serde(default)] remaining_credits: Option<f64>,
    #[serde(default)] used_credits: Option<f64>,
    #[serde(default)] reset_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub struct CodexProvider { http: reqwest::Client }
impl CodexProvider { pub fn new() -> Self { Self { http: reqwest::Client::new() } } }

fn read_tokens() -> Result<Tokens, ProviderError> {
    let v = secrets::read_json_file(&secrets::codex_auth_path())?;
    let a: AuthDotJson = serde_json::from_value(v)
        .map_err(|e| ProviderError::Parse(format!("codex auth: {e}")))?;
    match a.auth_mode.as_deref() {
        Some("chatgpt") => a.tokens.ok_or_else(|| ProviderError::NotLoggedIn(
            "Codex tokens missing (run `codex login`)".into())),
        _ => Err(ProviderError::NotLoggedIn(
            "Codex not in ChatGPT mode (subscription usage needs `codex login`)".into())),
    }
}

/// Pure: build a single window from a defensively-parsed Codex usage blob.
pub fn normalize(u: &CodexUsage) -> Vec<LimitWindow> {
    let used = u.used_credits;
    let limit = match (u.used_credits, u.remaining_credits) {
        (Some(used), Some(rem)) => Some(used + rem),
        _ => None,
    };
    let pct = match (used, limit) {
        (Some(used), Some(limit)) if limit > 0.0 => Some((used / limit * 100.0) as f32),
        _ => None,
    };
    vec![LimitWindow { label: "Codex credits".into(), used_percent: pct,
        resets_at: u.reset_at.map(|d| d.timestamp()), used, limit }]
}

#[async_trait]
impl crate::providers::ProviderApi for CodexProvider {
    fn key(&self) -> Provider { Provider::Codex }
    async fn fetch(&self) -> Result<ServiceUsage, ProviderError> {
        let t = read_tokens()?;
        let (plan, email) = match &t.id_token {
            Some(jwt) => match jwt_payload(jwt) {
                Ok(v) => (v.get("plan").and_then(|x| x.as_str()).map(String::from),
                          v.get("email").and_then(|x| x.as_str()).map(String::from)),
                Err(_) => (None, None),
            },
            None => (None, None),
        };
        let raw: serde_json::Value = crate::http::get_json(&self.http, &t.access_token,
            USAGE_URL, &[]).await?;
        // unwrap common wrapper if present, else use raw
        let inner = raw.get("usage").cloned().unwrap_or(raw);
        let u: CodexUsage = serde_json::from_value(inner)
            .map_err(|e| ProviderError::Parse(format!("codex usage: {e}")))?;
        Ok(ServiceUsage { provider: Provider::Codex, connected: true,
            plan, account: email, error: None, windows: normalize(&u) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_tokens_and_plan_from_fixture() {
        let v = serde_json::from_str(include_str!("../../tests/codex_auth_fixture.json")).unwrap();
        let a: AuthDotJson = serde_json::from_value(v).unwrap();
        assert_eq!(a.auth_mode.as_deref(), Some("chatgpt"));
        let plan = jwt_payload(a.tokens.as_ref().unwrap().id_token.as_ref().unwrap())
            .unwrap();
        assert_eq!(plan["email"], "me@ex.com");
    }
    #[test]
    fn normalize_computes_percent() {
        let u = CodexUsage { remaining_credits: Some(70.0), used_credits: Some(30.0), reset_at: None };
        let w = &normalize(&u)[0];
        assert_eq!(w.used_percent, Some(30.0));
        assert_eq!(w.limit, Some(100.0));
    }
}
```

- [ ] **Step 3: Run the test**

```bash
cd src-tauri && cargo test providers::codex::tests -- --nocapture
```
Expected: `2 passed`.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat(codex): provider with id_token decode + usage fetch"
```

---

## Task 7: Gemini provider (self-refresh + Code Assist quota)

**Files:**
- Create: `src-tauri/src/providers/gemini.rs`, `src-tauri/tests/gemini_oauth_fixture.json`, `src-tauri/tests/gemini_quota_fixture.json`
- Modify: `src-tauri/src/providers/mod.rs` (`pub mod gemini;`)

**Interfaces:**
- Consumes: `secrets::{read_json_file, gemini_creds_path}`, `model::*`, reqwest.
- Produces: `GeminiProvider` implementing `ProviderApi`.

- [ ] **Step 1: Fixtures**

`src-tauri/tests/gemini_oauth_fixture.json`:
```json
{ "access_token": "ya29.old", "refresh_token": "rt", "expiry_date": 1000,
  "client_id": "cid", "client_secret": "csec" }
```
`src-tauri/tests/gemini_quota_fixture.json`:
```json
{ "buckets": [
    { "modelId": "gemini-2.5-pro", "remainingFraction": 0.65, "remainingAmount": "650",
      "resetTime": "2026-06-19T00:00:00Z" },
    { "modelId": "gemini-2.5-flash-lite", "remainingFraction": 0.9993, "resetTime": "2026-06-19T00:00:00Z" }
] }
```

- [ ] **Step 2: Write the failing test + impl**

`src-tauri/src/providers/gemini.rs`:
```rust
use crate::model::{LimitWindow, Provider, ServiceUsage};
use crate::providers::ProviderError;
use crate::secrets;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const CODE_ASSIST_BASE: &str = "https://cloudcode-pa.googleapis.com/v1internal";

#[derive(Deserialize, Clone)]
struct OauthCreds {
    access_token: String,
    refresh_token: String,
    #[serde(default)] expiry_date: i64,
    #[serde(default)] client_id: Option<String>,
    #[serde(default)] client_secret: Option<String>,
}

#[derive(Deserialize)]
struct Bucket {
    #[serde(default)] model_id: Option<String>,
    #[serde(default)] remaining_fraction: Option<f64>,
    #[serde(default)] remaining_amount: Option<String>,
    #[serde(default)] reset_time: Option<chrono::DateTime<chrono::Utc>>,
}
#[derive(Deserialize, Default)]
struct QuotaResp { #[serde(default)] buckets: Vec<Bucket> }

pub struct GeminiProvider { http: reqwest::Client }
impl GeminiProvider { pub fn new() -> Self { Self { http: reqwest::Client::new() } } }

fn client_metadata(creds: &OauthCreds) -> Result<(String, String), ProviderError> {
    if let (Some(id), true) = (creds.client_id.clone(), creds.client_secret.is_some()) {
        return Ok((id, creds.client_secret.clone().unwrap()));
    }
    if let (Ok(id), Ok(sec)) = (std::env::var("GEMINI_OAUTH_CLIENT_ID"),
                                std::env::var("GEMINI_OAUTH_CLIENT_SECRET")) {
        return Ok((id, sec));
    }
    Err(ProviderError::NotLoggedIn(
        "Gemini client metadata not found (set GEMINI_OAUTH_CLIENT_ID/SECRET or run `gemini`)".into()))
}

/// Build the refresh request body (unit-testable, no network).
pub fn refresh_form(creds: &OauthCreds) -> Result<Vec<(String, String)>, ProviderError> {
    let (id, sec) = client_metadata(creds)?;
    Ok(vec![
        ("grant_type".into(), "refresh_token".into()),
        ("refresh_token".into(), creds.refresh_token.clone()),
        ("client_id".into(), id),
        ("client_secret".into(), sec),
    ])
}

/// Pure: bucket -> LimitWindow.
pub fn normalize(resp: &QuotaResp) -> Vec<LimitWindow> {
    resp.buckets.iter().filter_map(|b| {
        let label = b.model_id.clone().unwrap_or_else(|| "Gemini".into());
        let used_pct = b.remaining_fraction.map(|f| ((1.0 - f) * 100.0) as f32);
        let used = None;
        let limit = match (&b.remaining_amount, b.remaining_fraction) {
            (Some(amt), Some(f)) if f > 0.0 => amt.parse::<f64>().ok().map(|r| r / f),
            _ => None,
        };
        Some(LimitWindow { label, used_percent: used_pct,
            resets_at: b.reset_time.map(|d| d.timestamp()), used, limit })
    }).collect()
}

impl GeminiProvider {
    async fn fresh_token(&self) -> Result<String, ProviderError> {
        let v = secrets::read_json_file(&secrets::gemini_creds_path())?;
        let mut creds: OauthCreds = serde_json::from_value(v)
            .map_err(|e| ProviderError::Parse(format!("gemini creds: {e}")))?;
        let now_ms = chrono::Utc::now().timestamp_millis();
        if creds.expiry_date > 0 && now_ms < creds.expiry_date - 60_000 {
            return Ok(creds.access_token);
        }
        let form = refresh_form(&creds)?;
        let resp = self.http.post(TOKEN_URL).form(&form).send().await
            .map_err(|e| ProviderError::Network(e.to_string()))?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(ProviderError::Status { status: status.as_u16(), body });
        }
        let val: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| ProviderError::Parse(e.to_string()))?;
        creds.access_token = val["access_token"].as_str()
            .ok_or_else(|| ProviderError::Parse("no access_token in refresh response".into()))?
            .to_string();
        Ok(creds.access_token)
    }

    async fn post_internal(&self, token: &str, method: &str, payload: serde_json::Value)
        -> Result<serde_json::Value, ProviderError> {
        let url = format!("{CODE_ASSIST_BASE}:{method}");
        let resp = self.http.post(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .json(&payload).send().await
            .map_err(|e| ProviderError::Network(e.to_string()))?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(ProviderError::Status { status: status.as_u16(), body });
        }
        serde_json::from_str(&body).map_err(|e| ProviderError::Parse(e.to_string()))
    }
}

#[async_trait]
impl crate::providers::ProviderApi for GeminiProvider {
    fn key(&self) -> Provider { Provider::Gemini }
    async fn fetch(&self) -> Result<ServiceUsage, ProviderError> {
        let token = self.fresh_token().await?;
        let load = self.post_internal(&token, "loadCodeAssist", json!({
            "metadata": { "ideType": "IDE_UNSPECIFIED",
                          "platform": "PLATFORM_UNSPECIFIED", "pluginType": "GEMINI" }
        })).await?;
        let project = load.get("cloudaicompanionProject").cloned()
            .and_then(|v| v.as_str().map(String::from))
            .ok_or_else(|| ProviderError::Parse("no project id".into()))?;
        let quota_val = self.post_internal(&token, "retrieveUserQuota",
            json!({ "project": project })).await?;
        let quota: QuotaResp = serde_json::from_value(quota_val)
            .map_err(|e| ProviderError::Parse(e.to_string()))?;
        let tier = load.get("paidTier").and_then(|t| t.get("name")).and_then(|n| n.as_str());
        Ok(ServiceUsage { provider: Provider::Gemini, connected: true,
            plan: tier.map(String::from), account: None, error: None,
            windows: normalize(&quota) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalize_quota_fixture() {
        let q: QuotaResp = serde_json::from_str(
            include_str!("../../tests/gemini_quota_fixture.json")).unwrap();
        let ws = normalize(&q);
        assert_eq!(ws.len(), 2);
        let pro = ws.iter().find(|w| w.label == "gemini-2.5-pro").unwrap();
        assert_eq!(pro.used_percent, Some(35.0));
        assert_eq!(pro.limit, Some(1000.0)); // 650 / 0.65
    }
    #[test]
    fn refresh_form_uses_creds_client_metadata() {
        let c: OauthCreds = serde_json::from_str(
            include_str!("../../tests/gemini_oauth_fixture.json")).unwrap();
        let form = refresh_form(&c).unwrap();
        let map: std::collections::HashMap<String, String> = form.into_iter().collect();
        assert_eq!(map.get("client_id").unwrap(), "cid");
        assert_eq!(map.get("grant_type").unwrap(), "refresh_token");
    }
}
```

- [ ] **Step 3: Run the test**

```bash
cd src-tauri && cargo test providers::gemini::tests -- --nocapture
```
Expected: `2 passed`.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat(gemini): self-refresh + Code Assist quota provider"
```

---

## Task 8: Scheduler + config + Tauri commands + tray

**Files:**
- Create: `src-tauri/src/config.rs`, `src-tauri/src/scheduler.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/main.rs`
- Modify: `src-tauri/src/lib.rs`, `src-tauri/tauri.conf.json` (permissions), `Cargo.toml` (tauri plugins)

**Interfaces:**
- Consumes: all providers, `fetch_all`, `UsageSnapshot`.
- Produces: a running Tauri app with a tray, `get_usage`/`refresh_now` commands, and a background loop emitting `usage-updated`.

- [ ] **Step 1: Config**

`src-tauri/src/config.rs`:
```rust
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub poll_seconds: u64,
    pub enabled: [bool; 3], // [claude, codex, gemini]
}
impl Default for AppConfig {
    fn default() -> Self { Self { poll_seconds: 300, enabled: [true, true, true] } }
}
```

- [ ] **Step 2: Commands**

`src-tauri/src/commands.rs`:
```rust
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::RwLock;
use crate::config::AppConfig;
use crate::model::UsageSnapshot;
use crate::providers::{fetch_all, ProviderApi};

pub type SnapshotStore = Arc<RwLock<UsageSnapshot>>;
pub type ConfigStore = Arc<RwLock<AppConfig>>;

pub fn build_providers(cfg: &AppConfig) -> Vec<Box<dyn ProviderApi>> {
    let mut v: Vec<Box<dyn ProviderApi>> = Vec::new();
    if cfg.enabled[0] { v.push(Box::new(crate::providers::claude::ClaudeProvider::new())); }
    if cfg.enabled[1] { v.push(Box::new(crate::providers::codex::CodexProvider::new())); }
    if cfg.enabled[2] { v.push(Box::new(crate::providers::gemini::GeminiProvider::new())); }
    v
}

pub async fn refresh_once(app: &AppHandle, snap: &SnapshotStore) -> UsageSnapshot {
    let providers = build_providers(&*app.state::<ConfigStore>().inner().read().await);
    let services = fetch_all(&providers).await;
    let snapshot = UsageSnapshot { fetched_at: chrono::Utc::now().timestamp(), services };
    *snap.write().await = snapshot.clone();
    let _ = app.emit("usage-updated", &snapshot);
    snapshot
}

#[tauri::command]
pub async fn get_usage(snap: State<'_, SnapshotStore>) -> Result<UsageSnapshot, String> {
    Ok(snap.read().await.clone())
}

#[tauri::command]
pub async fn refresh_now(app: AppHandle, snap: State<'_, SnapshotStore>)
    -> Result<UsageSnapshot, String> {
    Ok(refresh_once(&app, &snap).await)
}

#[tauri::command]
pub async fn get_config(cfg: State<'_, ConfigStore>) -> Result<AppConfig, String> {
    Ok(cfg.read().await.clone())
}

#[tauri::command]
pub async fn set_config(app: AppHandle, cfg: State<'_, ConfigStore>, new: AppConfig)
    -> Result<(), String> {
    *cfg.write().await = new.clone();
    crate::scheduler::restart(&app, new.poll_seconds);
    Ok(())
}
```

- [ ] **Step 3: Scheduler**

`src-tauri/src/scheduler.rs`:
```rust
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use crate::commands::{refresh_once, ConfigStore, SnapshotStore};

static HANDLE_ID: AtomicU64 = AtomicU64::new(0);

pub fn start(app: AppHandle, poll_seconds: u64) {
    let app2 = app.clone();
    let id = HANDLE_ID.fetch_add(1, Ordering::SeqCst);
    tauri::async_runtime::spawn(async move {
        let snap = app2.state::<SnapshotStore>().inner().clone();
        refresh_once(&app2, &snap).await; // immediate first fetch
        loop {
            tokio::time::sleep(Duration::from_secs(poll_seconds.max(30))).await;
            if id + 1 != HANDLE_ID.load(Ordering::SeqCst) { break; } // superseded
            refresh_once(&app2, &snap).await;
        }
    });
}

pub fn restart(app: &AppHandle, poll_seconds: u64) {
    start(app.clone(), poll_seconds); // bumps HANDLE_ID, old loop exits
}
```

- [ ] **Step 4: main.rs — tray + window + wiring**

`src-tauri/src/main.rs` (Tauri 2 API; tray via `tauri::tray`):
```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod commands; mod config; mod http; mod jwt; mod model; mod providers; mod scheduler; mod secrets;

use std::sync::Arc;
use tauri::{Manager, Emitter};
use tokio::sync::RwLock;
use commands::{ConfigStore, SnapshotStore};
use config::AppConfig;
use model::UsageSnapshot;

fn main() {
    tauri::Builder::default()
        .manage(SnapshotStore::default_with_empty())
        .manage(ConfigStore::new())
        .setup(|app| {
            let handle = app.handle().clone();
            scheduler::start(handle.clone(), AppConfig::default().poll_seconds);
            Ok(())
        })
        .on_window_event(|win, ev| {
            // close-to-tray
            if let tauri::WindowEvent::CloseRequested { api, .. } = ev {
                win.hide().ok(); api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_usage, commands::refresh_now,
            commands::get_config, commands::set_config
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```
Add the tray in `setup` using `tauri::tray::TrayIconBuilder` + a menu (Open/Refresh/Quit), and update its title on `usage-updated` to the max percent. (Tray menu + title-update wiring is small; implement per current Tauri 2 tray API in `main.rs` `setup`.) If the exact tray API differs in the installed Tauri version, follow the compiler's guidance — the behavior is fixed: left-click toggles window, title shows `max_used_percent()`.

Helper additions for the `.manage(...)` stores:
```rust
impl SnapshotStore {
    fn default_with_empty() -> Self {
        Arc::new(RwLock::new(UsageSnapshot { fetched_at: 0, services: vec![] }))
    }
}
// ConfigStore: type alias = Arc<RwLock<AppConfig>>; wrap Arc::new(RwLock::new(default()))
```
Adjust the two `.manage(...)` calls to construct these via the helpers above.

- [ ] **Step 5: Permissions**

In `src-tauri/tauri.conf.json` set `"app.withGlobalTauri": true` is optional; ensure the default window `label: "main"` exists. Add to `src-tauri/capabilities/default.json` the core permissions (`core:default`, `core:event:default`, `core:window:default`) per the scaffold.

- [ ] **Step 6: Verify it builds and runs**

```bash
pnpm tauri dev
```
Expected: app launches, tray appears, after a moment the log shows fetch attempts (real or per-provider errors). Quit.

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "feat(app): scheduler, commands, tray, close-to-tray"
```

---

## Task 9: Frontend types + IPC + useUsage hook

**Files:**
- Create: `src/lib/types.ts`, `src/lib/ipc.ts`, `src/hooks/useUsage.ts`

**Interfaces:**
- Produces: typed `invoke`/`listen` wrappers and a `useUsage()` React hook returning `{snapshot, refresh, loading}`.

- [ ] **Step 1: Types mirroring Rust**

`src/lib/types.ts`:
```ts
export type Provider = "claude" | "codex" | "gemini";
export interface LimitWindow {
  label: string; used_percent: number | null;
  resets_at: number | null; used: number | null; limit: number | null;
}
export interface ServiceUsage {
  provider: Provider; connected: boolean;
  plan: string | null; account: string | null;
  error: string | null; windows: LimitWindow[];
}
export interface UsageSnapshot { fetched_at: number; services: ServiceUsage[]; }
export interface AppConfig { poll_seconds: number; enabled: [boolean, boolean, boolean]; }
```

- [ ] **Step 2: IPC wrappers**

`src/lib/ipc.ts`:
```ts
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AppConfig, UsageSnapshot } from "./types";

export const getUsage = () => invoke<UsageSnapshot>("get_usage");
export const refreshNow = () => invoke<UsageSnapshot>("refresh_now");
export const getConfig = () => invoke<AppConfig>("get_config");
export const setConfig = (cfg: AppConfig) => invoke<void>("set_config", { new: cfg });
export function onUsageUpdated(cb: (s: UsageSnapshot) => void): Promise<UnlistenFn> {
  return listen<UsageSnapshot>("usage-updated", e => cb(e.payload));
}
```

- [ ] **Step 3: Hook**

`src/hooks/useUsage.ts`:
```ts
import { useEffect, useState } from "react";
import type { UsageSnapshot } from "../lib/types";
import { getUsage, onUsageUpdated, refreshNow } from "../lib/ipc";

export function useUsage() {
  const [snapshot, setSnapshot] = useState<UsageSnapshot | null>(null);
  const [loading, setLoading] = useState(true);
  useEffect(() => {
    let un: (() => void) | undefined;
    (async () => {
      setSnapshot(await getUsage());
      setLoading(false);
      un = await onUsageUpdated(setSnapshot);
    })();
    return () => { un?.(); };
  }, []);
  const refresh = async () => { setLoading(true); setSnapshot(await refreshNow()); setLoading(false); };
  return { snapshot, loading, refresh };
}
```

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat(web): types, ipc wrappers, useUsage hook"
```

---

## Task 10: Dashboard UI (Header, Gauge, ServiceCard, Dashboard)

**Files:**
- Create: `src/components/Gauge.tsx`, `src/components/ServiceCard.tsx`, `src/components/Header.tsx`, `src/components/Dashboard.tsx`
- Modify: `src/App.tsx`, `src/index.css` (Tailwind layers), add shadcn `progress`, `card`, `badge` components.

**Interfaces:**
- Consumes: `useUsage`, `types`.
- Produces: the rendered dashboard.

- [ ] **Step 1: Add shadcn components**

```bash
npx shadcn@latest add card badge progress button tooltip -y
```

- [ ] **Step 2: Gauge**

`src/components/Gauge.tsx`:
```tsx
import { Progress } from "@/components/ui/progress";
export function Gauge({ label, percent, resetsAt }: {
  label: string; percent: number | null; resetsAt: number | null;
}) {
  const pct = percent ?? 0;
  const color = pct >= 70 ? "bg-red-500" : pct >= 40 ? "bg-amber-500" : "bg-emerald-500";
  const reset = resetsAt ? `resets ${new Date(resetsAt * 1000).toLocaleString()}` : "";
  return (
    <div className="space-y-1">
      <div className="flex justify-between text-sm">
        <span>{label}</span><span className="tabular-nums">{percent == null ? "?" : `${pct.toFixed(1)}%`}</span>
      </div>
      <Progress value={pct} className="[&>div]:bg-transparent">
        <div className={`h-full ${color} rounded-full`} style={{ width: `${pct}%` }} />
      </Progress>
      {reset && <div className="text-xs text-muted-foreground">{reset}</div>}
    </div>
  );
}
```
(If the installed shadcn `Progress` doesn't support children, render a plain `<div className="h-2 bg-muted rounded-full"><div className={...}/></div>` instead — behavior identical.)

- [ ] **Step 3: ServiceCard + Header + Dashboard**

`src/components/ServiceCard.tsx`:
```tsx
import type { ServiceUsage } from "../lib/types";
import { Gauge } from "./Gauge";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
const TITLES: Record<string,string> = { claude:"Claude", codex:"Codex", gemini:"Gemini" };
export function ServiceCard({ s }: { s: ServiceUsage }) {
  return (
    <Card>
      <CardHeader className="flex-row items-center justify-between">
        <span className="font-medium">{TITLES[s.provider]}</span>
        <div className="flex gap-2">
          {s.plan && <Badge variant="secondary">{s.plan}</Badge>}
          <Badge variant={s.connected ? "default" : "destructive"}>
            {s.connected ? "connected" : "offline"}</Badge>
        </div>
      </CardHeader>
      <CardContent className="space-y-3">
        {s.connected
          ? s.windows.map((w, i) => <Gauge key={i} {...w} />)
          : <p className="text-sm text-muted-foreground">{s.error ?? "not available"}</p>}
      </CardContent>
    </Card>
  );
}
```
`src/components/Header.tsx`:
```tsx
import { Button } from "@/components/ui/button";
export function Header({ fetchedAt, onRefresh, loading }: {
  fetchedAt: number; onRefresh: () => void; loading: boolean;
}) {
  return (
    <div className="flex items-center justify-between py-2">
      <span className="text-sm text-muted-foreground">
        {fetchedAt ? `Updated ${new Date(fetchedAt * 1000).toLocaleTimeString()}` : "—"}
      </span>
      <Button variant="outline" size="sm" onClick={onRefresh} disabled={loading}>Refresh</Button>
    </div>
  );
}
```
`src/components/Dashboard.tsx`:
```tsx
import { useUsage } from "../hooks/useUsage";
import { ServiceCard } from "./ServiceCard";
import { Header } from "./Header";
export function Dashboard() {
  const { snapshot, loading, refresh } = useUsage();
  return (
    <div className="min-h-screen p-4 max-w-3xl mx-auto">
      <h1 className="text-lg font-semibold mb-2">AI Usage Tracker</h1>
      <Header fetchedAt={snapshot?.fetched_at ?? 0} onRefresh={refresh} loading={loading} />
      <div className="grid gap-3 mt-2">
        {snapshot?.services.map(s => <ServiceCard key={s.provider} s={s} />) ?? <p>Loading…</p>}
      </div>
    </div>
  );
}
```
`src/App.tsx`:
```tsx
import { Dashboard } from "./components/Dashboard";
export default function App() { return <Dashboard />; }
```

- [ ] **Step 4: Verify end-to-end**

```bash
pnpm tauri dev
```
Expected: window shows three ServiceCards; connected providers render gauges; offline ones show the error hint. Tray shows the max %. Quit.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(web): dashboard UI (cards, gauges, header)"
```

---

## Task 11: Build verification + packaging

**Files:** none new; modifies `src-tauri/tauri.conf.json` (productName, identifier, icons).

- [ ] **Step 1: Set app identity**

In `tauri.conf.json`: `"productName": "AI Usage Tracker"`, `"identifier": "com.aiusage.tracker"`, provide icons under `src-tauri/icons/` (use `pnpm tauri icon path/to/icon.png`).

- [ ] **Step 2: Production build (macOS)**

```bash
pnpm tauri build
```
Expected: a `.app` (and `.dmg`) produced under `src-tauri/target/release/bundle/`. Launch it, confirm tray + dashboard work with real logged-in CLIs.

- [ ] **Step 3: Smoke-test each provider live**

With Claude Code / Codex CLI / Gemini CLI logged in: confirm each card shows real numbers; disconnect one CLI's token (rename its file) and confirm the card shows the actionable hint without breaking the others.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "chore: app identity, icons, build config"
```

---

## Self-Review

**1. Spec coverage**
- §6.1 Claude → Task 5 (keychain/file read, /api/oauth/usage + /profile, normalize windows, no-refresh + expiry hint). ✓
- §6.2 Codex → Task 6 (auth.json, id_token decode, /codex/usage, defensive normalize, read-only). ✓
- §6.3 Gemini → Task 7 (oauth_creds, self-refresh via oauth2.googleapis.com/token + client metadata, loadCodeAssist/retrieveUserQuota, bucket math). ✓
- §5 architecture (scheduler/IPC/tray/secrets boundary) → Tasks 2,3,8. ✓
- §7 data model → Task 2. ✓
- §8 frontend → Tasks 9,10. ✓
- §9 isolation/security → `fetch_all` error isolation (Task 2), tokens-in-Rust (http/providers). ✓
- §10 testing → TDD tasks 2–7 (parsing/normalize/refresh-form unit tests). ✓

**2. Placeholder scan** — no "TBD/TODO/add error handling". The Codex usage response shape is explicitly flagged as defensive (capture raw → unwrap wrapper → parse known keys), which is the honest treatment of an undocumented endpoint; pinning happens at the live smoke test (Task 11 Step 3). Tray title/menu wiring defers to the installed Tauri 2 tray API with the behavior fixed — acceptable since the API surface is small and compiler-guided.

**3. Type consistency** — `Provider`, `ServiceUsage`, `LimitWindow`, `UsageSnapshot` names match across model/providers/commands/frontend types. `ProviderApi::fetch`, `fetch_all`, `get_json` signatures consistent. `refresh_form`/`normalize` are `pub` and tested.

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-06-18-ai-usage-tracker.md`. Two execution options:

1. **Subagent-Driven (recommended)** — fresh subagent per task, review between tasks, fast iteration.
2. **Inline Execution** — execute tasks in this session using executing-plans, batch execution with checkpoints.

Which approach?

---

## Plan Amendments — advisor review (2026-06-18)

Code-level deltas (from advisor review + live verification) applied during implementation; see spec "Revision 2" for rationale:

- **T3 secrets:** macOS Claude via `/usr/bin/security` (account is the OS user, not empty); **drop the `keyring` crate**. Codex: no `auth_mode` gate; honor `$CODEX_HOME`.
- **T4 jwt:** tolerate padded base64url.
- **T2 `fetch_all`:** parallel via `futures::future::join_all`.
- **T5/T6/T7 http.rs:** `reqwest` features `["rustls-tls","json"]`; client `.timeout(15s).connect_timeout(5s)`. Codex (T6): add `User-Agent` + `chatgpt-account-id` header; capture the live shape now.
- **T8:** capabilities `core:tray:*` + window `allow-hide`; `RunEvent::ExitRequested => prevent_exit()`; `app.listen("usage-updated")` → `tray.set_title`.
- **New providers (Extend phase):** Copilot (gh token + billing API) and Cursor (state.vscdb + Connect-RPC), each with fixtures + unit tests.

### Execution waves (~5 dispatches)
W1 scaffold (done) → W2 core foundation (one agent: model+trait+errors+secrets+jwt+http; freeze shared files) → W3 three parallel provider agents (claude/codex/gemini) → W4 integration (scheduler+commands+tray+main+caps) → W5 frontend+build. Copilot/Cursor added as a parallel extension wave after W3.
