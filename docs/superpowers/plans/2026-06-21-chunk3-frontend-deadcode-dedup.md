# Chunk 3 — Frontend Dead-code + Dedup Implementation Plan

> Inline execution per repo workflow. Behavior-preserving. Gate: `pnpm exec tsc --noEmit` + `pnpm test` + `pnpm build`. One branch `fix/chunk3-frontend-cleanup`, commit per task, ff-merge to `main`.

**Goal:** Remove verified dead frontend code and collapse the genuine logic/markup duplication into shared helpers, with zero behavior change.

## Verified dead code (grep-confirmed: definitions only, no non-test callers)

`summarizeServices` + `StatusSummary` (status.ts), `cardWindows` (providers.ts), `severityBarClass` (format.ts), `providerOrder` fn (providers.ts). Plus dead fields `AccountRow.resetLabel` (always null, unread) and `AccountRow.providerName` (duplicates `title`; only read in `rowMatchesQuery`).

## Scope note (vs spec Chunk 3)

- The `InspectorSummary` unrendered-field removal (overallPercent/resetLabel/primaryUsedLimit/metricCards + `InspectorMetric` + `buildInspectorSummary` signature) is **moved to Chunk 4**, because it changes `buildInspectorSummary`'s signature and its `AccountDetailDialog` caller — which Chunk 4 restructures anyway.
- `<UsageBar>` extraction (touches AccountCard/PopoverDashboard/AccountDetailDialog/SettingsDialog) stays here as cross-component dedup.

---

## Task 1: delete dead exports + dead AccountRow fields

**Files:** `lib/status.ts`, `lib/providers.ts`, `lib/format.ts`, `lib/inspectorModel.ts`, `lib/status.test.ts`, `lib/providers.test.ts`.

- [ ] status.ts: delete `StatusSummary` interface + `summarizeServices`. Keep `serviceStatus`, `severityToStatus`, `ServiceStatus`. Drop now-unused imports if any (`Provider` import may become unused — remove if so).
- [ ] status.test.ts: remove the `summarizeServices` describe block (56-81); drop `summarizeServices` from the import.
- [ ] providers.ts: delete `cardWindows` and `providerOrder` fn. Keep the rest.
- [ ] providers.test.ts: remove the `providerOrder` describe block (97-117); drop `providerOrder` from import. (Check `cfg`/`pc` helpers stay used by the remaining `resolveHeadlineWindow` tests — they do.)
- [ ] format.ts: delete `severityBarClass`.
- [ ] inspectorModel.ts: remove `AccountRow.providerName` (line 23) and `AccountRow.resetLabel` (line 27); in `toAccountRow` drop the `providerName` local + the `providerName`/`resetLabel` fields (keep `title: providerDisplayName(...)`); in `rowMatchesQuery` replace `row.providerName` with nothing (its value equals `row.title`, already in the list — so just remove the `row.providerName` line).
- [ ] Verify (`tsc`/`pnpm test`/`pnpm build`) + commit `refactor(frontend): remove verified dead exports and AccountRow fields`.

## Task 2: `patchProviderConfig` helper (kills the tuple-cast dup)

**Files:** `lib/providers.ts` (add), `components/SettingsDialog.tsx`, `components/dashboard/AccountDetailDialog.tsx`.

- [ ] Add to providers.ts:
```ts
/** Immutably patch one provider's config slot. Centralizes the fixed-tuple
 *  splice + cast so call sites don't each re-cast. */
export function patchProviderConfig(
  config: AppConfig,
  provider: Provider,
  patch: Partial<ProviderConfig>,
): AppConfig {
  const providers = config.providers.map((p, i) =>
    i === providerIndex(provider) ? { ...p, ...patch } : p,
  ) as AppConfig["providers"];
  return { ...config, providers };
}
```
- [ ] SettingsDialog.tsx `toggleProvider` (and any other tuple-splice): replace the manual `[...config.providers]`/splice/`as` with `patchProviderConfig(config, provider, { ... })`.
- [ ] AccountDetailDialog.tsx `InspectorSettings.patch`: same replacement.
- [ ] Verify + commit `refactor(frontend): centralize provider-config patching`.

## Task 3: `helpers.ts` — `allServiceWindows` + `displayAccountId`

**Files:** `components/dashboard/helpers.ts` (add), `components/dashboard/AccountCard.tsx`, `components/dashboard/AccountDetailDialog.tsx`.

- [ ] Add `allServiceWindows(service): LimitWindow[]` = `[...(service.windows ?? []), ...(service.detail_windows ?? [])]`; replace the 2 inline builds in AccountDetailDialog.
- [ ] Add `displayAccountId(service): string` = strip `auto:`/`stored:` prefix; replace AccountCard's inline `.replace("auto:","").replace("stored:","")`.
- [ ] Verify + commit `refactor(frontend): share allServiceWindows + displayAccountId`.

## Task 4: `<UsageBar>` shared progress bar

**Files:** create `components/dashboard/UsageBar.tsx`; use in `AccountCard.tsx`, `PopoverDashboard.tsx`, `AccountDetailDialog.tsx` (MiniBar + card bar), `SettingsDialog.tsx` if it has a bar.

- [ ] Create `<UsageBar percent={number|null} tone={ServiceStatus|Severity} size?="sm"|"md" label?=string />` rendering the track + `statusFillClass` fill at `clamp(percent,0,100)%`, with the progressbar markup. (a11y ARIA is added in Chunk 5 — here it's pure markup dedup; keep current DOM/classes identical so screenshots don't move.)
- [ ] Replace the ~4 inline bar reimplementations. Keep each call producing the same rendered classes/heights (pass size/tone to match).
- [ ] Verify + screenshot parity (en) + commit `refactor(frontend): extract shared UsageBar`.

---

## Finalize
- [ ] tsc + pnpm test + pnpm build green; ff-merge to main; delete branch.
