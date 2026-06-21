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
import { formatUpdatedAgo } from "@/lib/format";
import {
  buildAccountSections,
  selectVisibleServiceId,
} from "@/lib/inspectorModel";
import {
  getConfig,
  onAnchorResult,
  onTriggerRefresh,
  removeAccount,
  setConfig,
} from "@/lib/ipc";
import {
  providerConfigFor,
  providerDisplayName,
  resolveHeadlineWindow,
} from "@/lib/providers";
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

  // Stable so memoized cards don't re-render when an unrelated card is selected
  // or a provider-loading event fires.
  const handleSelectService = useCallback((id: string) => {
    setOpenServiceId(id);
    setInspectorTab("limits");
  }, []);

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

  useEffect(() => {
    if (openServiceId != null && visibleServiceId == null) {
      setOpenServiceId(null);
    }
  }, [openServiceId, visibleServiceId]);


  const [toasts, setToasts] = useState<Toast[]>([]);
  const toastIdRef = useRef(0);
  const prevPctRef = useRef<Map<string, number>>(new Map());
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
    const un = onAnchorResult((p) => {
      pushToast(p.ok ? t("toast.anchorSent") : t("toast.anchorFailed", { error: p.detail ?? t("error.unknown") }));
    }).catch((e) => {
      console.error("subscribe anchor-result failed:", e);
      return undefined;
    });
    return () => {
      void un.then((u) => u?.());
    };
  }, [pushToast, t]);

  useEffect(() => {
    if (!snapshot || !config) return;
    if (lastProcessedRef.current === snapshot.fetched_at) return;
    lastProcessedRef.current = snapshot.fetched_at;

    const prev = prevPctRef.current;
    for (const service of snapshot.services) {
      const headline = resolveHeadlineWindow(service, config);
      const pct = headline?.used_percent;
      if (pct == null) {
        prev.delete(service.id);
        continue;
      }
      const previous = prev.get(service.id);
      prev.set(service.id, pct);
      if (previous == null) continue;
      const thresholds =
        providerConfigFor(config, service.provider)?.notify_thresholds ?? [];
      const crossed = thresholds
        .filter((threshold) => previous < threshold && pct >= threshold)
        .sort((a, b) => b - a)[0];
      if (crossed != null) {
        pushToast(
          t("toast.reached", {
            provider: providerDisplayName(config, service.provider),
            percent: Math.round(crossed),
          }),
        );
      }
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
      pushToast(t("toast.removeFailed", { error: String(e) }));
    }
  }

  return (
    <div className="min-h-dvh overflow-hidden bg-canvas text-text">
      <div className="flex min-h-dvh">
        <div className="flex min-w-0 flex-1 flex-col">
          <section className="relative flex min-h-dvh min-w-0 flex-col bg-canvas/95">
            {refreshing && (
              <div className="pointer-events-none absolute inset-0 z-20 flex items-center justify-center bg-canvas/50 backdrop-blur-[1.5px]">
                <RefreshCw className="size-8 animate-spin text-text-dim" />
              </div>
            )}
            <MobileHeader
              refreshing={refreshing}
              onRefresh={() => void refresh()}
              onOpenSettings={() => setSettingsOpen(true)}
            />

            <AccountToolbar
              query={query}
              onQueryChange={setQuery}
              onAddAccount={() => setAddOpen(true)}
            />

            <div className="px-5 pb-2 pt-1 text-text-dim">
              <span className="num text-sm">{onlineCount}</span>
              <span className="ml-1 text-sm">{t("nav.accounts")}</span>
            </div>

            <div className="scroll-area min-h-0 flex-1 overflow-y-auto px-4 pb-5">
              {loading && snapshot == null ? (
                <LoadingState />
              ) : snapshot == null ? (
                <ErrorState error={error ?? t("error.backendUnreachable")} />
              ) : !hasConfigured ? (
                <EmptyState onAddAccount={() => setAddOpen(true)} />
              ) : accountSections.length === 0 ? (
                <NoResults query={query} onShowOffline={() => setShowOffline(true)} />
              ) : (
                <AccountSections
                  sections={accountSections}
                  selectedId={visibleServiceId}
                  nowMs={nowMs}
                  loadingProviders={loadingProviders}
                  onSelect={handleSelectService}
                />
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
        refreshing={refreshing}
        tab={inspectorTab}
        onTabChange={setInspectorTab}
        moreOpen={moreMenuOpen}
        onMoreOpenChange={setMoreMenuOpen}
        onRefresh={() => void refresh()}
        onOpenAdd={() => setAddOpen(true)}
        onOpenSettings={() => setSettingsOpen(true)}
        onConfigChange={updateConfig}
        onRemove={handleRemoveSelected}
      />

      <AddAccountDialog
        open={addOpen}
        onOpenChange={setAddOpen}
        onChanged={() => void refresh()}
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
          void refresh();
          pushToast(t("toast.scanning"));
        }}
        onOpenAddAccount={() => setAddOpen(true)}
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
  return (
    <div className="rounded-lg border border-border bg-surface/50 px-5 py-12 text-center">
      <Search className="mx-auto mb-3 size-6 text-text-faint" />
      <h2 className="text-sm font-semibold">{t("noResults.title")}</h2>
      <p className="mt-1 text-sm text-text-faint">
        {query ? t("noResults.hintQuery") : t("noResults.hintOffline")}
      </p>
      <Button variant="secondary" size="sm" className="mt-4" onClick={onShowOffline}>
        {t("noResults.showOffline")}
      </Button>
    </div>
  );
}
