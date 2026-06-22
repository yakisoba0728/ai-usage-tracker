import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  Check,
  Command,
  Languages,
  Loader2,
  Plus,
  RefreshCw,
  Search,
  Settings,
} from "lucide-react";
import { useTranslation } from "react-i18next";

import { ActionFeedbackOverlay } from "@/components/ActionFeedbackOverlay";
import { AddAccountDialog } from "@/components/AddAccountDialog";
import { EmptyState } from "@/components/EmptyState";
import { ErrorState } from "@/components/ErrorState";
import { SettingsDialog, type SortBy } from "@/components/SettingsDialog";
import { Toaster, type Toast } from "@/components/Toaster";
import { AccountSections } from "@/components/dashboard/AccountCard";
import { AccountDetailDialog } from "@/components/dashboard/AccountDetailDialog";
import { storedAccountId } from "@/components/dashboard/helpers";
import type { InspectorTab } from "@/components/dashboard/inspectorTabs";
import { Button } from "@/components/ui/button";
import { useNow } from "@/hooks/useNow";
import { useSnapshot } from "@/hooks/useSnapshot";
import {
  clearAccountAction,
  finishAccountAction,
  getAccountAction,
  isAccountActionPending,
  startAccountAction,
  type AccountActionKind,
  type AccountActionState,
} from "@/lib/accountActionState";
import {
  shouldProcessThresholdSnapshot,
  shouldShowNoResultsOfflineCta,
  transitionToAddAccount,
  transitionToSettings,
} from "@/lib/dashboardState";
import { formatUpdatedAgo } from "@/lib/format";
import {
  buildAccountSections,
  selectVisibleServiceId,
} from "@/lib/inspectorModel";
import {
  getConfig,
  onAnchorResult,
  onRefreshResult,
  onTriggerRefresh,
  refreshAccount,
  removeAccount,
  sendAnchorNow,
  setConfig,
} from "@/lib/ipc";
import {
  providerDisplayName,
} from "@/lib/providers";
import { scrubErrorText } from "@/lib/errorScrub";
import { collectThresholdCrossings } from "@/lib/thresholdToasts";
import type { AppConfig, UsageSnapshot } from "@/lib/types";

function accountActionKey(serviceId: string, kind: AccountActionKind): string {
  return `${kind}:${serviceId}`;
}

