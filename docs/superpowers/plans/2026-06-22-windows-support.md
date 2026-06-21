# Windows Support (macOS + Windows) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make ai-usage-tracker correct and CI-buildable on Windows (in addition to macOS), with Linux explicitly dropped.

**Architecture:** The codebase is already largely Windows-correct (researched). The real unknown is a Windows *compile*, so Task 1 establishes that (keyring feature trim + CI matrix → macOS+Windows). Then Gemini becomes OAuth-only (its CLI auto-detect path is dead), Copilot's Windows keyring is annotated as hardware-unverified, and the README gets a macOS+Windows support matrix.

**Tech Stack:** Rust (Tauri 2, keyring v3, rusqlite bundled, reqwest), React/TS, GitHub Actions.

## Global Constraints

- Supported OSes = **macOS + Windows only** (Linux out of scope).
- No Windows test hardware. Verification tiers: **compile-verified via CI** > **static/research-correct (not compiled)** > runtime **`[NEEDS HARDWARE VERIFICATION]`**. Never claim live Windows behavior.
- Commit messages contain **NO** AI/Claude watermark.
- Branch per task → verify green (`cargo fmt`/`clippy`/`cargo test --lib` from `src-tauri/`; `pnpm exec tsc --noEmit`/`pnpm test` for FE) → fast-forward merge to `main` → delete branch.
- `main` is **local-only** — do NOT push without an explicit ask. (See the PUSH DECISION in Task 1 — the one exception, user-gated.)
- Tokens stay in Rust; `panic = "abort"` not set.
- Run `cargo` from `src-tauri/`; `pnpm` from the repo root.

## ⚠️ PUSH DECISION (resolve before Task 1's Windows verification)

CI has **never run on the current tree** (`main` is 37 commits ahead of `origin/main`). With no Windows hardware and no local macOS→`windows-msvc` cross-compile, the ONLY way to verify Windows even compiles is to **push a branch so GitHub Actions runs**. The user must choose:
- **(P) Push** a branch (e.g. `feat/windows-support`) → CI compiles + unit-tests on Windows = real verification.
- **(N) Don't push** → Windows stays **static/research-correct, not compiled**; only macOS local builds are checked.

Every task below is implementable under either choice; only Task 1's *Windows* verification step differs.

## File Structure

- `src-tauri/Cargo.toml` — keyring feature set (Task 1).
- `.github/workflows/ci.yml`, `.github/workflows/build-smoke.yml` — matrix → macOS+Windows (Task 1).
- `src-tauri/src/providers/gemini.rs` + `src-tauri/src/secrets.rs` — Gemini OAuth-only (Task 2).
- `src-tauri/src/providers/copilot.rs` — `[NEEDS HARDWARE VERIFICATION]` annotation (Task 3).
- `README.md` — macOS+Windows matrix, Linux-not-supported (Task 4).

---

### Task 1: Windows-compile foundation (keyring features + CI matrix)

**Files:**
- Modify: `src-tauri/Cargo.toml` (the `keyring` dependency line)
- Modify: `.github/workflows/ci.yml`, `.github/workflows/build-smoke.yml`

**Interfaces:** Produces a keyring dep that pulls no Linux-only (`secret-service`/`zbus`/dbus) crates, and a CI matrix limited to `macos-latest` + `windows-latest`.

- [ ] **Step 1: Trim keyring features.** In `src-tauri/Cargo.toml`, change:

```toml
keyring = { version = "3", features = ["apple-native", "windows-native", "sync-secret-service", "crypto-rust"] }
```
to:
```toml
keyring = { version = "3", features = ["apple-native", "windows-native"] }
```
(Drops `sync-secret-service` — Linux Secret Service, now out of scope — and `crypto-rust`, which only backed the secret-service session.)

- [ ] **Step 2: Verify it still compiles on macOS.** Run (from `src-tauri/`): `cargo build --lib`
Expected: builds. If it fails referencing a missing keyring backend/crypto symbol, re-add `crypto-rust` only (not `sync-secret-service`) and re-run. (copilot.rs uses `keyring::Entry::new("copilot-cli", …)` which is satisfied by `apple-native`/`windows-native`.)

