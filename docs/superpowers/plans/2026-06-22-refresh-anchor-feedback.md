# Refresh Anchor Feedback Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make refresh and manual anchor-send actions visibly and accessibly communicate pending, success, and failure states.

**Architecture:** Keep the dashboard as the owner of user-triggered work state. Use small pure helpers for per-account activity transitions, then pass derived loading/status props into cards and detail panels. Use scoped overlays so only the affected content region looks busy.

**Tech Stack:** React 19, TypeScript, Tailwind CSS, Vitest, Tauri IPC.

---

### Task 1: Account Action State

**Files:**
- Create: `src/lib/accountActionState.ts`
- Create: `src/lib/accountActionState.test.ts`

- [ ] Write tests for starting, finishing, and ignoring duplicate per-account actions.
- [ ] Implement a small immutable reducer-style helper set.
- [ ] Run: `pnpm test src/lib/accountActionState.test.ts`

### Task 2: Refresh Feedback

**Files:**
- Modify: `src/hooks/useSnapshot.ts`
- Modify: `src/lib/snapshotState.test.ts`
- Modify: `src/components/Dashboard.tsx`
- Modify: `src/components/dashboard/AccountDetailDialog.tsx`

- [ ] Add the missing refresh idempotency test.
- [ ] Return a structured refresh result from `useSnapshot.refresh()`.
- [ ] Add a scoped dashboard refresh overlay with `role="status"` and `aria-busy`.
- [ ] Add a scoped detail-panel refresh overlay.
- [ ] Run: `pnpm test src/lib/snapshotState.test.ts`

### Task 3: Manual Anchor Feedback

**Files:**
- Modify: `src/components/Dashboard.tsx`
- Modify: `src/components/dashboard/AccountCard.tsx`
- Modify: `src/components/dashboard/detail/InspectorSettings.tsx`

- [ ] Route manual anchor sends through Dashboard-owned handlers.
- [ ] Disable duplicate sends for the same account.
- [ ] Show card-level pending/success/failure feedback.
- [ ] Preserve existing background `anchor-result` event toasts.
- [ ] Keep unsupported/offline send controls hidden or disabled.

### Task 4: Styling And Copy

**Files:**
- Modify: `src/index.css`
- Modify: `src/locales/en.json`
- Modify: `src/locales/ko.json`

- [ ] Add scoped overlay and card activity animation classes.
- [ ] Add localized refresh/send status text.
- [ ] Keep English and Korean locale key sets identical.
- [ ] Run: `pnpm test src/locales/locales.test.ts`

### Task 5: Verification

**Files:**
- Modify or create focused tests only where the state is stable.

- [ ] Run: `pnpm test`
- [ ] Run: `pnpm lint`
- [ ] Run: `pnpm exec tsc --noEmit`
- [ ] Run rendered verification against the Vite app using the Browser plugin if available, otherwise Playwright.