export function Dashboard() {
  const { snapshot, loading, refreshing, error, loadingProviders, refresh } =
    useSnapshot();
  // Coarse clock for the heavy card tree — reset countdowns are minute-granular,
  // so a 10s tick is plenty and cuts idle re-renders ~10x. The per-second
  // "Updated Xs ago" footer keeps its own isolated 1s clock (LiveUpdatedAgo).
  const nowMs = useNow(10000);
  const { t } = useTranslation();

  const [addOpen, setAddOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [moreMenuOpen, setMoreMenuOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [openServiceId, setOpenServiceId] = useState<string | null>(null);
  const [inspectorTab, setInspectorTab] = useState<InspectorTab>("limits");

  const [sortBy, setSortBy] = useState<SortBy>("custom");
  const [showOffline, setShowOffline] = useState(false);
  const [accountActions, setAccountActions] = useState<AccountActionState>({});
  const accountActionsRef = useRef<AccountActionState>({});
  const clearActionTimersRef = useRef<Map<string, number>>(new Map());

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

  const applyAccountActions = useCallback((next: AccountActionState) => {
    accountActionsRef.current = next;
    setAccountActions(next);
  }, []);

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
      status: "success" | "error",
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

  // Stable so memoized cards don't re-render when an unrelated card is selected
  // or a provider-loading event fires.
  const handleSelectService = useCallback((id: string) => {
    setOpenServiceId(id);
    setInspectorTab("limits");
  }, []);

  // Memoized so the `?? []` fallback doesn't hand a fresh array identity to the
  // dependent useMemos on every render (surfaced by react-hooks/exhaustive-deps).
  const allServices = useMemo(() => snapshot?.services ?? [], [snapshot]);
  const accountSections = useMemo(
    () =>
      buildAccountSections(allServices, config, {
        query,
        showOffline,
        sortBy,
      }),
    [allServices, config, query, showOffline, sortBy],
  );
  // Account count is online-only — offline accounts are never tallied here
  // (and are hidden by default; see `showOffline`).
  const onlineCount = useMemo(
    () => accountSections.find((section) => section.key === "online")?.count ?? 0,
    [accountSections],
  );
  const visibleServiceId = useMemo(
    () => selectVisibleServiceId(openServiceId, accountSections),
    [accountSections, openServiceId],
  );
  const detailService = useMemo(
    () =>
      visibleServiceId == null
        ? null
        : allServices.find((service) => service.id === visibleServiceId) ?? null,
    [allServices, visibleServiceId],
  );
  const detailRefreshing =
    refreshing ||
    (detailService != null &&
      (loadingProviders.has(detailService.id) ||
        getAccountAction(accountActions, detailService.id, "refresh") === "pending"));

  useEffect(() => {
    if (openServiceId != null && visibleServiceId == null) {
      setOpenServiceId(null);
    }
  }, [openServiceId, visibleServiceId]);


  const [toasts, setToasts] = useState<Toast[]>([]);
  const toastIdRef = useRef(0);
  const prevPctRef = useRef<Map<string, number>>(new Map());
  const lastProcessedThresholdSnapshotRef = useRef<UsageSnapshot | null>(null);
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

  const handleRefreshAll = useCallback(async () => {
    const result = await refresh();
    if (!result.ok) {
      pushToast(t("toast.refreshFailed", { error: result.error }));
    }
  }, [pushToast, refresh, t]);

  const handleRefreshAccount = useCallback(
    (serviceId: string) => {
      if (loadingProviders.has(serviceId)) return;
      if (!beginAccountAction(serviceId, "refresh")) return;

      // refresh-result is the source of truth for provider outcome; invoke
      // rejection is only a transport-level fallback.
      void refreshAccount(serviceId)
        .catch((e) => {
          if (isAccountActionPending(accountActionsRef.current, serviceId, "refresh")) {
            finishVisibleAccountAction(serviceId, "refresh", "error");
            pushToast(
              t("toast.refreshFailed", {
                error: scrubErrorText(String(e)),
              }),
            );
          }
        });
    },
    [beginAccountAction, finishVisibleAccountAction, loadingProviders, pushToast, t],
  );

  const handleSendAnchor = useCallback(
    (serviceId: string) => {
      if (!beginAccountAction(serviceId, "anchor")) return;

      void sendAnchorNow(serviceId)
        .then(() => {
          if (isAccountActionPending(accountActionsRef.current, serviceId, "anchor")) {
            finishVisibleAccountAction(serviceId, "anchor", "success");
            pushToast(t("toast.anchorSent"));
          }
        })
        .catch((e) => {
          if (isAccountActionPending(accountActionsRef.current, serviceId, "anchor")) {
            finishVisibleAccountAction(serviceId, "anchor", "error");
            pushToast(
              t("toast.anchorFailed", {
                error: scrubErrorText(String(e)),
              }),
            );
          }
        });
    },
    [beginAccountAction, finishVisibleAccountAction, pushToast, t],
  );

  useEffect(() => {
    const un = onTriggerRefresh(() => void handleRefreshAll()).catch((e) => {
      console.error("subscribe trigger-refresh failed:", e);
      return undefined;
    });
    return () => {
      void un.then((u) => u?.());
    };
  }, [handleRefreshAll]);

  useEffect(() => {
    const un = onAnchorResult((p) => {
      const current = getAccountAction(accountActionsRef.current, p.id, "anchor");
      if (current === "pending") {
        finishVisibleAccountAction(p.id, "anchor", p.ok ? "success" : "error");
      }
      if (current === "success" || current === "error") {
        return;
      }
      pushToast(
        p.ok
          ? t("toast.anchorSent")
          : t("toast.anchorFailed", {
              error: scrubErrorText(p.detail ?? t("error.unknown")),
            }),
      );
    }).catch((e) => {
      console.error("subscribe anchor-result failed:", e);
      return undefined;
    });
    return () => {
      void un.then((u) => u?.());
    };
  }, [finishVisibleAccountAction, pushToast, t]);

  // A per-card refresh emits `refresh-result` on every path; only surface a
  // failure (success already updates the card via usage-updated) — F-7.
  useEffect(() => {
    const un = onRefreshResult((p) => {
      const current = getAccountAction(accountActionsRef.current, p.id, "refresh");
      if (current === "pending") {
        finishVisibleAccountAction(p.id, "refresh", p.ok ? "success" : "error");
      }
      if (current === "success" || current === "error") {
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
  }, [finishVisibleAccountAction, pushToast, t]);

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
          provider: providerDisplayName(config, crossing.provider),
          percent: Math.round(crossing.threshold),
        }),
      );
    }
  }, [snapshot, config, pushToast, t]);

  const hasConfigured = allServices.length > 0;
  const fetchedAt = snapshot?.fetched_at ?? null;

  async function handleRemoveSelected() {
    if (!detailService) return;
    const accountId = storedAccountId(detailService);
    if (!accountId) return;
    try {
      await removeAccount(accountId);
      setMoreMenuOpen(false);
      setOpenServiceId(null);
      await refresh();
      pushToast(t("toast.removed"));
    } catch (e) {
      pushToast(t("toast.removeFailed", { error: scrubErrorText(String(e)) }));
    }
  }

  const openAddAccount = useCallback(() => {
    const next = transitionToAddAccount({
      addOpen,
      settingsOpen,
      detailOpen: openServiceId != null,
      moreMenuOpen,
    });
    setMoreMenuOpen(next.moreMenuOpen);
    if (!next.detailOpen) setOpenServiceId(null);
    setSettingsOpen(next.settingsOpen);
    setAddOpen(next.addOpen);
  }, [addOpen, moreMenuOpen, openServiceId, settingsOpen]);

  const openSettings = useCallback(() => {
    const next = transitionToSettings({
      addOpen,
      settingsOpen,
      detailOpen: openServiceId != null,
      moreMenuOpen,
    });
    setMoreMenuOpen(next.moreMenuOpen);
    if (!next.detailOpen) setOpenServiceId(null);
    setAddOpen(next.addOpen);
    setSettingsOpen(next.settingsOpen);
  }, [addOpen, moreMenuOpen, openServiceId, settingsOpen]);

  return (
    <div className="min-h-dvh overflow-hidden bg-canvas text-text">
      <div className="flex min-h-dvh">
        <div className="flex min-w-0 flex-1 flex-col">
          <section className="relative flex min-h-dvh min-w-0 flex-col bg-canvas/95">
            <MobileHeader
              refreshing={refreshing}
              onRefresh={() => void handleRefreshAll()}
              onOpenSettings={openSettings}
            />

            <AccountToolbar
              query={query}
              onQueryChange={setQuery}
              onAddAccount={openAddAccount}
            />

            <div className="px-5 pb-2 pt-1 text-text-dim">
              <span className="num text-sm">{onlineCount}</span>
              <span className="ml-1 text-sm">{t("nav.accounts")}</span>
            </div>

            <div className="relative min-h-0 flex-1" aria-busy={refreshing}>
              <div className="scroll-area h-full min-h-0 overflow-y-auto px-4 pb-5">
                {(loading || snapshot == null || snapshot.fetched_at === 0) &&
                error == null ? (
                  // Treat the cold-start sentinel (fetched_at:0, before the first
                  // real refresh) as still-loading, so a configured user never
                  // flashes the new-user EmptyState (F-2).
                  <LoadingState />
                ) : snapshot == null ? (
                  <ErrorState error={error ?? t("error.backendUnreachable")} />
                ) : !hasConfigured ? (
                  <EmptyState onAddAccount={openAddAccount} />
                ) : accountSections.length === 0 ? (
                  <NoResults query={query} onShowOffline={() => setShowOffline(true)} />
                ) : (
                  <AccountSections
                    sections={accountSections}
                    selectedId={visibleServiceId}
                    nowMs={nowMs}
                    loadingProviders={loadingProviders}
                    accountActions={accountActions}
                    onSelect={handleSelectService}
                    onRefreshAccount={handleRefreshAccount}
                    onSendAnchor={handleSendAnchor}
                  />
                )}
              </div>
              {refreshing && (
                <ActionFeedbackOverlay message={t("status.refreshingUsage")} />
              )}
            </div>

            <div className="flex items-center justify-between border-t border-border px-5 py-3 text-xs text-text-faint">
              <span className="inline-flex items-center gap-1.5">
                <Check className="size-3.5" />
                <LiveUpdatedAgo fetchedAt={fetchedAt} />
              </span>
              <button
                type="button"
                onClick={() => setShowOffline((value) => !value)}
                className="rounded-md px-2 py-1 transition-colors hover:bg-surface-2 hover:text-text"
              >
                {showOffline ? t("footer.hideOffline") : t("footer.showOffline")}
              </button>
            </div>
          </section>
        </div>
      </div>

      <AccountDetailDialog
        service={detailService}
        open={detailService != null}
        onOpenChange={(open) => {
          if (!open) {
            setOpenServiceId(null);
            setMoreMenuOpen(false);
          }
        }}
        config={config}
        nowMs={nowMs}
        fetchedAt={fetchedAt}
        refreshing={detailRefreshing}
        tab={inspectorTab}
        onTabChange={setInspectorTab}
        moreOpen={moreMenuOpen}
        onMoreOpenChange={setMoreMenuOpen}
        accountActions={accountActions}
        onRefresh={() => {
          if (detailService) handleRefreshAccount(detailService.id);
        }}
        onSendAnchor={handleSendAnchor}
        onOpenAdd={openAddAccount}
        onOpenSettings={openSettings}
        onConfigChange={updateConfig}
        onRemove={handleRemoveSelected}
      />

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
        onReuseLocalSession={() => {
          void handleRefreshAll();
          pushToast(t("toast.scanning"));
        }}
        onOpenAddAccount={openAddAccount}
      />

      <Toaster toasts={toasts} onDismiss={dismissToast} />
    </div>
  );
}