- [ ] **Step 3: Confirm rusqlite stays bundled** (no edit expected). Verify the line in `src-tauri/Cargo.toml` reads `rusqlite = { version = "0.32", features = ["bundled"] }` — `bundled` compiles SQLite from source so the Windows build needs no system sqlite. If `bundled` is absent, add it.

- [ ] **Step 4: Set the CI rust matrix to macOS+Windows** in `.github/workflows/ci.yml`. In the `rust` job, change the matrix and move fmt/clippy off the (removed) Linux runner onto macOS, and delete the Linux-deps step:

```yaml
  rust:
    name: Rust (fmt + clippy + test)
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        os: [macos-latest, windows-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { components: rustfmt, clippy }
      - uses: Swatinem/rust-cache@v2
        with: { workspaces: src-tauri }
      - name: cargo fmt --check
        if: matrix.os == 'macos-latest'
        working-directory: src-tauri
        run: cargo fmt --all -- --check
      - name: cargo clippy
        if: matrix.os == 'macos-latest'
        working-directory: src-tauri
        run: cargo clippy --all-targets -- -D warnings
      - name: cargo test
        working-directory: src-tauri
        run: cargo test --lib --locked
```
(The `frontend` job stays on `ubuntu-latest` — it type-checks/tests OS-agnostic JS; ubuntu is just the cheap runner, not a support target. Leave it unchanged. Delete the `- name: Linux deps (Tauri)` step entirely from the `rust` job.)

- [ ] **Step 5: Set the build-smoke matrix to macOS+Windows** in `.github/workflows/build-smoke.yml`. Change `matrix.os` to `[macos-latest, windows-latest]` and delete the `- name: Linux deps (Tauri)` step (the whole `if: matrix.os == 'ubuntu-latest'` apt-get block).

- [ ] **Step 6: Commit on a branch** (`feat/windows-support`):

```bash
git checkout -b feat/windows-support
git add src-tauri/Cargo.toml src-tauri/Cargo.lock .github/workflows/ci.yml .github/workflows/build-smoke.yml
git commit -m "build(windows): trim keyring to apple+windows-native, CI matrix macOS+Windows (drop Linux)"
```
(Include `Cargo.lock` — trimming features changes the lockfile.)

- [ ] **Step 7: Windows verification — PUSH-GATED.**
  - **(P) If pushing:** `git push -u origin feat/windows-support`, then watch the run: `gh run watch $(gh run list --branch feat/windows-support --limit 1 --json databaseId -q '.[0].databaseId')` (or `gh run list --branch feat/windows-support`). Expected: the `windows-latest` legs of both `rust` and `build smoke` go green. If the Windows compile fails, fix the reported error (most likely a keyring/dep issue from Step 1) and re-push. **This is the compile-verified tier.**
  - **(N) If not pushing:** record in the commit/PR notes that Windows is **static-only, not compiled**. macOS `cargo build`/`clippy`/`test` (already green) is the only executed check. Do NOT mark Windows "verified".

