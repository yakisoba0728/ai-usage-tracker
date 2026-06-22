import { useEffect, useRef } from "react";
import type { TFunction } from "i18next";

import { shouldProcessThresholdSnapshot } from "@/lib/dashboardState";
import { providerDisplayName } from "@/lib/providers";
import { collectThresholdCrossings } from "@/lib/thresholdToasts";
import type { AppConfig, UsageSnapshot } from "@/lib/types";

/**
 * Watches each accepted snapshot for usage crossing a configured notify
 * threshold and toasts once per crossing. The two pieces of cross-render memory
 * — the per-service previous-percent map and the last snapshot already
 * processed (dedupes identical re-renders) — live in refs here; the actual
 * "which threshold fired for which account" decision stays in the tested
 * `collectThresholdCrossings` lib fn, and the display name in `providerDisplayName`.
 */
export function useThresholdToasts(
  snapshot: UsageSnapshot | null,
  config: AppConfig | null,
  pushToast: (message: string) => void,
  t: TFunction,
): void {
  const prevPctRef = useRef<Map<string, number>>(new Map());
  const lastProcessedThresholdSnapshotRef = useRef<UsageSnapshot | null>(null);

  useEffect(() => {
    if (!snapshot || !config) return;
    if (
      !shouldProcessThresholdSnapshot(
        lastProcessedThresholdSnapshotRef.current,
        snapshot,
      )
    ) {
      return;
    }
    lastProcessedThresholdSnapshotRef.current = snapshot;

    for (const crossing of collectThresholdCrossings(snapshot, config, prevPctRef.current)) {
      pushToast(
        t("toast.reached", {
          provider: providerDisplayName(config, crossing.serviceId, crossing.provider),
          percent: Math.round(crossing.threshold),
        }),
      );
    }
  }, [snapshot, config, pushToast, t]);
}
