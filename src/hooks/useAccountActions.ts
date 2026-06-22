import { useCallback, useEffect, useRef, useState } from "react";

import {
  clearAccountAction,
  finishAccountAction,
  getAccountAction,
  isAccountActionPending,
  startAccountAction,
  type AccountActionCompletionStatus,
  type AccountActionKind,
  type AccountActionState,
  type AccountActionStatus,
} from "@/lib/accountActionState";

function accountActionKey(serviceId: string, kind: AccountActionKind): string {
  return `${kind}:${serviceId}`;
}

export interface UseAccountActionsResult {
  /**
   * The current action map as React state — render consumers (card list, detail
   * dialog, `detailRefreshing`) read this so they re-render when it changes.
   */
  accountActions: AccountActionState;
  /**
   * Read the latest action status from the ref (no re-render dependency) — for
   * handlers and event effects that must see the freshest value without going
   * stale across the async gap.
   */
  getCurrentAction: (
    serviceId: string,
    kind: AccountActionKind,
  ) => AccountActionStatus | null;
  /** Ref-backed `isPending` — same freshness contract as `getCurrentAction`. */
  isActionPending: (serviceId: string, kind: AccountActionKind) => boolean;
  /**
   * Mark an action pending. Returns false if one of that kind is already pending
   * for the service (the caller should abort). Cancels any scheduled clear timer.
   */
  beginAccountAction: (serviceId: string, kind: AccountActionKind) => boolean;
  /**
   * Resolve a pending action to success/error and schedule it to clear after the
   * visible feedback window. No-op (returns false) if it was not pending.
   */
  finishVisibleAccountAction: (
    serviceId: string,
    kind: AccountActionKind,
    status: AccountActionCompletionStatus,
  ) => boolean;
}

/**
 * The per-account action lifecycle engine: the pure `accountActionState` map
 * paired with the timer Map that auto-clears the success/error badge after a
 * fixed window. State + a ref mirror so handlers/effects read the freshest value
 * without stale closures while render consumers still re-render on change.
 */
export function useAccountActions(): UseAccountActionsResult {
  const [accountActions, setAccountActions] = useState<AccountActionState>({});
  const accountActionsRef = useRef<AccountActionState>({});
  const clearActionTimersRef = useRef<Map<string, number>>(new Map());

  const applyAccountActions = useCallback((next: AccountActionState) => {
    accountActionsRef.current = next;
    setAccountActions(next);
  }, []);

  const getCurrentAction = useCallback(
    (serviceId: string, kind: AccountActionKind) =>
      getAccountAction(accountActionsRef.current, serviceId, kind),
    [],
  );

  const isActionPending = useCallback(
    (serviceId: string, kind: AccountActionKind) =>
      isAccountActionPending(accountActionsRef.current, serviceId, kind),
    [],
  );

  const clearVisibleAccountAction = useCallback(
    (serviceId: string, kind: AccountActionKind) => {
      const next = clearAccountAction(accountActionsRef.current, serviceId, kind);
      if (next !== accountActionsRef.current) {
        applyAccountActions(next);
      }
    },
    [applyAccountActions],
  );

  const scheduleAccountActionClear = useCallback(
    (serviceId: string, kind: AccountActionKind) => {
      const key = accountActionKey(serviceId, kind);
      const existing = clearActionTimersRef.current.get(key);
      if (existing != null) {
        window.clearTimeout(existing);
      }

      const timeout = window.setTimeout(() => {
        clearActionTimersRef.current.delete(key);
        clearVisibleAccountAction(serviceId, kind);
      }, 2200);
      clearActionTimersRef.current.set(key, timeout);
    },
    [clearVisibleAccountAction],
  );

  const beginAccountAction = useCallback(
    (serviceId: string, kind: AccountActionKind) => {
      const result = startAccountAction(
        accountActionsRef.current,
        serviceId,
        kind,
      );
      if (!result.started) return false;

      const key = accountActionKey(serviceId, kind);
      const existing = clearActionTimersRef.current.get(key);
      if (existing != null) {
        window.clearTimeout(existing);
        clearActionTimersRef.current.delete(key);
      }

      applyAccountActions(result.state);
      return true;
    },
    [applyAccountActions],
  );

  const finishVisibleAccountAction = useCallback(
    (
      serviceId: string,
      kind: AccountActionKind,
      status: AccountActionCompletionStatus,
    ) => {
      const next = finishAccountAction(
        accountActionsRef.current,
        serviceId,
        kind,
        status,
      );
      if (next === accountActionsRef.current) return false;
      applyAccountActions(next);
      scheduleAccountActionClear(serviceId, kind);
      return true;
    },
    [applyAccountActions, scheduleAccountActionClear],
  );

  useEffect(
    () => () => {
      for (const timeout of clearActionTimersRef.current.values()) {
        window.clearTimeout(timeout);
      }
      clearActionTimersRef.current.clear();
    },
    [],
  );

  return {
    accountActions,
    getCurrentAction,
    isActionPending,
    beginAccountAction,
    finishVisibleAccountAction,
  };
}
