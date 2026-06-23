# Native Swift z.ai Slice Design

Date: 2026-06-23
Status: **Approved**. User approved the vertical-slice approach, z.ai as the first provider, and isolated native storage.

## Goal

Create a new native macOS Swift app alongside the existing Tauri app. The first implementation slice proves a real end-to-end path for z.ai usage data: native account/env credential -> Swift provider client -> usage snapshot -> menu bar extra + dashboard.

The existing Tauri implementation remains in place and unchanged during this slice.

## Confirmed Decisions

- First migration shape: **one real vertical slice**, not a UI-only shell and not all-provider parity.
- First provider: **z.ai**.
- Native storage: **fully separated** from the Tauri config/account files.
- Initial app identity: separate bundle/product identity, e.g. `com.aiusage.tracker.native` and `AI Usage Tracker Native`.
- Repo placement: new native app under `apps/macos-native/`; do not add a root `Package.swift`.
- Project shape: **Xcode macOS App project + internal SwiftPM core package**.

## Non-Goals For This Slice

- Do not remove, rewrite, or replace the Tauri app.
- Do not share write access to the existing Tauri `config.json` or `accounts.json`.
- Do not import existing Tauri accounts yet.
- Do not implement all six providers.
- Do not implement OAuth, device-code login, Cursor SQLite, Copilot Keychain, or Codex refresh logic.
- Do not implement anchor send.
- Do not implement real launch-at-login or update notification behavior yet.
- Do not support Windows or Linux in the native Swift app.

## Architecture

Use an Xcode macOS app for the native shell and an internal SwiftPM package for tested core logic.

```text
apps/
  macos-native/
    AIUsageTracker.xcodeproj
    AIUsageTracker/
      App/
      Scenes/
      Features/
      Resources/
      AIUsageTracker.entitlements
    AIUsageTrackerTests/
    Packages/
      UsageCore/
        Package.swift
        Sources/UsageCore/
        Tests/UsageCoreTests/
scripts/
  macos-native-run.sh
```

The app target owns SwiftUI scenes, window lifecycle, menu bar UI, app resources, and macOS integration shells. `UsageCore` owns `Codable` models, projection logic, z.ai parsing, credential/account storage abstractions, redaction, provider client contracts, and refresh coordination.

## Scene Model

- `MenuBarExtra`: always available surface with z.ai status, Refresh Now, Show Dashboard, Settings, and Quit.
- Dashboard window: opened on demand from the menu bar. The app is menu-bar-first and should not show the main window automatically on launch.
- `Settings`: native macOS Settings scene, not a modal inside the dashboard.
- Add Account: dashboard sheet for the first slice. It supports z.ai API key and label entry only.
- AppKit bridge: keep narrow, for activation policy, opening/focusing the dashboard, and close-to-menu-bar behavior if SwiftUI alone is insufficient.

## Runtime And Data Flow

`AppStore` is the observable UI owner. It holds the current `UsageSnapshot`, `AppConfig`, loading service ids, selected account, and transient toast/action state.

Core services:

- `RefreshCoordinator` actor: full refresh, single-service refresh shape, polling/restart shell, duplicate refresh prevention.
- `ProviderRegistry`: returns the enabled provider clients. In the first slice only z.ai is real.
- `ZaiProviderClient`: resolves `ZAI_API_KEY` or a native stored account key, calls the z.ai quota endpoint, parses response into `ServiceUsage`.
- `ConfigStore` actor: native-only config file with schema version.
- `AccountStore` actor: native-only account file for z.ai credentials. It never writes the Tauri `accounts.json`.
- `Redactor`: strips tokens, API keys, account ids, cookies, auth headers, and sensitive JSON fields before user-visible errors/raw diagnostics.

Primary flow:

```text
MenuBarExtra / Dashboard refresh
  -> AppStore.refresh()
  -> RefreshCoordinator.refreshAll()
  -> ZaiProviderClient.fetch()
  -> UsageSnapshot
  -> AppStore
  -> MenuBarExtra + Dashboard
```

The first slice should preserve the existing product invariant that one provider failure becomes a disconnected service row and does not crash or abort the app.

## Data Model

Swift models should intentionally mirror the current cross-provider shape:

- `Provider`: `claude`, `codex`, `gemini`, `copilot`, `cursor`, `zai`
- `ServiceSource`: `auto`, `stored`
- `LimitWindow`: label, used percent, reset time, used, limit
- `ServiceError`: stable code plus optional detail
- `ServiceUsage`: id, source, provider, connection state, plan, account label, error, windows, detail windows, optional redacted raw response
- `UsageSnapshot`: fetched time plus service list
- `AccountInfo`: display-only account projection with no credential material
- `AppConfig`: native-only schema with refresh interval, provider enabled/threshold settings, account display overrides, and UI preferences needed by the first slice