function MobileHeader({
  refreshing,
  onRefresh,
  onOpenSettings,
}: {
  refreshing: boolean;
  onRefresh: () => void;
  onOpenSettings: () => void;
}) {
  const { t, i18n } = useTranslation();
  const nextLang = i18n.resolvedLanguage === "ko" ? "en" : "ko";
  return (
    <header className="flex h-12 items-center justify-between border-b border-border px-4">
      <div className="flex items-center gap-2 font-semibold">
        <Command className="size-5 text-[#73b8f4]" />
        AI Usage Tracker
      </div>
      <div className="flex items-center gap-1">
        <Button
          variant="ghost"
          size="icon"
          onClick={() => void i18n.changeLanguage(nextLang)}
          aria-label={t("language.label")}
          title={t(nextLang === "ko" ? "language.korean" : "language.english")}
        >
          <Languages className="size-4" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          onClick={onRefresh}
          disabled={refreshing}
          aria-label={t("common.refresh")}
        >
          <RefreshCw className={refreshing ? "size-4 animate-spin" : "size-4"} />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          onClick={onOpenSettings}
          aria-label={t("common.settings")}
        >
          <Settings className="size-4" />
        </Button>
      </div>
    </header>
  );
}

function AccountToolbar({
  query,
  onQueryChange,
  onAddAccount,
}: {
  query: string;
  onQueryChange: (query: string) => void;
  onAddAccount: () => void;
}) {
  const { t } = useTranslation();
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        inputRef.current?.focus();
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  return (
    <div className="flex items-center gap-2 px-4 py-5">
      <label className="relative min-w-0 flex-1">
        <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-text-faint" />
        <input
          ref={inputRef}
          value={query}
          onChange={(event) => onQueryChange(event.target.value)}
          placeholder={t("toolbar.searchPlaceholder")}
          className="h-10 w-full rounded-lg border border-border bg-surface/60 pl-9 pr-12 text-sm text-text placeholder:text-text-faint outline-none transition-colors focus:border-border-strong focus:bg-surface"
        />
        <span className="num absolute right-3 top-1/2 -translate-y-1/2 rounded border border-border bg-surface-2 px-1.5 py-0.5 text-[10px] text-text-faint">
          ⌘K
        </span>
      </label>

      <Button
        variant="outline"
        size="default"
        onClick={onAddAccount}
        aria-label={t("toolbar.addAccount")}
        className="h-10 gap-2 border-border bg-surface/80"
      >
        <Plus className="size-4" />
        <span className="hidden sm:inline">{t("toolbar.addAccount")}</span>
      </Button>
    </div>
  );
}

