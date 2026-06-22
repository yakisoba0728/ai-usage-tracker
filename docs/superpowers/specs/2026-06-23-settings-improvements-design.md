# Settings Page Improvements — Design

Date: 2026-06-23
Status: **Approved** (user confirmed all 4). Target release: **v0.3.0**.

## Goal

Four settings-page improvements, all frontend-centric + 1 config field. New behavior (not behavior-preserving) → each gets a logic-in-lib test.

## 1. Left rail → real tabs + General grouping

- `SettingsDialog`: add `activeSection` state (`"general" | "providers"`, default `"general"`). The rail's General/Providers items become `<button>`s that switch `activeSection`; only the selected section renders on the right (today both render at once). Mark the active item visually (existing selected style).
- Within General, group rows under subheadings using the existing `Section`/`SettingsRow` pattern: **Display** (refresh interval, sort, show offline, language) · **System** (launch at login) · **Updates** (auto-update mode, check for updates).

## 2. Update notification mode (`update_auto_open`)

- `config.rs`: add `AppConfig.update_auto_open: bool` with `#[serde(default)]` (→ `false`). serde default means existing configs load fine with no migration (default = **notify-only**, less intrusive).
- `update.rs`: change `notify_update_available(app, update, open: bool)` — only call `open_release_page` when `open` is true. In `run_update_check`: the automatic path (`force == false`) passes `open = cfg.read().await.update_auto_open`; the manual path (`force == true`, the "Check for updates" button) passes `open = true` (explicit user action always opens). The notification always fires regardless.
- `types.ts`: add `update_auto_open?: boolean`; `ipc.ts` browser-default config includes it.
- SettingsDialog "Updates" group: a select — *"On new version: Notify only / Notify + open page"* → `patchConfig({ update_auto_open })`.

## 3. Notification threshold (%) UI

- SettingsDialog Providers section: each provider row gets an expand control (chevron). Expanded → threshold chips (`50 · 75 · 90 · 95 · 100`) that toggle membership in `providers[i].notify_thresholds`. Reuse `patchProviderConfig`.
- New pure lib fn `toggleThreshold(thresholds: number[], value: number): number[]` — adds/removes the value, keeps the list sorted ascending and deduped. Unit-tested (`src/lib/`).

## 4. Language select integrated into Settings

- SettingsDialog General → Display: a Language select (EN / KO) → `i18n.changeLanguage(lang)` (the existing browser-languagedetector already persists to localStorage). Reuse the existing `SelectValue`/`Segmented` style.
- `Dashboard.tsx`: remove the header Language button (now in Settings). Keep the i18n wiring.

## Verification

- logic-in-lib tests: `toggleThreshold` (add/remove/sort/dedupe); the `update_auto_open` branch is covered by extending update.rs tests (auto vs manual `open` flag) if cleanly reachable, else the existing run_update_check shape + the pure version logic.
- `pnpm verify:runtime` green (now includes fmt + clippy).
- Release: bump 0.2.0 → 0.3.0 (4 version files), tag push → CI release.yml builds + draft Release → publish.

## Scope notes

- Reuse existing SettingsDialog primitives (`Section`, `SettingsRow`, `Toggle`, `Segmented`, `SelectValue`, `ButtonLike`) — don't introduce a new design language.
- No backend change beyond `update_auto_open` + the `notify_update_available` signature. thresholds already exist per-provider in config.
