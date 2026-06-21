# Windows Support (macOS + Windows) — Design Spec

**Date:** 2026-06-22
**Status:** Approved design (Gemini = OAuth-only; **Linux dropped — macOS + Windows only**); ready for implementation plan.

## Goal

Make ai-usage-tracker correct and buildable on **Windows** (it is macOS-verified today). **Linux is explicitly out of scope** — supported targets are **macOS + Windows only**. Scope is the **core**: Windows credential paths, `cfg` correctness, a macOS+Windows CI build, and unsigned packages. Tray / window / the anchor feature are already cross-platform (Tauri tray-icon + reqwest HTTP) and need confirmation, not rework. **Code signing / notarization is out of scope.**

## Hard constraint: no Windows test hardware

The implementer and the maintainer cannot run the app on real Windows. "Support" is therefore verified by: **(1) macOS+Windows CI build green, (2) cross-OS-safe `cargo test` + `vitest`, (3) static review against researched Windows credential locations.** Anything not confirmable without a real machine is marked **`[NEEDS HARDWARE VERIFICATION]`** in code comments + the README, and the user verifies it live later. Do NOT claim a path "works" on Windows — claim it "compiles + matches researched behavior".

## Research result: the codebase is already largely Windows-correct

A web-research pass (per-provider, cited) cross-checked the current code against where each CLI stores credentials on Windows:

| Provider | Windows credential source | Current code | Action |
|---|---|---|---|
| **Claude** | file `%USERPROFILE%\.claude\.credentials.json` (+ `%CLAUDE_CONFIG_DIR%`); **no** Credential Manager | ✅ correct — `#[cfg(not(macos))]` reads the file; `dirs::home_dir()` = `%USERPROFILE%` | none (confirm + document) |
| **Codex** | `%USERPROFILE%\.codex\auth.json` (+ `%CODEX_HOME%`) | ✅ correct | none. (Keyring-store mode ignored; LOW — `auto` mode always writes `auth.json` as fallback. Leave.) |
| **Cursor** | `%APPDATA%\Cursor\User\globalStorage\state.vscdb` (Roaming) | ✅ correct — `#[cfg(target_os="windows")]` uses `dirs::data_dir()` = `%APPDATA%` | none (confirm + document) |
| **z.ai** | `ZAI_API_KEY` env / pasted key | ✅ cross-OS | none |
| **Copilot** | keyring service `copilot-cli` (Windows Credential Manager) + file `%USERPROFILE%\.copilot\config.json` (+ `%COPILOT_HOME%`) | ⚠️ service name confirmed; **keyring account name is a heuristic** (`["github.com","copilot-cli",$USER]`) — not publicly documented | keep best-effort (keyring + file fallback); mark `[NEEDS HARDWARE VERIFICATION]`; document the verify command |
| **Gemini** | modern CLI migrated **off** `~/.gemini/oauth_creds.json` (deleted) → OS keychain `gemini-cli-oauth` / encrypted file | 🔴 reads the dead legacy file | **OAuth-only** (see decision) |

Also confirmed correct for Windows: the `keyring` v3 `windows-native` feature covers Windows Credential Manager; `dirs::{home_dir,config_dir,data_dir}` resolve correctly on Windows (`%USERPROFILE%` / `%APPDATA%` Roaming / `%LOCALAPPDATA%`); `lib.rs` `set_dock_visible` is already a no-op off-macOS and the tray is the cross-platform Tauri `MenuBuilder` (no change). CI already builds a multi-OS matrix.

## Advisor review: the verification gate may not exist + build risks

The advisor flagged that **"CI build green" is the spec's primary gate, but CI has almost certainly never run on this code.** `main` is **37 commits ahead of `origin/main`** (everything this session is unpushed). The 3-OS workflow *files* exist; they have not *executed* on the current tree. So today "Windows support" = "compiles in theory; never once compiled in practice."

Combined with "no Windows hardware" and "can't cross-compile macOS → `windows-msvc` locally (needs MSVC + WebView2)", there is exactly one way to even *compile-check* Windows: **push a branch so GitHub Actions runs.** This collides with the standing "local-only, don't push" rule.

**→ REQUIRED USER DECISION before the plan's verification can be real (the only honest choice):**
- **(P) Push a branch** (e.g. `feat/windows-support`) so CI actually compiles + unit-tests on Windows. This is the only way to verify Windows even builds. (Does not require pushing `main`.)
- **(N) Don't push** — accept that Windows is **static-review-only, not even compiled**. The plan still makes the code correct-by-research, but no Windows build is proven.

Track three honesty tiers, not two: **compile-verified via CI** > **static/research-correct (not compiled)** > **runtime `[NEEDS HARDWARE VERIFICATION]`**.

**Build risks (compile-time, not path-time) — first task must establish a Windows compile:**
- **keyring features:** drop **`sync-secret-service`** (Linux Secret Service only — that platform was just cut; it drags in `secret-service`/`zbus`/dbus which is exactly the kind of non-target-gated optional dep that breaks a Windows build). Re-derive to `["apple-native", "windows-native"]` and check whether **`crypto-rust`** is still needed (it backed the secret-service session). Each change gets its own compile check.
- **Cursor SQLite:** `rusqlite = { version = "0.32", features = ["bundled"] }` — **confirmed bundled**, so it compiles its own SQLite on Windows with no system dep. ✅ No action beyond confirming the CI Windows build links it.