/**
 * Isolated per-second clock for the "Updated Xs ago" footer, so only this tiny
 * node re-renders each second instead of the whole dashboard tree.
 */
function LiveUpdatedAgo({ fetchedAt }: { fetchedAt: number | null }) {
  const { t } = useTranslation();
  const now = useNow(1000);
  return <>{formatUpdatedAgo(fetchedAt, now, t)}</>;
}

function LoadingState() {
  const { t } = useTranslation();
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 py-24 text-text-dim">
      <Loader2 className="size-5 animate-spin" />
      <span className="text-sm">{t("detail.loading")}</span>
    </div>
  );
}

function NoResults({
  query,
  onShowOffline,
}: {
  query: string;
  onShowOffline: () => void;
}) {
  const { t } = useTranslation();
  const showOfflineCta = shouldShowNoResultsOfflineCta(query);
  return (
    <div className="rounded-lg border border-border bg-surface/50 px-5 py-12 text-center">
      <Search className="mx-auto mb-3 size-6 text-text-faint" />
      <h2 className="text-sm font-semibold">{t("noResults.title")}</h2>
      <p className="mt-1 text-sm text-text-faint">
        {query ? t("noResults.hintQuery") : t("noResults.hintOffline")}
      </p>
      {showOfflineCta && (
        <Button variant="secondary" size="sm" className="mt-4" onClick={onShowOffline}>
          {t("noResults.showOffline")}
        </Button>
      )}
    </div>
  );
}
