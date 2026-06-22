import { useCallback, useEffect, useRef, useState } from "react";

import {
  getUsage,
  onProviderLoading,
  onUsageUpdated,
  refreshNow,
} from "@/lib/ipc";
import { scrubErrorText } from "@/lib/errorScrub";
import {
  finishRefreshActivity,
  shouldAcceptSnapshot,
  startRefreshActivity,
  type RefreshActivity,
} from "@/lib/snapshotState";
import type { UsageSnapshot } from "@/lib/types";

export interface UseSnapshotResult {
  snapshot: UsageSnapshot | null;
  loading: boolean;
  refreshing: boolean;
  error: string | null;
  /** Service ids mid-fetch in the current cycle — cleared on each snapshot. */
  loadingProviders: Set<string>;
  refresh: () => Promise<RefreshResult>;
}

export type RefreshResult =
  | { ok: true; accepted: boolean }
  | { ok: false; error: string };

export function useSnapshot(): UseSnapshotResult {
  const [snapshot, setSnapshot] = useState<UsageSnapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Per-service loading flags. Mutated through a ref + state mirror so the
  // `provider-loading` listener (fired outside React) can append without
  // racing, and the snapshot handler can clear in one shot.
  const loadingRef = useRef<Set<string>>(new Set());
  const [loadingProviders, setLoadingProviders] = useState<Set<string>>(
    () => new Set(),
  );
  const snapshotRef = useRef<UsageSnapshot | null>(null);
  const sourceOrderRef = useRef(0);
  const latestAcceptedOrderRef = useRef(0);
  const refreshActivityRef = useRef<RefreshActivity>({
    pending: 0,
    refreshing: false,
  });

  const acceptSnapshot = useCallback((incoming: UsageSnapshot, order: number) => {
    if (
      !shouldAcceptSnapshot({
        current: snapshotRef.current,
        incoming,
        incomingOrder: order,
        latestAcceptedOrder: latestAcceptedOrderRef.current,
      })
    ) {
      return false;
    }
    snapshotRef.current = incoming;
    latestAcceptedOrderRef.current = order;
    setSnapshot(incoming);
    return true;
  }, []);

  const markLoading = useCallback((serviceId: string) => {
    const next = new Set(loadingRef.current);
    next.add(serviceId);
    loadingRef.current = next;
    setLoadingProviders(next);
  }, []);

  const clearLoading = useCallback(() => {
    if (loadingRef.current.size === 0) return;
    loadingRef.current = new Set();
    setLoadingProviders(new Set());
  }, []);

  useEffect(() => {
    let cancelled = false;
    const initialOrder = ++sourceOrderRef.current;

    getUsage()
      .then((s) => {
        if (!cancelled) acceptSnapshot(s, initialOrder);
      })
      .catch((err) => {
        if (!cancelled) setError(scrubErrorText(String(err)));
        console.error("get_usage failed:", err);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    const usageUnlisten = onUsageUpdated((s) => {
      const eventOrder = ++sourceOrderRef.current;
      if (!acceptSnapshot(s, eventOrder)) return;
      // A full snapshot closes every in-flight provider fetch.
      clearLoading();
    }).catch((err) => {
      console.error("subscribe usage-updated failed:", err);
      return undefined;
    });

    const loadingUnlisten = onProviderLoading((p) => {
      if (!cancelled) markLoading(p.id);
    }).catch((err) => {
      console.error("subscribe provider-loading failed:", err);
      return undefined;
    });

    return () => {
      cancelled = true;
      void usageUnlisten.then((un) => un?.());
      void loadingUnlisten.then((un) => un?.());
    };
  }, [acceptSnapshot, clearLoading, markLoading]);

  const refresh = useCallback(async () => {
    const refreshOrder = ++sourceOrderRef.current;
    refreshActivityRef.current = startRefreshActivity(refreshActivityRef.current);
    setRefreshing(refreshActivityRef.current.refreshing);
    try {
      const s = await refreshNow();
      const accepted = acceptSnapshot(s, refreshOrder);
      if (accepted) {
        setError(null);
        clearLoading();
      }
      return { ok: true as const, accepted };
    } catch (err) {
      console.error("refresh_now failed:", err);
      const message = scrubErrorText(String(err));
      if (refreshOrder >= latestAcceptedOrderRef.current) {
        setError(message);
      }
      clearLoading();
      return { ok: false as const, error: message };
    } finally {
      refreshActivityRef.current = finishRefreshActivity(refreshActivityRef.current);
      setRefreshing(refreshActivityRef.current.refreshing);
    }
  }, [acceptSnapshot, clearLoading]);

  return { snapshot, loading, refreshing, error, loadingProviders, refresh };
}
