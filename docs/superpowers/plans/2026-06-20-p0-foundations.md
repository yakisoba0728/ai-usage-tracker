# P0 Foundations & Quick Wins — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the low-risk, high-ROI P0 items from the 2026-06-20 investigation — CI on 3 OSes, a hardened webview, an optimized release profile, lazy-loaded windows, and an honest "last updated" footer — without touching the large redesign/migration work.

**Architecture:** Five independent task groups, each shippable on its own: (1) Tauri webview CSP, (2) Cargo release profile, (3) `React.lazy` window split, (4) honest footer timestamp, (5) GitHub Actions CI (lint+test+build-smoke, **no signing**). None alter the provider/IPC behavior or the token-never-crosses-IPC invariant.

**Tech Stack:** Tauri 2, Rust (edition 2021), React 19, Vite 7, Tailwind v4, pnpm 9, Vitest, GitHub Actions.

## Global Constraints

- **No code signing / notarization in this plan.** Signing identity is deferred (user decision 2026-06-20). CI builds run `tauri build --debug --no-bundle` only.
- **Preserve the per-provider isolation invariant.** Do NOT set `panic = "abort"` in the release profile — a panic in one provider's fetch must not abort the whole process (would break `fetch_all` isolation).
- **Preserve the P0 token invariant.** No task here serializes tokens to the frontend; keep it that way.
- **macOS is the only locally-verifiable OS.** Windows/Linux are verified by CI only.
- **Exact tool versions:** Node 24, pnpm 9, Rust stable. Cargo workspace lives in `src-tauri/`. Use `pnpm exec tauri …` directly (the `tauri` npm script is `env -u CI tauri`, which interferes with CI detection).
- Run frontend commands from repo root; Rust commands from `src-tauri/`.

---

### Task 1: Harden the webview with a Content-Security-Policy

**Files:**
- Modify: `src-tauri/tauri.conf.json:22-24`

**Interfaces:**
- Consumes: nothing.
- Produces: a non-null `app.security.csp` string. No code depends on it.

- [ ] **Step 1: Replace the null CSP with a restrictive policy**

In `src-tauri/tauri.conf.json`, change the `security` block from:

```json
    "security": {
      "csp": null
    }
```

to:

```json
    "security": {
      "csp": "default-src 'self'; img-src 'self' asset: data:; font-src 'self' data:; style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src 'self' ipc: http://ipc.localhost"
    }
```

Rationale per directive: `style-src 'unsafe-inline'` is required because Tailwind-injected styles and a few `style={{…}}` props are inline; `connect-src` must include `ipc: http://ipc.localhost` or Tauri 2 IPC `invoke`/events break; `img-src asset:` covers the Tauri asset protocol; `script-src 'self'` (no `unsafe-eval`/`unsafe-inline`).

- [ ] **Step 2: Verify the app still loads and IPC works under the CSP**

Run: `pnpm tauri dev`
Expected: the dashboard renders, usage cards populate (proves `invoke`/events pass `connect-src`), and the devtools Console shows **no** `Content Security Policy` violation errors. If a violation appears for a resource you legitimately need (e.g. a font data URI), widen only that one directive and re-run.

- [ ] **Step 3: Type/format check**

