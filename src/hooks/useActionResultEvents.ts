import { useEffect } from "react";
import type { TFunction } from "i18next";

import { buildAnchorToast } from "@/lib/anchorToast";
import { scrubErrorText } from "@/lib/errorScrub";
import {
  onAnchorResult,
  onRefreshResult,
  onTriggerRefresh,
} from "@/lib/ipc";
import { reconcileActionResult } from "@/lib/actionResultReconcile";
import type {
  AccountActionCompletionStatus,
  AccountActionKind,
  AccountActionStatus,
} from "@/lib/accountActionState";

export interface UseActionResultEventsArgs {
  /** Tray "Refresh now" handler — fired on `trigger-refresh`. */
  onTriggerRefreshAll: () => void;
  /** Latest tracked action status (ref-backed) for an id + kind. */
  getCurrentAction: (
    serviceId: string,
    kind: AccountActionKind,
  ) => AccountActionStatus | null;
  /** Resolve a pending action to success/error. */
  finishVisibleAccountAction: (
    serviceId: string,
    kind: AccountActionKind,
    status: AccountActionCompletionStatus,
  ) => boolean;
  pushToast: (message: string) => void;
  t: TFunction;
}

/**
 * The three backend→client result subscriptions, reconciling each event against
 * the action this client is already tracking (via `reconcileActionResult`):
 *
 * - `trigger-refresh` → run a full refresh (tray menu item).
 * - `anchor-result` → finish a pending manual anchor, then toast success/failure
 *   (auto path reads `isAuto` and names the account differently).
 * - `refresh-result` → finish a pending manual refresh; only toast on failure
 *   (success already updates the card via `usage-updated` — F-7).
 *
 * Kept as three separate effects with their exact original dependency arrays so
 * the subscribe/unsubscribe lifecycle (and language-change re-subscription via
 * `t`) is unchanged.
 */
export function useActionResultEvents({
  onTriggerRefreshAll,
  getCurrentAction,
  finishVisibleAccountAction,
  pushToast,
  t,
}: UseActionResultEventsArgs): void {
  useEffect(() => {
    const un = onTriggerRefresh(() => onTriggerRefreshAll()).catch((e) => {
      console.error("subscribe trigger-refresh failed:", e);
      return undefined;
    });
    return () => {
      void un.then((u) => u?.());
    };
  }, [onTriggerRefreshAll]);

  useEffect(() => {
    const un = onAnchorResult((p) => {
      const current = getCurrentAction(p.id, "anchor");
      const { finishStatus, proceedToToast } = reconcileActionResult(current, p.ok);
      if (finishStatus != null) {
        finishVisibleAccountAction(p.id, "anchor", finishStatus);
      }
      if (!proceedToToast) {
        return;
      }
      // No tracked manual action for this id → the background auto-anchor fired
      // it; that reads differently from a button the user just pressed.
      const isAuto = current == null;
      const toast = buildAnchorToast(
        p.provider,
        p.label,
        p.ok,
        isAuto,
        p.ok ? undefined : scrubErrorText(p.detail ?? t("error.unknown")),
      );
      pushToast(t(toast.key, toast.params));
    }).catch((e) => {
      console.error("subscribe anchor-result failed:", e);
      return undefined;
    });
    return () => {
      void un.then((u) => u?.());
    };
  }, [finishVisibleAccountAction, getCurrentAction, pushToast, t]);

  // A per-card refresh emits `refresh-result` on every path; only surface a
  // failure (success already updates the card via usage-updated) — F-7.
  useEffect(() => {
    const un = onRefreshResult((p) => {
      const current = getCurrentAction(p.id, "refresh");
      const { finishStatus, proceedToToast } = reconcileActionResult(current, p.ok);
      if (finishStatus != null) {
        finishVisibleAccountAction(p.id, "refresh", finishStatus);
      }
      if (!proceedToToast) {
        return;
      }
      if (!p.ok) {
        pushToast(
          t("toast.refreshFailed", {
            error: scrubErrorText(p.detail ?? t("error.unknown")),
          }),
        );
      }
    }).catch((e) => {
      console.error("subscribe refresh-result failed:", e);
      return undefined;
    });
    return () => {
      void un.then((u) => u?.());
    };
  }, [finishVisibleAccountAction, getCurrentAction, pushToast, t]);
}
