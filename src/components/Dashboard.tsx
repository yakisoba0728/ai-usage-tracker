import { useCallback, useEffect, useMemo, useState } from "react";
import { Check } from "lucide-react";
import { useTranslation } from "react-i18next";

import { ActionFeedbackOverlay } from "@/components/ActionFeedbackOverlay";
import { AddAccountDialog } from "@/components/AddAccountDialog";
import { EmptyState } from "@/components/EmptyState";
import { ErrorState } from "@/components/ErrorState";
import { SettingsDialog, type SortBy } from "@/components/SettingsDialog";
import { Toaster } from "@/components/Toaster";
import { AccountSections } from "@/components/dashboard/AccountCard";
import { AccountDetailDialog } from "@/components/dashboard/AccountDetailDialog";
import { AccountToolbar } from "@/components/dashboard/AccountToolbar";
import { LiveUpdatedAgo } from "@/components/dashboard/LiveUpdatedAgo";
import { LoadingState } from "@/components/dashboard/LoadingState";
import { MobileHeader } from "@/components/dashboard/MobileHeader";
import { NoResults } from "@/components/dashboard/NoResults";
import { storedAccountId } from "@/components/dashboard/helpers";
import type { InspectorTab } from "@/components/dashboard/inspectorTabs";
import { useNow } from "@/hooks/useNow";
import { useSnapshot } from "@/hooks/useSnapshot";
import { useAccountActions } from "@/hooks/useAccountActions";
import { useActionResultEvents } from "@/hooks/useActionResultEvents";
import { useThresholdToasts } from "@/hooks/useThresholdToasts";
import { useToasts } from "@/hooks/useToasts";
import { getAccountAction } from "@/lib/accountActionState";
import { buildAnchorToast } from "@/lib/anchorToast";
import {
  transitionToAddAccount,
  transitionToSettings,
} from "@/lib/dashboardState";
import {
  buildAccountSections,
  selectVisibleServiceId,
} from "@/lib/inspectorModel";
import {
  checkUpdateNow,
  getConfig,
  refreshAccount,
  removeAccount,
  renameAccount,
  sendAnchorNow,
  setConfig,
  setLaunchAtLogin,
} from "@/lib/ipc";
import { patchAccountConfig } from "@/lib/providers";
import { scrubErrorText } from "@/lib/errorScrub";
import type { AppConfig } from "@/lib/types";

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
  const {
    accountActions,
    getCurrentAction,
    isActionPending,
    beginAccountAction,
    finishVisibleAccountAction,
  } = useAccountActions();

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

  // Per-account rename (BUG-2). Persists via the dedicated `rename_account`
  // command — NOT `set_config` — so a rename never restarts the poll scheduler.
  // The optimistic local update is what refreshes the title in both runtimes.
  const handleRenameAccount = useCallback(
    (serviceId: string, name: string | null) => {
      setConfigState((prev) =>
        prev ? patchAccountConfig(prev, serviceId, { custom_name: name }) : prev,
      );
      void renameAccount(serviceId, name).catch((e) =>
        console.error("rename_account failed:", e),
      );
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

  const { toasts, pushToast, dismissToast } = useToasts();

  // Launch-at-login (FEAT-4): optimistic flag update, then the dedicated command
  // (OS login item + persist). On failure, revert the optimistic flag and toast.
  const handleLaunchAtLogin = useCallback(
    (enable: boolean) => {
      setConfigState((prev) =>
        prev ? { ...prev, launch_at_login: enable } : prev,
      );
      void setLaunchAtLogin(enable).catch((e) => {
        setConfigState((prev) =>
          prev ? { ...prev, launch_at_login: !enable } : prev,
        );
        pushToast(t("toast.launchAtLoginFailed", { error: String(e) }));
      });
    },
    [pushToast, t],
  );

  // Manual "Check for updates" (FEAT-5): runs regardless of the auto-check
  // toggle. Toasts the outcome; the OS notification (with click-to-open) is
  // fired from Rust on a newer release.
  const handleCheckForUpdates = useCallback(() => {
    void checkUpdateNow()
      .then((update) => {
        pushToast(
          update
            ? t("toast.updateAvailable", { version: update.version })
            : t("toast.upToDate"),
        );
      })
      .catch((e) => pushToast(t("toast.updateCheckFailed", { error: String(e) })));
  }, [pushToast, t]);

  const handleRefreshAll = useCallback(async () => {
    const result = await refresh();
    if (!result.ok) {
      pushToast(t("toast.refreshFailed", { error: result.error }));
    }
  }, [pushToast, refresh, t]);

  // Fire-and-forget wrapper for the tray `trigger-refresh` subscription, kept
  // stable on `handleRefreshAll` so that effect's subscribe lifecycle is unchanged.
  const handleTriggerRefreshAll = useCallback(() => {
    void handleRefreshAll();
  }, [handleRefreshAll]);

  const handleRefreshAccount = useCallback(
    (serviceId: string) => {
      if (loadingProviders.has(serviceId)) return;
      if (!beginAccountAction(serviceId, "refresh")) return;

      // refresh-result is the source of truth for provider outcome; invoke
      // rejection is only a transport-level fallback.
      void refreshAccount(serviceId)
        .catch((e) => {
          if (isActionPending(serviceId, "refresh")) {
            finishVisibleAccountAction(serviceId, "refresh", "error");
            pushToast(
              t("toast.refreshFailed", {
                error: scrubErrorText(String(e)),
              }),
            );
          }
        });
    },
    [
      beginAccountAction,
      finishVisibleAccountAction,
      isActionPending,
      loadingProviders,
      pushToast,
      t,
    ],
  );

  const handleSendAnchor = useCallback(
    (serviceId: string) => {
      if (!beginAccountAction(serviceId, "anchor")) return;

      // Resolve provider + account label from the current snapshot so the
      // MANUAL toast names the account too (matches the enriched anchor-result
      // payload the auto path uses). isAuto = false here.
      const svc = allServices.find((s) => s.id === serviceId) ?? null;
      const provider = svc?.provider ?? null;
      const label = svc?.account ?? null;

      void sendAnchorNow(serviceId)
        .then(() => {
          if (isActionPending(serviceId, "anchor")) {
            finishVisibleAccountAction(serviceId, "anchor", "success");
            const toast = buildAnchorToast(provider, label, true, false);
            pushToast(t(toast.key, toast.params));
          }
        })
        .catch((e) => {
          if (isActionPending(serviceId, "anchor")) {
            finishVisibleAccountAction(serviceId, "anchor", "error");
            const toast = buildAnchorToast(
              provider,
              label,
              false,
              false,
              scrubErrorText(String(e)),
            );
            pushToast(t(toast.key, toast.params));
          }
        });
    },
    [
      allServices,
      beginAccountAction,
      finishVisibleAccountAction,
      isActionPending,
      pushToast,
      t,
    ],
  );

  useActionResultEvents({
    onTriggerRefreshAll: handleTriggerRefreshAll,
    getCurrentAction,
    finishVisibleAccountAction,
    pushToast,
    t,
  });

  useThresholdToasts(snapshot, config, pushToast, t);

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
        onRenameAccount={handleRenameAccount}
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
        onLaunchAtLoginChange={handleLaunchAtLogin}
        onCheckForUpdates={handleCheckForUpdates}
      />

      <Toaster toasts={toasts} onDismiss={dismissToast} />
    </div>
  );
}