Provider order remains `[Claude, Codex, Gemini, Copilot, Cursor, Zai]` even though only z.ai is active initially. This keeps the future migration path aligned with the Tauri model.

## Native Storage Policy

Native storage is isolated for the first slice.

- Use a native app-specific application support directory derived from the native bundle id.
- Store native config and native z.ai accounts separately from the Tauri files.
- The Swift app may read `ZAI_API_KEY` from the environment for development/testing.
- A pasted z.ai API key is stored in the native account store only.
- Keychain migration is intentionally deferred. Moving app-added credentials to Keychain changes behavior and must be designed with signing, ACL prompts, and migration separately.

## UI Scope

Included in the first slice:

- Menu bar status for z.ai remaining headroom.
- Refresh Now, Show Dashboard, Settings, Quit actions.
- Dashboard with loading, error, empty, no-results, online/offline grouping, z.ai account row, usage bars, and refreshed-at footer.
- Account detail with Limits as the real tab; Sessions/Raw are excluded from the first slice unless a disabled tab is needed for layout continuity.
- Add z.ai Account sheet with label and secure API key entry.
- Settings scene with refresh interval and show-offline preference. Other settings can appear disabled if the backing service is not implemented.

Preferred layout is native macOS list/detail rather than a pixel-for-pixel React card grid copy. The goal is a durable native app structure, not visual parity at all costs.

## Build, Run, And Tooling

Create `scripts/macos-native-run.sh` as the single native run entrypoint. It should stop any running native app, build the Debug app, launch it, and support a `--verify` mode that confirms the process is running.

Initial verification commands:

```bash
xcodebuild -list -project apps/macos-native/AIUsageTracker.xcodeproj
swift test --package-path apps/macos-native/Packages/UsageCore
xcodebuild test -project apps/macos-native/AIUsageTracker.xcodeproj -scheme AIUsageTracker -destination 'platform=macOS' -derivedDataPath .build/xcode/macos-native
xcodebuild -project apps/macos-native/AIUsageTracker.xcodeproj -scheme AIUsageTracker -configuration Debug -derivedDataPath .build/xcode/macos-native build
scripts/macos-native-run.sh --verify
```

Keep existing `package.json`, `src/`, `src-tauri/`, and `scripts/tauri.mjs` untouched for this slice.

## Testing Strategy

Use test-driven development. No production Swift implementation without a failing test first, except generated Xcode scaffold files where practical test-first is not meaningful.

Core tests:

- `UsageCore` Codable shape tests for `ServiceUsage`, `LimitWindow`, `ServiceError`, `UsageSnapshot`, and provider order.
- z.ai fixture parser tests ported from the current Rust fixtures and parser expectations.
- redaction tests proving tokens/API keys do not appear in errors, raw response diagnostics, snapshots, or account projections.
- native `ConfigStore` and `AccountStore` tests for isolated paths, roundtrip, corrupt file behavior, and atomic writes.
- `RefreshCoordinator` tests for duplicate refresh prevention, failure isolation, fetched timestamp update, and z.ai success/failure paths.
- projection tests for remaining headroom, account grouping/filtering, headline window selection, and no raw `stored:<uuid>` display.

App tests:

- dashboard opens from menu bar action.
- close hides/focus behavior matches menu-bar-first intent.
- Add z.ai Account saves a native account without touching Tauri files.
- Settings updates first-slice preferences.

Baseline checks:

- Existing Tauri baseline remains green with `pnpm verify:runtime` before/after the native slice when feasible.
- Native checks use SwiftPM and `xcodebuild` commands listed above.

## Risks

- Xcode project scaffolding can create noisy generated files. Keep the generated app isolated under `apps/macos-native/` and review the project file carefully.
- Provider behavior can drift from Rust. Port z.ai fixtures first and keep parser tests close to current Rust expectations.
- Credential storage decisions can create user-data risk. The first slice avoids shared write access and defers import/migration.
- Menu-bar-first app behavior may require small AppKit interop. Keep it narrow and test the behavior through the run script and app tests.
- Companion and subagent artifacts under `.superpowers/` must not be committed. `.gitignore` should include `.superpowers/`.

## Implementation Handoff

After this spec is reviewed, create a detailed implementation plan with task-sized TDD steps. The plan should be executed with subagent-driven development: one implementer per task, followed by spec compliance review and code quality review.