- [ ] **Step 8: Merge** (after green on the chosen path): `git checkout main && git merge --ff-only feat/windows-support && git branch -d feat/windows-support`. (Under (P), the branch lives on origin too; that's fine.)

---

### Task 2: Gemini = OAuth only (remove dead CLI auto-detect)

**Files:**
- Modify: `src-tauri/src/providers/gemini.rs`
- Modify: `src-tauri/src/secrets.rs` (remove `gemini_creds_path` once unused)

**Interfaces:** The Gemini auto provider (`GeminiProvider::fetch`) no longer touches the filesystem; it returns `ProviderError::NotLoggedIn`. The stored OAuth path (`fetch_stored`/`refresh_stored`) is untouched.

- [ ] **Step 1: Write the failing test** in `gemini.rs`'s `#[cfg(test)] mod tests` (the auto path must NOT read any file and must report not-logged-in):

```rust
    #[tokio::test]
    async fn auto_fetch_is_oauth_only_not_logged_in() {
        // Gemini auto-detect of the CLI is unsupported; only in-app OAuth (stored)
        // is. The auto provider must report not_logged_in without touching disk.
        let out = GeminiProvider::new().fetch().await;
        match out {
            Err(crate::providers::ProviderError::NotLoggedIn(_)) => {}
            other => panic!("expected NotLoggedIn, got {other:?}"),
        }
    }
```

- [ ] **Step 2: Run it — expect FAIL** (current code reads the file / may not return NotLoggedIn deterministically). Run: `cargo test --lib providers::gemini::tests::auto_fetch_is_oauth_only -- --nocapture`

- [ ] **Step 3: Replace the auto `fetch()` body.** In `gemini.rs`, the `#[async_trait] impl ProviderApi for GeminiProvider` `async fn fetch(&self)` currently reads `secrets::gemini_creds_path()` and refreshes. Replace its entire body with:

```rust
    async fn fetch(&self) -> Result<ServiceUsage, ProviderError> {
        // Gemini is supported via in-app OAuth (Add account → stored) ONLY. The
        // Gemini CLI migrated off ~/.gemini/oauth_creds.json (deletes it) to an OS
        // keychain / encrypted file we can't reliably read, so CLI auto-detect is
        // dropped. A connected Gemini comes only from a stored OAuth account.
        Err(ProviderError::NotLoggedIn(
            "Gemini auto-detect disabled — add via Add account (OAuth)".into(),
        ))
    }
```
Keep `fn key(&self) -> Provider { Provider::Gemini }` as-is.

- [ ] **Step 4: Remove now-dead auto-only code, clippy-guided.** Run `cargo clippy --lib --all-targets` from `src-tauri/`. It will flag the helpers that only the old auto `fetch()` used — typically `write_back_creds` (writes to `gemini_creds_path`) and any auto-only refresh wrapper. Delete each flagged dead item. Then in `secrets.rs`, delete `pub fn gemini_creds_path()` (its only callers were the auto read + `write_back_creds`). KEEP everything the stored path uses (`fetch_stored`, `fetch_with`, `refresh_gemini_token`, `OauthCreds`, `refresh_stored`) — if clippy flags one of those, it means it was auto-only and is safe to remove; if it does NOT flag it, leave it.

- [ ] **Step 5: Run checks.** Run: `cargo fmt && cargo clippy --lib --all-targets` (clean) && `cargo test --lib` (the new test passes; no regressions; if an old test exercised the Gemini auto file-read, delete or rewrite it to the not-logged-in expectation).

- [ ] **Step 6: Commit.**

```bash
git checkout -b feat/gemini-oauth-only
git add -A && git commit -m "feat(gemini): OAuth-only — drop dead CLI auto-detect (file path migrated away)"
git checkout main && git merge --ff-only feat/gemini-oauth-only && git branch -d feat/gemini-oauth-only
```

---

### Task 3: Copilot Windows annotation + credential-path comments

**Files:**
- Modify: `src-tauri/src/providers/copilot.rs`

**Interfaces:** No behavior change — documentation/comments only.

- [ ] **Step 1: Annotate the keyring account-name heuristic.** In `copilot.rs`, find `read_copilot_keyring` (the `#[cfg(not(target_os = "macos"))]` fn that tries `keyring::Entry::new("copilot-cli", acct)` over candidate accounts like `["github.com", "copilot-cli", $USER]`). Add a comment directly above the candidate-account list:

```rust
    // [NEEDS HARDWARE VERIFICATION] On Windows the credential lives in Windows
    // Credential Manager under service "copilot-cli", but the ACCOUNT name the
    // Copilot CLI uses is embedded in its native addon and undocumented. These
    // candidates are a best-effort guess; the `~/.copilot/config.json` file
    // fallback covers the miss. Verify on a real Windows box with `cmdkey /list`.
```

- [ ] **Step 2: Run checks.** Run: `cargo fmt && cargo clippy --lib --all-targets` (clean) && `cargo test --lib` (no change).

- [ ] **Step 3: Commit.**

```bash
git checkout -b docs/copilot-windows-note
git add src-tauri/src/providers/copilot.rs
git commit -m "docs(copilot): flag Windows keyring account name as needs-hardware-verification"
git checkout main && git merge --ff-only docs/copilot-windows-note && git branch -d docs/copilot-windows-note
```

---

### Task 4: README — macOS + Windows support matrix

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Update the Providers table's credential-source notes + add a platform matrix.** After the existing `## Providers` table, add a new subsection:

```markdown
### Platform support (macOS + Windows)

Linux is **not a supported target**. Credential auto-detection per OS:

| Provider | macOS | Windows | Notes |
|---|---|---|---|
| **Claude** | Keychain `Claude Code-credentials` → file | `%USERPROFILE%\.claude\.credentials.json` (+ `%CLAUDE_CONFIG_DIR%`) | no Credential Manager off-macOS |
| **Codex** | `~/.codex/auth.json` (+ `CODEX_HOME`) | `%USERPROFILE%\.codex\auth.json` (+ `%CODEX_HOME%`) | |
| **Gemini** | **OAuth only** (Add account) | **OAuth only** (Add account) | CLI auto-detect dropped — the CLI encrypts/migrates its token store |
| **Copilot** | Keychain `copilot-cli` → file | Credential Manager `copilot-cli` → `%USERPROFILE%\.copilot\config.json` | Windows keyring account name `[NEEDS HARDWARE VERIFICATION]` — verify with `cmdkey /list` |
| **Cursor** | `~/Library/.../Cursor/.../state.vscdb` | `%APPDATA%\Cursor\User\globalStorage\state.vscdb` | |
| **z.ai** | `ZAI_API_KEY` env / pasted key | `ZAI_API_KEY` env / pasted key | |

Windows support is CI-compile-verified; live runtime (tray, real credential reads, anchor send) is not yet hardware-tested.
```

- [ ] **Step 2: Fix the Gemini row in the main Providers table** — change its "Credential source" cell to note OAuth-only (remove the `~/.gemini/oauth_creds.json` auto-detect claim; keep the in-app browser OAuth description).

- [ ] **Step 3: Update the build/CI + status sections** — in the `## Develop`/CI prose, change "macOS + Windows + Linux matrix" to "macOS + Windows matrix"; in `## Status / limitations`, replace the "Linux/Windows are CI-built…" line with: "macOS is the primary dev/test platform; **Windows is CI-compile-verified but not yet hardware-tested**; **Linux is not supported.**"

- [ ] **Step 4: Verify markdown + commit.** Run `git diff README.md` (eyeball table renders), then:

```bash
git checkout -b docs/readme-platform-matrix
git add README.md
git commit -m "docs(readme): macOS+Windows support matrix; Gemini OAuth-only; Linux unsupported"
git checkout main && git merge --ff-only docs/readme-platform-matrix && git branch -d docs/readme-platform-matrix
```

---

## Self-Review

**Spec coverage:** keyring trim + CI matrix (Task 1) ✓; rusqlite bundled confirm (Task 1.3) ✓; Gemini OAuth-only + dead-code removal (Task 2) ✓; Copilot NEEDS-HARDWARE (Task 3) ✓; README matrix + Gemini note + Linux-unsupported + Copilot verify cmd (Task 4) ✓; push-decision/verification tiers (Task 1.7 + Global) ✓; "no code change for Claude/Codex/Cursor/z.ai/lib.rs" — covered by confirming via CI (Task 1.7) + documenting (Task 4), no task needed. Build-smoke Windows artifact check is Task 1.5/1.7.

**Placeholder scan:** The only non-literal step is Task 2.4 (clippy-guided dead-code removal) — this is deterministic (clippy names the exact unused items) and necessary because the dead set depends on what the stored path shares; not a vague placeholder. The push-gate (Task 1.7) is a real user decision, stated explicitly, not a TODO.

**Type consistency:** `GeminiProvider::fetch` → `Err(ProviderError::NotLoggedIn(..))`; `keyring` features `["apple-native","windows-native"]`; CI matrix `[macos-latest, windows-latest]`; fmt/clippy gated to `macos-latest`. Consistent across tasks.
