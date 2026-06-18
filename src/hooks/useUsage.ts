import { useCallback, useEffect, useState } from "react";

import { getUsage, onUsageUpdated, refreshNow } from "@/lib/ipc";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type { UsageSnapshot } from "@/lib/types";

export interface UseUsageResult {
  snapshot: UsageSnapshot | null;
  loading: boolean;
  refresh: () => Promise<void>;
}

export function useUsage(): UseUsageResult {
  const [snapshot, setSnapshot] = useState<UsageSnapshot | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;

    // Initial fetch.
    getUsage()
      .then((s) => {
        if (!cancelled) setSnapshot(s);
      })
      .catch((err) => {
        console.error("get_usage failed:", err);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    // Live push subscription.
    const unlistenPromise: Promise<UnlistenFn | undefined> = onUsageUpdated(
      (s) => setSnapshot(s),
    ).catch((err) => {
      console.error("subscribe usage-updated failed:", err);
      return undefined;
    });

    return () => {
      cancelled = true;
      void unlistenPromise.then((un) => {
        if (un) un();
      });
    };
  }, []);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const s = await refreshNow();
      setSnapshot(s);
    } catch (err) {
      console.error("refresh_now failed:", err);
    } finally {
      setLoading(false);
    }
  }, []);

  return { snapshot, loading, refresh };
}