Run: `cd src-tauri && cargo fmt --check && cd ..`
Expected: PASS (JSON isn't formatted by cargo, but this confirms nothing else regressed if you touched Rust). Also confirm the JSON is valid: `node -e "JSON.parse(require('fs').readFileSync('src-tauri/tauri.conf.json'))"` → no output = valid.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/tauri.conf.json
git commit -m "security: add restrictive webview CSP (defense-in-depth)"
```

---

### Task 2: Add an optimized Cargo release profile

**Files:**
- Modify: `src-tauri/Cargo.toml` (append a new section after line 34)

**Interfaces:**
- Consumes: nothing.
- Produces: a smaller/faster release binary. No source code depends on this.

- [ ] **Step 1: Append the release profile**

Add to the end of `src-tauri/Cargo.toml`:

```toml

[profile.release]
opt-level = "z"      # optimize for size (menu-bar utility, not CPU-bound)
lto = true           # link-time optimization
codegen-units = 1    # better optimization at the cost of compile time
strip = true         # strip symbols from the binary
# NOTE: deliberately NOT setting `panic = "abort"`. The app relies on
# unwinding so a panic in one provider fetch does not abort the whole
# process (preserves the per-provider isolation invariant in fetch_all).
```

- [ ] **Step 2: Verify a release build still compiles**

Run: `cd src-tauri && cargo build --release && cd ..`
Expected: build succeeds. (Optional, record the win) compare binary size before/after: `ls -la src-tauri/target/release/ai-usage-tracker`.

- [ ] **Step 3: Verify the test suite is unaffected**

Run: `cd src-tauri && cargo test --lib && cd ..`
Expected: all existing tests PASS (66 at time of writing).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/Cargo.toml
git commit -m "perf: add size-optimized release profile (no panic=abort, keeps isolation)"
```

---

### Task 3: Lazy-load the two window roots so the popover stops parsing Dashboard

**Files:**
- Modify: `src/App.tsx:1-35`

**Interfaces:**
- Consumes: `Dashboard` (named export from `@/components/Dashboard`), `TrayPopover` (named export from `@/components/TrayPopover`).
- Produces: same `App` default export; render is unchanged behaviorally, only code-split.

- [ ] **Step 1: Convert the static imports to `React.lazy` + `Suspense`**

In `src/App.tsx`, replace the two static component imports:

```tsx
import { Dashboard } from "@/components/Dashboard";
import { TrayPopover } from "@/components/TrayPopover";
```

with lazy imports (both components are *named* exports, so map them to `default`):

```tsx
import { lazy, Suspense } from "react";

const Dashboard = lazy(() =>
  import("@/components/Dashboard").then((m) => ({ default: m.Dashboard })),
);
const TrayPopover = lazy(() =>
  import("@/components/TrayPopover").then((m) => ({ default: m.TrayPopover })),
);
```

- [ ] **Step 2: Wrap the render in `Suspense`**

Change the component body return from:

```tsx
export default function App() {
  return isPopover ? <TrayPopover /> : <Dashboard />;
}
```

to:

```tsx
export default function App() {
  return (
    <Suspense fallback={null}>
      {isPopover ? <TrayPopover /> : <Dashboard />}
    </Suspense>
  );
}
```

(`fallback={null}` is acceptable — both chunks are local and load in a few ms; no spinner flash desired for a menu-bar app.)

- [ ] **Step 3: Type-check**

Run: `pnpm exec tsc --noEmit`
Expected: PASS (no type errors).

- [ ] **Step 4: Build and confirm the chunks split**

Run: `pnpm build`
Expected: build succeeds and the Vite output lists **separate** chunks for Dashboard and TrayPopover (e.g. `dist/assets/Dashboard-*.js` and `dist/assets/TrayPopover-*.js`), not one monolithic `index-*.js`. Verify: `ls dist/assets/*.js` shows more than one JS chunk.

- [ ] **Step 5: Commit**

```bash
git add src/App.tsx
git commit -m "perf: lazy-load Dashboard/TrayPopover so the popover webview skips the Dashboard bundle"
```

---

### Task 4: Replace the hardcoded "Updated just now" with the real fetch timestamp

**Files:**
- Modify: `src/components/Dashboard.tsx:291-295` (footer) and its import block
- Create: `src/lib/format.test.ts` (regression guard for the helper the footer now uses)

**Interfaces:**
- Consumes: `formatUpdatedAgo(fetchedAtSec: number | null, nowMs: number): string` from `@/lib/format` (already exists, format.ts:61); `snapshot` (`UsageSnapshot | null`, has `fetched_at: number` epoch seconds) and `nowMs` (already in `Dashboard` scope, Dashboard.tsx:99).
- Produces: a live-updating footer label.

- [ ] **Step 1: Write a failing test for the helper's boundaries**

Create `src/lib/format.test.ts`:

```ts
import { describe, expect, it } from "vitest";

import { formatUpdatedAgo } from "@/lib/format";

describe("formatUpdatedAgo", () => {
  it("says 'just now' under 5s and counts seconds after", () => {
    const fetchedAt = 1_000_000; // epoch seconds
    const nowMs = fetchedAt * 1000;
    expect(formatUpdatedAgo(fetchedAt, nowMs)).toBe("Updated just now");
    expect(formatUpdatedAgo(fetchedAt, nowMs + 30_000)).toBe("Updated 30s ago");
  });

  it("returns the awaiting message when no timestamp", () => {
    expect(formatUpdatedAgo(null, 0)).toBe("Awaiting first update…");
  });
});
```

- [ ] **Step 2: Run it to confirm it passes against the existing helper**

Run: `pnpm test -- format.test.ts`
Expected: PASS (the helper already exists — this test locks its behavior before the footer depends on it).

- [ ] **Step 3: Ensure `formatUpdatedAgo` is imported in Dashboard**

In `src/components/Dashboard.tsx`, confirm the `@/lib/format` import line includes `formatUpdatedAgo`. If the existing import is e.g. `import { formatPercent, severityBarClass } from "@/lib/format";`, add `formatUpdatedAgo`:

```tsx
import { formatPercent, formatUpdatedAgo, severityBarClass } from "@/lib/format";
```

(Match the existing named imports already present; only add `formatUpdatedAgo` if absent.)

- [ ] **Step 4: Replace the hardcoded footer string**

In `src/components/Dashboard.tsx`, change the footer span (lines 292-295):

```tsx
              <span className="inline-flex items-center gap-1.5">
                <Check className="size-3.5" />
                Updated just now
              </span>
```

to use the real snapshot timestamp:

```tsx
              <span className="inline-flex items-center gap-1.5">
                <Check className="size-3.5" />
                {formatUpdatedAgo(snapshot?.fetched_at ?? null, nowMs)}
              </span>
```

- [ ] **Step 5: Type-check and run the frontend suite**

Run: `pnpm exec tsc --noEmit && pnpm test`
Expected: PASS.

- [ ] **Step 6: Visually verify (macOS)**

Run: `pnpm tauri dev`
Expected: the footer reads "Updated just now" right after a refresh, then ticks "Updated 5s ago", "Updated 1m 0s ago", … proving it's bound to real data, not a constant.

- [ ] **Step 7: Commit**

```bash
git add src/components/Dashboard.tsx src/lib/format.test.ts
git commit -m "fix(ui): bind footer to real fetched_at instead of hardcoded 'Updated just now'"
```

---

### Task 5: Stand up CI — lint, unit tests, and a 3-OS build smoke (no signing)

**Files:**
- Create: `.github/workflows/ci.yml`
- Create: `.github/workflows/build-smoke.yml`

**Interfaces:**
- Consumes: existing `pnpm test`, `cargo test --lib`, `cargo clippy`, `cargo fmt`, `tsc`, and `tauri build` scripts.
- Produces: green checks on push/PR; the mechanism that verifies Windows/Linux (which can't be verified locally).

- [ ] **Step 1: Create the fast lint+unit workflow**

Create `.github/workflows/ci.yml`:

```yaml
name: CI
on:
  push: { branches: [main] }
  pull_request:

concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true

jobs:
  frontend:
    name: Frontend (tsc + vitest)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
        with: { version: 9 }
      - uses: actions/setup-node@v4
        with: { node-version: 24, cache: pnpm }
      - run: pnpm install --frozen-lockfile
      - name: Type-check
        run: pnpm exec tsc --noEmit
      - name: Unit tests
        run: pnpm test

  rust:
    name: Rust (fmt + clippy + test)
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        os: [macos-latest, windows-latest, ubuntu-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { components: rustfmt, clippy }
      - name: Linux deps (Tauri)
        if: matrix.os == 'ubuntu-latest'
        run: |
          sudo apt-get update
          sudo apt-get install -y libwebkit2gtk-4.1-dev libayatana-appindicator3-dev \
            librsvg2-dev libssl-dev
      - uses: Swatinem/rust-cache@v2
        with: { workspaces: src-tauri }
      - name: cargo fmt --check
        if: matrix.os == 'ubuntu-latest'
        working-directory: src-tauri
        run: cargo fmt --all -- --check
      - name: cargo clippy
        if: matrix.os == 'ubuntu-latest'
        working-directory: src-tauri
        run: cargo clippy --all-targets -- -D warnings
      - name: cargo test
        working-directory: src-tauri
        run: cargo test --lib --locked
```

- [ ] **Step 2: Create the 3-OS build-smoke workflow (debug, no bundle, no signing)**

Create `.github/workflows/build-smoke.yml`:

```yaml
name: Build smoke
on:
  push: { branches: [main] }
  pull_request:
  workflow_dispatch:

jobs:
  build:
    name: tauri build (debug, no-bundle)
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        os: [macos-latest, windows-latest, ubuntu-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
        with: { version: 9 }
      - uses: actions/setup-node@v4
        with: { node-version: 24, cache: pnpm }
      - uses: dtolnay/rust-toolchain@stable
      - name: Linux deps (Tauri)
        if: matrix.os == 'ubuntu-latest'
        run: |
          sudo apt-get update
          sudo apt-get install -y libwebkit2gtk-4.1-dev libayatana-appindicator3-dev \
            librsvg2-dev libssl-dev
      - uses: Swatinem/rust-cache@v2
        with: { workspaces: src-tauri }
      - run: pnpm install --frozen-lockfile
      - name: tauri build (debug, no bundle, NO signing)
        run: pnpm exec tauri build --debug --no-bundle
```

- [ ] **Step 3: Validate YAML locally**

Run: `node -e "const y=require('fs').readFileSync('.github/workflows/ci.yml','utf8'); require('child_process')" ` — or simpler, confirm indentation by eye and that both files parse: `python3 -c "import yaml,sys; [yaml.safe_load(open(f)) for f in ['.github/workflows/ci.yml','.github/workflows/build-smoke.yml']]; print('ok')"`
Expected: `ok` (no YAML parse error).

- [ ] **Step 4: Commit and push a branch to confirm green**

```bash
git add .github/workflows/ci.yml .github/workflows/build-smoke.yml
git commit -m "ci: add lint+unit CI and 3-OS build smoke (no signing)"
git push -u origin <branch>
```

Then verify on GitHub: both workflows run; `rust` passes on all three OSes (this is the first time Windows/Linux compile is verified); `build-smoke` succeeds on all three. If Linux fails on a missing system lib, add it to the `apt-get install` line and re-push.

---

## Self-Review

- **Spec coverage:** Covers P0 items #2 (CI), #3 partial (lazy split + release profile; the 1 Hz re-render memoization is intentionally deferred — see below), #4 partial (footer honesty; the larger fake-nav/`recentActivity()` removal is part of the Cockpit redesign plan), #5 (CSP). The token write-failure fix (P0 #1) and Copilot keystore + keychain migration (P0 #6) are deferred to a dedicated plan (they need a logging strategy + a cross-OS keychain abstraction + reading the exact `write_back`/`write_auth`/`read_copilot_token` signatures).
- **Placeholder scan:** none — every step has exact paths, exact code, exact commands.
- **Type consistency:** `formatUpdatedAgo(fetchedAtSec, nowMs)` matches format.ts:61; `snapshot.fetched_at` matches types.ts:42; lazy default-mapping matches the named exports.

## Follow-up plans (NOT in this plan — each needs its own dedicated plan)

1. **Token safety + keychain migration + Copilot keystore read** (P0 #1 + user-chosen direction). Surface `write_back`/`write_auth`/`write_back_creds`/`store::update` failures (currently `let _ =` at claude.rs:499/530, codex.rs:279, gemini.rs:195, mod.rs:77); move `accounts.json` tokens into OS keychain; add non-macOS Copilot keystore read (Credential Manager / libsecret) + `COPILOT_HOME`. Needs a logging approach and a cross-OS secret-store abstraction.
2. **Cockpit UI redesign** (chosen direction A). Remove fake `NAV_PRIMARY`/`NAV_WORKSPACE` (Dashboard.tsx:81-94) + traffic-light dots + `recentActivity()` fabrication; rebuild card/tray/modal around the remaining-headroom radial gauge. Large; brainstorm component anatomy first.
3. **1 Hz re-render fix** (P0 #3 remainder) — memoize cards / isolate the `useNow` timer or coarsen interval. Pending user decision on countdown granularity.
4. **Refactor**, **full test suite (HTTP/contract/E2E)**, **a11y**, **i18n (en/ko)**, **packaging+signing** (once signing identity is decided) — one plan each, per the consolidated roadmap §4.
