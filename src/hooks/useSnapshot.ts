import { useCallback, useEffect, useRef, useState } from "react";

import {
  getUsage,
  onProviderLoading,
  onUsageUpdated,
  refreshNow,
} from "@/lib/ipc";
import type { Provider, UsageSnapshot } from "@/lib/types";

export interface UseSnapshotResult {
  snapshot: UsageSnapshot | null;
  loading: boolean;
  refreshing: boolean;
  error: string | null;
  /** Providers mid-fetch in the current cycle — cleared on each snapshot. */
  loadingProviders: Set<Provider>;
  refresh: () => Promise<void>;
}

export function useSnapshot(): UseSnapshotResult {
  const [snapshot, setSnapshot] = useState<UsageSnapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Per-provider loading flags. Mutated through a ref + state mirror so the
  // `provider-loading` listener (fired outside React) can append without
  // racing, and the snapshot handler can clear in one shot.
  const loadingRef = useRef<Set<Provider>>(new Set());
  const [loadingProviders, setLoadingProviders] = useState<Set<Provider>>(
    () => new Set(),
  );

  const markLoading = useCallback((provider: Provider) => {
    const next = new Set(loadingRef.current);
    next.add(provider);
    loadingRef.current = next;
    setLoadingProviders(next);
  }, []);

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

    const usageUnlisten = onUsageUpdated((s) => {
      // A full snapshot closes every in-flight provider fetch.
      if (loadingRef.current.size > 0) {
        loadingRef.current = new Set();
        setLoadingProviders(new Set());
      }
      setSnapshot(s);
    }).catch((err) => {
      console.error("subscribe usage-updated failed:", err);
      return undefined;
    });

    const loadingUnlisten = onProviderLoading((p) => {
      if (!cancelled) markLoading(p);
    }).catch((err) => {
      console.error("subscribe provider-loading failed:", err);
      return undefined;
    });

    return () => {
      cancelled = true;
      void usageUnlisten.then((un) => un?.());
      void loadingUnlisten.then((un) => un?.());
    };
  }, [markLoading]);

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

  return { snapshot, loading, refreshing, error, loadingProviders, refresh };
}
