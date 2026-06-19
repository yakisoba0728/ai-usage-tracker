import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Loader2 } from "lucide-react";

import { Header } from "@/components/Header";
import { ProviderGrid } from "@/components/ProviderGrid";
import { ProviderDetail } from "@/components/ProviderDetail";
import { AddAccountDialog } from "@/components/AddAccountDialog";
import { SettingsDialog, type SortBy } from "@/components/SettingsDialog";
import { EmptyState } from "@/components/EmptyState";
import { ErrorState } from "@/components/ErrorState";
import { Toaster, type Toast } from "@/components/Toaster";
import { Dialog, DialogContent } from "@/components/ui/dialog";
import { useSnapshot } from "@/hooks/useSnapshot";
import { useNow } from "@/hooks/useNow";
import { getConfig, listAccounts, onTriggerRefresh, setConfig } from "@/lib/ipc";
import {
  PROVIDER_ORDER,
  providerConfigFor,
  providerDisplayName,
  providerOrder,
  resolveHeadlineWindow,
} from "@/lib/providers";
import { formatUsedLimit } from "@/lib/format";
import type { AppConfig, Provider, ServiceUsage } from "@/lib/types";

export function Dashboard() {
  const { snapshot, loading, refreshing, error, loadingProviders, refresh } =
    useSnapshot();
  const nowMs = useNow(1000);

  const [addOpen, setAddOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [openProvider, setOpenProvider] = useState<Provider | null>(null);
  const [openAccountId, setOpenAccountId] = useState<string | null>(null);

  // Display preferences (local). Default "custom" so drag-order is visible.
  const [sortBy, setSortBy] = useState<SortBy>("custom");
  const [showOffline, setShowOffline] = useState(false);

  // ── Config ownership: load once, optimistic update + persist on change. ────
  const [config, setConfigState] = useState<AppConfig | null>(null);
  useEffect(() => {
    getConfig()
      .then(setConfigState)
      .catch((e) => console.error("get_config failed:", e));
  }, []);
  const updateConfig = useCallback((next: AppConfig) => {
    setConfigState(next);
    void setConfig(next).catch((e) => console.error("set_config failed:", e));
  }, []);

  // ── Tray "Refresh now" → same path as the header Refresh button. ───────────
  useEffect(() => {
    const un = onTriggerRefresh(() => void refresh()).catch((e) => {
      console.error("subscribe trigger-refresh failed:", e);
      return undefined;
    });
    return () => {
      void un.then((u) => u?.());
    };
  }, [refresh]);

  const allServices = snapshot?.services ?? [];

  // Display order: custom (config sort_index) / usage / name.
  const order = useMemo(() => providerOrder(config), [config]);
  const services = useMemo<ServiceUsage[]>(() => {
    const list = (
      showOffline ? allServices : allServices.filter((s) => s.connected)
    ).slice();
    if (sortBy === "name") {
      list.sort((a, b) =>
        providerDisplayName(config, a.provider).localeCompare(
          providerDisplayName(config, b.provider),
        ),
      );
    } else if (sortBy === "usage") {
      list.sort(
        (a, b) => headlinePct(b, config) - headlinePct(a, config),
      );
    } else {
      list.sort(
        (a, b) => order.indexOf(a.provider) - order.indexOf(b.provider),
      );
    }
    return list;
  }, [allServices, showOffline, sortBy, order, config]);

  // When a detail modal opens, look up whether that provider has a stored
  // (user-added) credential — drives the Remove affordance.
  useEffect(() => {
    if (openProvider == null) {
      setOpenAccountId(null);
      return;
    }
    let cancelled = false;
    listAccounts()
      .then((list) => {
        if (cancelled) return;
        const match = list.find((a) => a.provider === openProvider);
        setOpenAccountId(match?.id ?? null);
      })
      .catch(() => {
        if (!cancelled) setOpenAccountId(null);
      });
    return () => {
      cancelled = true;
    };
  }, [openProvider]);

  const openService = useMemo<ServiceUsage | undefined>(
    () => snapshot?.services.find((s) => s.provider === openProvider),
    [snapshot, openProvider],
  );

  // ── Drag-and-drop reorder → rewrite sort_index, persist. ───────────────────
  const reorderProviders = useCallback(
    (from: Provider, to: Provider) => {
      if (!config) return;
      const current = providerOrder(config);
      const fromIdx = current.indexOf(from);
      const toIdx = current.indexOf(to);
      if (fromIdx < 0 || toIdx < 0 || fromIdx === toIdx) return;
      const next = [...current];
      next.splice(fromIdx, 1);
      next.splice(toIdx, 0, from);
      const providers = [...config.providers] as AppConfig["providers"];
      next.forEach((p, i) => {
        const pi = PROVIDER_ORDER.indexOf(p);
        providers[pi] = { ...providers[pi], sort_index: i };
      });
      // Dropping implicitly switches to manual order so the result is visible.
      if (sortBy !== "custom") setSortBy("custom");
      updateConfig({ ...config, providers });
    },
    [config, sortBy, updateConfig],
  );

  // ── Threshold notifications (in-app toasts) on each new snapshot. ──────────
  const [toasts, setToasts] = useState<Toast[]>([]);
  const toastIdRef = useRef(0);
  const prevPctRef = useRef<Map<Provider, number>>(new Map());
  const lastProcessedRef = useRef<number | null>(null);

  const pushToast = useCallback((message: string) => {
    const id = ++toastIdRef.current;
    setToasts((t) => [...t, { id, message }]);
    window.setTimeout(() => {
      setToasts((t) => t.filter((x) => x.id !== id));
    }, 5000);
  }, []);
  const dismissToast = useCallback((id: number) => {
    setToasts((t) => t.filter((x) => x.id !== id));
  }, []);

  useEffect(() => {
    if (!snapshot || !config) return;
    // Evaluate exactly once per new snapshot (config edits don't re-fire).
    if (lastProcessedRef.current === snapshot.fetched_at) return;
    lastProcessedRef.current = snapshot.fetched_at;

    const prev = prevPctRef.current;
    for (const s of snapshot.services) {
      const headline = resolveHeadlineWindow(s, config);
      const pct = headline?.used_percent;
      if (pct == null) {
        // Disconnected / no reading — drop the baseline so the next reconnect
        // re-establishes it instead of firing a stale crossing.
        prev.delete(s.provider);
        continue;
      }
      const prevPct = prev.get(s.provider);
      prev.set(s.provider, pct);
      if (prevPct == null) continue; // baseline — need a prior value to cross
      const pc = providerConfigFor(config, s.provider);
      const thresholds = pc?.notify_thresholds ?? [];
      // Highest threshold crossed this tick (one toast per jump, not a burst).
      let crossed: number | null = null;
      for (const t of thresholds) {
        if (prevPct < t && pct >= t && t > (crossed ?? -1)) crossed = t;
      }
      if (crossed != null) {
        const name = providerDisplayName(config, s.provider);
        const ul = headline ? formatUsedLimit(headline) : null;
        pushToast(
          `[${name}] Usage reached ${Math.round(pct)}%${ul ? ` (${ul})` : ""}`,
        );
      }
    }
  }, [snapshot, config, pushToast]);

  const hasConfigured = allServices.length > 0;
  const fetchedAt = snapshot?.fetched_at ?? null;

  return (
    <div className="relative flex min-h-dvh flex-col bg-canvas text-text">
      <Header
        fetchedAt={fetchedAt}
        nowMs={nowMs}
        refreshing={refreshing}
        onRefresh={() => void refresh()}
        onAddAccount={() => setAddOpen(true)}
        onOpenSettings={() => setSettingsOpen(true)}
      />

      <main className="mx-auto w-full max-w-[1100px] flex-1 px-5 py-6">
        {loading && snapshot == null ? (
          <LoadingState />
        ) : snapshot == null ? (
          <ErrorState error={error ?? "Couldn't reach the tracker backend."} />
        ) : !hasConfigured ? (
          <EmptyState onAddAccount={() => setAddOpen(true)} />
        ) : services.length === 0 ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-2 py-24 text-center">
            <p className="text-text-dim" style={{ fontSize: 13 }}>
              No connected providers right now.
            </p>
            <p className="text-text-faint" style={{ fontSize: 12 }}>
              Enable “Show offline” in Settings to see all configured providers.
            </p>
          </div>
        ) : (
          <ProviderGrid
            services={services}
            config={config}
            nowMs={nowMs}
            loadingProviders={loadingProviders}
            sortBy={sortBy}
            onOpen={setOpenProvider}
            onReorder={reorderProviders}
          />
        )}
      </main>

      {/* Detail modal — one instance, controlled by openProvider. */}
      <Dialog
        open={openProvider != null && openService != null}
        onOpenChange={(o) => !o && setOpenProvider(null)}
      >
        <DialogContent className="gap-0 overflow-hidden rounded-lg border-border bg-surface p-0 sm:max-w-lg">
          {openService && (
            <ProviderDetail
              service={openService}
              nowMs={nowMs}
              accountId={openAccountId}
              onRemoved={() => {
                void refresh();
                setOpenProvider(null);
              }}
              config={config}
              onConfigChange={updateConfig}
            />
          )}
        </DialogContent>
      </Dialog>

      <AddAccountDialog
        open={addOpen}
        onOpenChange={setAddOpen}
        onChanged={refresh}
      />

      <SettingsDialog
        open={settingsOpen}
        onOpenChange={setSettingsOpen}
        config={config}
        onConfigChange={updateConfig}
        sortBy={sortBy}
        onSortByChange={setSortBy}
        showOffline={showOffline}
        onShowOfflineChange={setShowOffline}
      />

      <Toaster toasts={toasts} onDismiss={dismissToast} />
    </div>
  );
}

/** Headline usage percent for sorting (-1 when unknown → sorts last). */
function headlinePct(s: ServiceUsage, config: AppConfig | null): number {
  return resolveHeadlineWindow(s, config)?.used_percent ?? -1;
}

function LoadingState() {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 py-24 text-text-dim">
      <Loader2 className="size-5 animate-spin" />
      <span style={{ fontSize: 13 }}>Loading usage…</span>
    </div>
  );
}
