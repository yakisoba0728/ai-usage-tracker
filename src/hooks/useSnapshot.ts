import { useCallback, useEffect, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";

import { getUsage, onUsageUpdated, refreshNow } from "@/lib/ipc";
import type { UsageSnapshot } from "@/lib/types";

export interface UseSnapshotResult {
  snapshot: UsageSnapshot | null;
  loading: boolean;
  refreshing: boolean;
  error: string | null;
  refresh: () => Promise<void>;
}

/**
 * Subscribe to the live `usage-updated` push, fetch once on mount, and expose
 * an on-demand `refresh`. `loading` is the initial cold-start state; a later
 * `refresh()` flips `refreshing` instead so the UI can keep rendering stale
 * data behind the spinner.
 */
export function useSnapshot(): UseSnapshotResult {
  const [snapshot, setSnapshot] = useState<UsageSnapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    getUsage()
      .then((s) => {
        if (!cancelled) setSnapshot(s);
      })
      .catch((err) => {
        if (!cancelled) setError(String(err));
        console.error("get_usage failed:", err);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

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
    setRefreshing(true);
    try {
      const s = await refreshNow();
      setSnapshot(s);
      setError(null);
    } catch (err) {
      console.error("refresh_now failed:", err);
      setError(String(err));
    } finally {
      setRefreshing(false);
    }
  }, []);

  return { snapshot, loading, refreshing, error, refresh };
}
