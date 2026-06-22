import type {
  AccountActionCompletionStatus,
  AccountActionStatus,
} from "@/lib/accountActionState";

export interface ActionResultReconciliation {
  /**
   * The completion status to finish a still-pending action with, or `null` when
   * there is nothing to finish (the action already resolved, or was never a
   * tracked manual action — the auto path).
   */
  readonly finishStatus: AccountActionCompletionStatus | null;
  /**
   * Whether the caller should surface a toast for this event. False only when
   * the action already resolved to success/error (the invoke's own `.then`/
   * `.catch` already toasted) — surfacing again would duplicate it.
   */
  readonly proceedToToast: boolean;
}

/**
 * Pure decision for an `anchor-result` / `refresh-result` event, reconciling it
 * against the action this client is already tracking for that service id + kind
 * (spec §8 "how a pending action reconciles to success/error").
 *
 * The promise-vs-event race is the reason this exists: a manual send awaits the
 * IPC invoke's own promise AND subscribes to the broadcast event. Whichever
 * arrives first should finish the action and toast; the loser must be a no-op.
 *
 * - `current === "pending"` → finish with the event's outcome AND toast (the
 *   event won the race, or it is the only signal).
 * - `current === "success" | "error"` → already resolved; do nothing (the
 *   promise won the race).
 * - `current == null` → no tracked manual action; the background auto path fired
 *   it. Nothing to finish, but still toast (caller reads `isAuto = current == null`).
 */
export function reconcileActionResult(
  current: AccountActionStatus | null,
  ok: boolean,
): ActionResultReconciliation {
  if (current === "pending") {
    return {
      finishStatus: ok ? "success" : "error",
      proceedToToast: true,
    };
  }
  if (current === "success" || current === "error") {
    return { finishStatus: null, proceedToToast: false };
  }
  return { finishStatus: null, proceedToToast: true };
}