## Decisions

- **Gemini = OAuth only** (user decision). Remove the CLI file auto-detect (the legacy `oauth_creds.json` path is migrated-away/deleted, and the modern keychain/encrypted-file formats can't be confirmed without hardware). Gemini is supported solely via the in-app **browser OAuth** flow → stored account, which is pure HTTP and already cross-OS. The Gemini auto-detect `fetch()` no longer reads any file; it returns `not_logged_in` (hint: add via OAuth) so a connected stored Gemini is the only "online" Gemini. `secrets::gemini_creds_path()` + the auto file-read become dead and are removed.
- **Copilot stays best-effort** on Windows (Credential Manager keyring + `config.json` file fallback) with an explicit `[NEEDS HARDWARE VERIFICATION]` on the keyring account name.
- **Linux dropped** — remove the Linux runner from CI and don't document/package Linux. The existing Linux `#[cfg(target_os="linux")]` arms (e.g. Cursor's path, and `#[cfg(not(macos))]` arms that also compile on Linux) are **left in place, dormant** — removing them is pure churn and the `not(macos)` arms are needed for Windows anyway. We simply stop targeting/testing Linux.
- **Everything else: no code change** — confirm via CI + document.

## Components / changes

1. **`src-tauri/src/providers/gemini.rs`** — drop the auto-detect file read; the auto `fetch()` returns `ProviderError::NotLoggedIn` (hint to add via Add account → OAuth). Keep `fetch_stored` (OAuth/refresh) untouched. Remove now-dead `secrets::gemini_creds_path` if unused elsewhere; update any test that exercised the Gemini auto file-read.
2. **`src-tauri/src/providers/copilot.rs`** — add a `[NEEDS HARDWARE VERIFICATION]` comment on the keyring account-name heuristic (Windows Credential Manager); no behavior change.
3. **Docs/comments** — annotate the Windows credential paths with confidence / `[NEEDS HARDWARE VERIFICATION]` where applicable.
4. **CI** (`.github/workflows/`) — set the cross-OS matrix to **macOS + Windows only** (remove the Linux runner from both the `cargo test` matrix and the `build-smoke` `tauri build`). No Linux system deps needed. Confirm the Windows runner produces bundle artifacts (`targets: "all"` → nsis/msi) and macOS produces app/dmg. Update `cargo fmt --check`/`clippy` host if it was pinned to Linux (move to macOS or Windows runner).
5. **README** — a **macOS + Windows support matrix** (per provider: where its credential lives on each of the two OSes + confidence), a "Gemini = OAuth (Add account) only" note, the Copilot `[NEEDS HARDWARE VERIFICATION]` + the verify command (`cmdkey /list` on Windows), an updated platform-status line that states **Linux is not supported**, and removal of Linux mentions from the build/CI section.
6. **`src-tauri/Cargo.toml` — Windows-compile foundation (the plan sequences this FIRST).** keyring features → `["apple-native", "windows-native"]` (drop `sync-secret-service`; drop `crypto-rust` if a compile check shows it unneeded). Confirm `rusqlite` stays `bundled`. This is the riskiest unknown (a Windows *compile*, not a path), so it leads the plan; its verification depends on the push decision (P → CI compiles on Windows; N → only macOS `cargo build` confirms non-Windows still builds, Windows stays static-only).

## Error handling

No new failure modes. Gemini auto reports `not_logged_in` (an isolated, non-fatal card state). Copilot keyring miss falls through to the file as today. Provider isolation (`fetch_all`) unchanged.

## Testing / verification

- **Unit tests** are already cross-OS-safe (pure parsing + path helpers; no live network in the suite). Add/adjust a test for the Gemini auto path now returning `not_logged_in` without touching the filesystem. `cargo test --lib` + `vitest` stay green.
- **CI**: the **macOS + Windows** matrix is the primary gate (compile + unit tests on both; debug `tauri build` smoke on both).
- **Live runtime** (tray rendering, real credential reads, the anchor send) on **Windows** is **deferred to the user** and tracked by the `[NEEDS HARDWARE VERIFICATION]` markers.

## Scope check

Single cohesive spec → single implementation plan. Not decomposed.

## Global constraints (every task)

- Commit messages contain **NO** AI/Claude watermark.
- Branch per task → verify green (`cargo fmt`/`clippy`/`cargo test --lib`; `tsc`/`vitest` for FE) → fast-forward merge to `main` → delete branch.
- `main` is **local-only** — do not push without an explicit ask.
- Tokens stay in Rust; only masked metadata + usage snapshots cross IPC (P0). `panic = "abort"` not set.
- Replies in Korean.
- Supported OSes = **macOS + Windows only** (Linux out of scope). Verification is CI + static/research; do not claim live Windows behavior — mark `[NEEDS HARDWARE VERIFICATION]`.
