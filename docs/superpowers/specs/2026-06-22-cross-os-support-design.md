# Windows + Linux Support — Design Spec

**Date:** 2026-06-22
**Status:** Approved design (user resolved the Gemini decision); ready for implementation plan.

## Goal

Make ai-usage-tracker correct and buildable on **Windows and Linux** (it is macOS-verified today). Scope is the **core**: per-OS credential paths, `cfg` correctness, 3-OS CI builds, and unsigned packages. Tray / window / the anchor feature are already cross-platform (Tauri tray-icon + reqwest HTTP) and need confirmation, not rework. **Code signing / notarization is out of scope.**

## Hard constraint: no Win/Linux test hardware

The implementer (and the maintainer) cannot run the app on real Windows or Linux. Therefore "support" is verified by: **(1) 3-OS CI build green, (2) cross-OS-safe `cargo test` + `vitest`, (3) static review against the researched credential locations.** Anything that cannot be confirmed without a real machine is marked **`[NEEDS HARDWARE VERIFICATION]`** in code comments + the README, and the user verifies it live later. Do NOT claim a path "works" — claim it "compiles + matches researched behavior".

## Research result: most of the codebase is already cross-OS-correct

A web-research pass (per-provider, per-OS, cited) cross-checked the current code. Findings:

| Provider | Win/Linux credential source | Current code | Action |
|---|---|---|---|
| **Claude** | file `~/.claude/.credentials.json` (+ `$CLAUDE_CONFIG_DIR`); **no** keychain off-macOS | ✅ correct (`#[cfg(not(macos))]` reads the file; `dirs::home_dir()` = `%USERPROFILE%` on Win) | none (confirm + document) |
| **Codex** | `~/.codex/auth.json` (+ `$CODEX_HOME`) | ✅ correct cross-OS | none. (Keyring-store mode is ignored; LOW risk — `auto` mode always writes `auth.json` as fallback. Leave.) |
| **Cursor** | Linux `~/.config/Cursor/.../state.vscdb`; Win `%APPDATA%\Cursor\...` (`dirs::data_dir()` = Roaming) | ✅ correct (`#[cfg(target_os)]` arms) | none (confirm + document) |
| **z.ai** | `ZAI_API_KEY` env / pasted key | ✅ cross-OS | none |
| **Copilot** | keyring service `copilot-cli` (Win Credential Manager / Linux Secret Service) + file `~/.copilot/config.json` (+ `$COPILOT_HOME`) | ⚠️ service name confirmed; **keyring account name is a heuristic** (`["github.com","copilot-cli",$USER]`) — not publicly documented | keep best-effort (keyring + file fallback); mark `[NEEDS HARDWARE VERIFICATION]`; document the verify commands |
| **Gemini** | modern CLI migrated **off** `~/.gemini/oauth_creds.json` (deleted) → OS keychain `gemini-cli-oauth` / encrypted file | 🔴 reads the dead legacy file | **OAuth-only** (see decision) |

Also confirmed correct: `keyring` v3 features (`windows-native`, `sync-secret-service`) cover WCM + Secret Service; `dirs::{home_dir,config_dir,data_dir}` resolve correctly on Windows; `lib.rs` `set_dock_visible` is already a no-op off-macOS and the tray is the cross-platform Tauri `MenuBuilder` (no change). CI already builds a 3-OS matrix.

## Decisions

- **Gemini = OAuth only** (user decision). Remove the CLI file auto-detect (the legacy `oauth_creds.json` path is migrated-away/deleted, and the modern keychain/encrypted-file formats can't be confirmed without hardware). Gemini is supported solely via the in-app **browser OAuth** flow → stored account, which is pure HTTP and already cross-OS. The Gemini auto-detect provider no longer reads any file; it reports `not_logged_in` with a hint to add via OAuth (so a connected stored Gemini is the only "online" Gemini). `gemini_creds_path()` + the auto file-read become dead and are removed.
- **Copilot stays best-effort** off-macOS (keyring + file fallback) with explicit `[NEEDS HARDWARE VERIFICATION]` on the keyring account name.
- **Everything else: no code change** — confirm via CI + document.

## Components / changes

1. **`src-tauri/src/providers/gemini.rs`** — drop the auto-detect file read; the auto `fetch()` returns `ProviderError::NotLoggedIn` (hint: "add Gemini via Add account → OAuth"). Keep `fetch_stored` (OAuth/refresh) untouched. Remove now-dead `secrets::gemini_creds_path` if unused elsewhere.
2. **`src-tauri/src/providers/copilot.rs`** — add a `[NEEDS HARDWARE VERIFICATION]` comment on the keyring account-name heuristic; no behavior change.
3. **Docs/comments** — annotate the per-OS credential paths with confidence/`[NEEDS HARDWARE VERIFICATION]` where applicable.
4. **CI** (`.github/workflows/`) — confirm the 3-OS matrix builds `cargo test --lib` + the debug `tauri build` (build-smoke) green on macOS + Windows + Linux. Add the Linux system deps the build needs if missing (e.g. `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `libsecret-1-dev` for keyring). Verify Windows + Linux bundle targets produce artifacts (`targets: "all"` → Linux deb/rpm/AppImage, Windows nsis/msi).
5. **README** — a **cross-OS support matrix** (per provider: where its credential lives on each OS + confidence), a "Gemini = OAuth (Add account) only" note, the Copilot `[NEEDS HARDWARE VERIFICATION]` + verify commands (`secret-tool search service copilot-cli` on Linux, `cmdkey /list` on Windows), and an updated platform-status line.

## Error handling

No new failure modes. Gemini auto simply reports `not_logged_in` (already an isolated, non-fatal card state). Copilot keyring miss falls through to the file as today. Provider isolation (`fetch_all`) unchanged.

## Testing / verification

- **Unit tests** are already cross-OS-safe (pure parsing + path helpers; no live network in the suite). Add/adjust a test for the Gemini auto path now returning `not_logged_in` without touching the filesystem. `cargo test --lib` + `vitest` stay green.
- **CI**: the 3-OS matrix is the primary cross-OS gate (compile + unit tests on macOS/Windows/Linux; debug `tauri build` smoke on all three).
- **Live runtime** (tray rendering, real credential reads, the anchor send) on Windows/Linux is **deferred to the user** and tracked by the `[NEEDS HARDWARE VERIFICATION]` markers.

## Scope check

Single cohesive spec → single implementation plan. Not decomposed.

## Global constraints (every task)

- Commit messages contain **NO** AI/Claude watermark.
- Branch per task → verify green (`cargo fmt`/`clippy`/`cargo test --lib`; `tsc`/`vitest` for FE) → fast-forward merge to `main` → delete branch.
- `main` is **local-only** — do not push without an explicit ask.
- Tokens stay in Rust; only masked metadata + usage snapshots cross IPC (P0). `panic = "abort"` not set.
- Replies in Korean.
- Verification is CI + static/research; do not claim live Win/Linux behavior — mark `[NEEDS HARDWARE VERIFICATION]`.
