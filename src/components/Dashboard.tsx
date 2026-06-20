import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  Activity,
  Check,
  ChevronDown,
  Cloud,
  Code2,
  Command,
  Database,
  Edit3,
  Filter,
  Flag,
  Gauge,
  LayoutList,
  Loader2,
  MoreHorizontal,
  Plus,
  RefreshCw,
  Search,
  Settings,
  Trash2,
  Users,
} from "lucide-react";

import { AddAccountDialog } from "@/components/AddAccountDialog";
import { EmptyState } from "@/components/EmptyState";
import { ErrorState } from "@/components/ErrorState";
import { ProviderMark } from "@/components/ProviderMark";
import { SettingsDialog, type SortBy } from "@/components/SettingsDialog";
import { Toaster, type Toast } from "@/components/Toaster";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import { useNow } from "@/hooks/useNow";
import { useSnapshot } from "@/hooks/useSnapshot";
import { formatPercent, formatUpdatedAgo, formatUsedLimit } from "@/lib/format";
import {
  buildAccountSections,
  buildInspectorSummary,
  selectVisibleServiceId,
  type AccountRow,
  type AccountSection,
  type InspectorMetric,
} from "@/lib/inspectorModel";
import {
  getConfig,
  onTriggerRefresh,
  removeAccount,
  setConfig,
} from "@/lib/ipc";
import {
  PROVIDER_ORDER,
  providerConfigFor,
  providerDisplayName,
  resolveHeadlineWindow,
} from "@/lib/providers";
import { serviceStatus, summarizeServices } from "@/lib/status";
import { cn, clamp } from "@/lib/utils";
import type { AppConfig, LimitWindow, Provider, ProviderConfig, ServiceUsage } from "@/lib/types";

type InspectorTab = "overview" | "limits" | "sessions" | "settings";

const INSPECTOR_TABS: { id: InspectorTab; label: string }[] = [
  { id: "overview", label: "Overview" },
  { id: "limits", label: "Limits" },
  { id: "sessions", label: "Sessions" },
  { id: "settings", label: "Settings" },
];

const NAV_PRIMARY = [
  { id: "monitor", label: "Monitor", icon: <Activity className="size-4" /> },
  { id: "activity", label: "Activity", icon: <Gauge className="size-4" /> },
  { id: "accounts", label: "Accounts", icon: <Users className="size-4" /> },
  { id: "rules", label: "Rules", icon: <Flag className="size-4" /> },
  { id: "settings", label: "Settings", icon: <Settings className="size-4" /> },
] as const;

const NAV_WORKSPACE = [
  { id: "overview", label: "Overview", icon: <Gauge className="size-4" /> },
  { id: "members", label: "Members", icon: <Users className="size-4" />, count: 3 },
  { id: "api", label: "API & Export", icon: <Code2 className="size-4" /> },
  { id: "billing", label: "Billing", icon: <Database className="size-4" /> },
] as const;

export function Dashboard() {
  const { snapshot, loading, refreshing, error, loadingProviders, refresh } =
    useSnapshot();
  // Coarse clock for the heavy card tree — reset countdowns are minute-granular,
  // so a 10s tick is plenty and cuts idle re-renders ~10x. The per-second
  // "Updated Xs ago" footer keeps its own isolated 1s clock (LiveUpdatedAgo).
  const nowMs = useNow(10000);

  const [addOpen, setAddOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [moreMenuOpen, setMoreMenuOpen] = useState(false);
  const [activeNav, setActiveNav] = useState("monitor");
  const [query, setQuery] = useState("");
  const [openServiceId, setOpenServiceId] = useState<string | null>(null);
  const [inspectorTab, setInspectorTab] = useState<InspectorTab>("overview");

  const [sortBy, setSortBy] = useState<SortBy>("custom");
  const [showOffline, setShowOffline] = useState(true);

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
    setInspectorTab("overview");
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
  const visibleRows = useMemo(
    () => accountSections.flatMap((section) => section.rows),
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

  const summary = useMemo(
    () => summarizeServices(allServices, config),
    [allServices, config],
  );

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
          `${providerDisplayName(config, service.provider)} reached ${Math.round(crossed)}%.`,
        );
      }
    }
  }, [snapshot, config, pushToast]);

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
      pushToast("Stored account removed.");
    } catch (e) {
      pushToast(`Remove failed: ${String(e)}`);
    }
  }

  return (
    <div className="min-h-dvh overflow-hidden bg-canvas text-text">
      <div className="flex min-h-dvh">
        <ControlSidebar
          activeNav={activeNav}
          onNav={(id) => {
            setActiveNav(id);
            if (id === "settings") setSettingsOpen(true);
          }}
          fetchedAt={fetchedAt}
          nowMs={nowMs}
          refreshing={refreshing}
          onRefresh={() => void refresh()}
          connectedCount={summary.connected}
        />

        <div className="flex min-w-0 flex-1 flex-col">
          <section className="flex min-h-dvh min-w-0 flex-col bg-canvas/95">
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
              <span className="num text-sm">{visibleRows.length}</span>
              <span className="ml-1 text-sm">Accounts</span>
            </div>

            <div className="scroll-area min-h-0 flex-1 overflow-y-auto px-4 pb-5">
              {loading && snapshot == null ? (
                <LoadingState />
              ) : snapshot == null ? (
                <ErrorState error={error ?? "Couldn't reach the tracker backend."} />
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
                {showOffline ? "Hide offline" : "Show offline"}
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
          pushToast("Local sessions are being scanned again.");
        }}
        onOpenAddAccount={() => setAddOpen(true)}
      />

      <Toaster toasts={toasts} onDismiss={dismissToast} />
    </div>
  );
}

function ControlSidebar({
  activeNav,
  onNav,
  fetchedAt,
  nowMs,
  refreshing,
  onRefresh,
  connectedCount,
}: {
  activeNav: string;
  onNav: (id: string) => void;
  fetchedAt: number | null;
  nowMs: number;
  refreshing: boolean;
  onRefresh: () => void;
  connectedCount: number;
}) {
  return (
    <aside className="hidden w-[230px] shrink-0 flex-col border-r border-border bg-[#171a1d] lg:flex">
      <div className="flex h-12 items-center gap-2 px-4">
        <span className="size-3 rounded-full bg-[#ff605c]" />
        <span className="size-3 rounded-full bg-[#ffbd44]" />
        <span className="size-3 rounded-full bg-[#00ca4e]" />
      </div>

      <div className="px-4 pb-5 pt-2">
        <div className="flex items-center gap-3">
          <span className="flex size-9 items-center justify-center rounded-lg border border-border bg-surface-2 text-text">
            <Command className="size-5" />
          </span>
          <div className="min-w-0">
            <div className="truncate text-base font-semibold">Control Plane</div>
            <div className="num text-xs text-text-faint">v1.2.0</div>
          </div>
        </div>
      </div>

      <nav className="space-y-1 px-3">
        {NAV_PRIMARY.map((item) => (
          <SidebarButton
            key={item.id}
            active={activeNav === item.id}
            icon={item.icon}
            label={item.label}
            onClick={() => onNav(item.id)}
          />
        ))}
      </nav>

      <div className="mx-4 my-5 h-px bg-border" />

      <div className="px-4">
        <div className="mb-3 text-xs font-medium uppercase text-text-faint">
          Workspace
        </div>
        <button
          type="button"
          className="flex w-full items-center justify-between rounded-lg border border-border bg-surface/70 px-3 py-2 text-left transition-colors hover:border-border-strong hover:bg-surface-2"
        >
          <span className="flex min-w-0 items-center gap-2">
            <span className="size-2 rounded-full bg-ok" />
            <span className="truncate text-sm">My Workspace</span>
          </span>
          <ChevronDown className="size-4 text-text-faint" />
        </button>
      </div>

      <nav className="mt-5 space-y-1 px-3">
        {NAV_WORKSPACE.map((item) => (
          <SidebarButton
            key={item.id}
            active={false}
            icon={item.icon}
            label={item.label}
            count={"count" in item ? item.count : undefined}
            onClick={() => undefined}
          />
        ))}
      </nav>

      <div className="mt-auto border-t border-border p-4">
        <div className="mb-3 flex items-center justify-between text-xs text-text-faint">
          <span>Last refresh</span>
          <span className="num flex items-center gap-2">
            {formatUpdatedAgo(fetchedAt, nowMs).replace("Updated ", "")}
            <span className="size-2 rounded-full bg-ok" />
          </span>
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={onRefresh}
          disabled={refreshing}
          className="w-full justify-between border-border bg-surface/80 text-text"
        >
          <span className="flex items-center gap-2">
            <RefreshCw className={refreshing ? "size-4 animate-spin" : "size-4"} />
            Refresh Now
          </span>
          <span className="num rounded bg-surface-2 px-1.5 py-0.5 text-[10px] text-text-faint">
            {connectedCount}
          </span>
        </Button>
      </div>
    </aside>
  );
}

function SidebarButton({
  active,
  icon,
  label,
  count,
  onClick,
}: {
  active: boolean;
  icon: ReactNode;
  label: string;
  count?: number;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left text-sm transition-colors",
        active
          ? "bg-[#1f4261] text-text"
          : "text-text-dim hover:bg-surface/70 hover:text-text",
      )}
    >
      <span className={active ? "text-[#73b8f4]" : "text-text-faint"}>{icon}</span>
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {count != null && (
        <span className="num rounded-md bg-surface-2 px-1.5 py-0.5 text-xs text-text-faint">
          {count}
        </span>
      )}
    </button>
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
  return (
    <header className="flex h-12 items-center justify-between border-b border-border px-4 lg:hidden">
      <div className="flex items-center gap-2 font-semibold">
        <Command className="size-5 text-[#73b8f4]" />
        Control Plane
      </div>
      <div className="flex items-center gap-1">
        <Button variant="ghost" size="icon" onClick={onRefresh} disabled={refreshing}>
          <RefreshCw className={refreshing ? "size-4 animate-spin" : "size-4"} />
        </Button>
        <Button variant="ghost" size="icon" onClick={onOpenSettings}>
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
  return (
    <div className="flex items-center gap-2 px-4 py-5">
      <label className="relative min-w-0 flex-1">
        <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-text-faint" />
        <input
          value={query}
          onChange={(event) => onQueryChange(event.target.value)}
          placeholder="Search accounts, providers, tags..."
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
        className="h-10 gap-2 border-border bg-surface/80"
      >
        <Plus className="size-4" />
        <span className="hidden sm:inline">Add Account</span>
      </Button>

      <Button variant="ghost" size="icon" className="h-10 w-10 border border-border bg-surface/50">
        <LayoutList className="size-4" />
      </Button>
      <Button variant="ghost" size="icon" className="h-10 w-10 border border-border bg-surface/50">
        <Filter className="size-4" />
      </Button>
    </div>
  );
}

/**
 * Isolated per-second clock for the "Updated Xs ago" footer, so only this tiny
 * node re-renders each second instead of the whole dashboard tree.
 */
function LiveUpdatedAgo({ fetchedAt }: { fetchedAt: number | null }) {
  const now = useNow(1000);
  return <>{formatUpdatedAgo(fetchedAt, now)}</>;
}

function AccountSections({
  sections,
  selectedId,
  nowMs,
  loadingProviders,
  onSelect,
}: {
  sections: AccountSection[];
  selectedId: string | null;
  nowMs: number;
  loadingProviders: Set<Provider>;
  onSelect: (id: string) => void;
}) {
  return (
    <div className="space-y-5">
      {sections.map((section) => (
        <section key={section.key}>
          <div className="mb-3 flex items-center gap-2 px-1">
            <h2 className="text-xs font-semibold uppercase text-text-faint">
              {section.title}
            </h2>
            <span className="num rounded-md bg-surface-2 px-1.5 py-0.5 text-xs text-text-faint">
              {section.count}
            </span>
          </div>
          <div className="grid grid-cols-1 gap-3 md:grid-cols-2 2xl:grid-cols-3">
            {section.rows.map((row) => (
              <AccountCardButton
                key={row.id}
                row={row}
                nowMs={nowMs}
                selected={selectedId === row.id}
                loading={loadingProviders.has(row.service.provider)}
                onSelect={onSelect}
              />
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}

const AccountCardButton = memo(function AccountCardButton({
  row,
  nowMs,
  selected,
  loading,
  onSelect,
}: {
  row: AccountRow;
  nowMs: number;
  selected: boolean;
  loading: boolean;
  onSelect: (id: string) => void;
}) {
  const reset = row.headline?.resets_at
    ? compactReset(row.headline.resets_at, nowMs)
    : null;
  const percent = row.headlinePercent;
  const width = `${clamp(percent ?? 0, 0, 100)}%`;
  const secondary = row.service.windows
    .filter((window) => window !== row.headline)
    .slice(0, 2);

  return (
    <button
      type="button"
      onClick={() => onSelect(row.id)}
      className={cn(
        "group relative flex min-h-[188px] w-full flex-col overflow-hidden rounded-lg border p-4 text-left",
        "transition-[background-color,border-color,box-shadow,transform] duration-150 hover:-translate-y-0.5 hover:border-border-strong hover:bg-surface",
        selected
          ? "border-border-strong bg-surface-2 shadow-lg shadow-black/10"
          : row.status === "offline"
            ? "border-border bg-surface/40 hover:bg-surface/60"
            : "border-border bg-surface/60 hover:border-border-strong hover:bg-surface",
      )}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 items-center gap-3">
          <ProviderIconTile provider={row.service.provider} status={row.status} />
          <div className="min-w-0">
            <div className="flex min-w-0 items-center gap-2">
              <span className="truncate text-sm font-semibold">{row.title}</span>
              {row.service.source === "stored" && (
                <span className="rounded border border-border bg-surface px-1.5 py-0.5 text-[10px] text-text-faint">
                  Stored
                </span>
              )}
            </div>
            <div className="num mt-0.5 truncate text-xs text-text-faint">
              {row.subtitle ?? row.service.id.replace("auto:", "").replace("stored:", "")}
            </div>
          </div>
        </div>
        <span className="rounded-md border border-border bg-surface px-2 py-1 text-xs text-text-dim">
          {row.service.connected
            ? row.service.source === "stored"
              ? "Session"
              : "OAuth"
            : "Offline"}
        </span>
      </div>

      <div className="mt-5">
        <div className="mb-2 flex items-end justify-between gap-3">
          <div className="min-w-0">
            <div className="truncate text-xs font-medium text-text-faint">
              {row.headline?.label ?? "No usage window"}
            </div>
            <div className="num mt-1 truncate text-xs text-text-faint">
              {row.usedLimit ?? (row.service.connected ? "Limit not reported" : row.service.error ?? "Offline")}
            </div>
          </div>
          <div className={cn("num text-lg font-semibold", statusTextClass(row.status))}>
            {formatPercent(percent)}
          </div>
        </div>
        <div className="h-1.5 overflow-hidden rounded-full bg-white/[0.10]">
          {percent != null && (
            <div
              className={cn("h-full rounded-full transition-[width] duration-300", statusFillClass(row.status))}
              style={{ width }}
            />
          )}
        </div>
      </div>

      <div className="mt-4 grid gap-2">
        {secondary.map((window) => (
          <CompactWindowLine key={window.label} window={window} />
        ))}
        {secondary.length === 0 && (
          <div className="flex items-center justify-between text-xs text-text-faint">
            <span>{row.service.connected ? "Online" : "Offline"}</span>
            <span className="num">{reset ?? "—"}</span>
          </div>
        )}
      </div>

      <div className="mt-auto flex items-center justify-between pt-4 text-xs text-text-faint">
        <span className="num">
          {reset ? `Resets in ${reset}` : row.service.connected ? "No reset" : "Disconnected"}
        </span>
        <span className="opacity-0 transition-opacity group-hover:opacity-100">
          View details
        </span>
      </div>

      {loading && (
        <span className="provider-fetch-dot absolute right-2 top-2 size-2 rounded-full bg-[#73b8f4]" />
      )}
    </button>
  );
});

function CompactWindowLine({ window }: { window: LimitWindow }) {
  const percent = window.used_percent;
  const tone =
    percent == null
      ? "unknown"
      : percent > 85
        ? "critical"
        : percent >= 60
          ? "warning"
          : "ok";
  return (
    <div>
      <div className="mb-1 flex items-center justify-between gap-2 text-xs">
        <span className="truncate text-text-faint">{window.label}</span>
        <span className={cn("num shrink-0", statusTextClass(tone))}>
          {formatPercent(percent)}
        </span>
      </div>
      <div className="h-1 overflow-hidden rounded-full bg-white/[0.09]">
        {percent != null && (
          <div
            className={cn("h-full rounded-full", statusFillClass(tone))}
            style={{ width: `${clamp(percent, 0, 100)}%` }}
          />
        )}
      </div>
    </div>
  );
}

function AccountDetailDialog({
  service,
  open,
  onOpenChange,
  config,
  nowMs,
  fetchedAt,
  refreshing,
  tab,
  onTabChange,
  moreOpen,
  onMoreOpenChange,
  onRefresh,
  onOpenAdd,
  onOpenSettings,
  onConfigChange,
  onRemove,
}: {
  service: ServiceUsage | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  config: AppConfig | null;
  nowMs: number;
  fetchedAt: number | null;
  refreshing: boolean;
  tab: InspectorTab;
  onTabChange: (tab: InspectorTab) => void;
  moreOpen: boolean;
  onMoreOpenChange: (open: boolean) => void;
  onRefresh: () => void;
  onOpenAdd: () => void;
  onOpenSettings: () => void;
  onConfigChange: (next: AppConfig) => void;
  onRemove: () => void;
}) {
  return (
    <Dialog open={open && service != null} onOpenChange={onOpenChange}>
      <DialogContent className="h-[min(760px,86dvh)] w-[min(860px,94vw)] max-w-none gap-0 overflow-hidden rounded-xl border-border bg-[#1b1d20] p-0 shadow-2xl shadow-black/50">
        {service && (
          <>
            <DialogTitle className="sr-only">
              {providerDisplayName(config, service.provider)} details
            </DialogTitle>
            <DialogDescription className="sr-only">
              Usage, limits, sessions, and provider settings.
            </DialogDescription>
            <DetailPanelContent
              service={service}
              config={config}
              nowMs={nowMs}
              fetchedAt={fetchedAt}
              refreshing={refreshing}
              tab={tab}
              onTabChange={onTabChange}
              moreOpen={moreOpen}
              onMoreOpenChange={onMoreOpenChange}
              onRefresh={onRefresh}
              onOpenAdd={onOpenAdd}
              onOpenSettings={onOpenSettings}
              onConfigChange={onConfigChange}
              onRemove={onRemove}
            />
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}

function DetailPanelContent({
  service,
  config,
  nowMs,
  fetchedAt,
  refreshing,
  tab,
  onTabChange,
  moreOpen,
  onMoreOpenChange,
  onRefresh,
  onOpenAdd,
  onOpenSettings,
  onConfigChange,
  onRemove,
}: {
  service: ServiceUsage;
  config: AppConfig | null;
  nowMs: number;
  fetchedAt: number | null;
  refreshing: boolean;
  tab: InspectorTab;
  onTabChange: (tab: InspectorTab) => void;
  moreOpen: boolean;
  onMoreOpenChange: (open: boolean) => void;
  onRefresh: () => void;
  onOpenAdd: () => void;
  onOpenSettings: () => void;
  onConfigChange: (next: AppConfig) => void;
  onRemove: () => void;
}) {
  const summary = buildInspectorSummary(service, config, nowMs);
  const accountId = storedAccountId(service);
  const allWindows = [...(service.windows ?? []), ...(service.detail_windows ?? [])];

  return (
    <section className="scroll-area h-full min-h-0 overflow-y-auto bg-[#1b1d20]">
      <div className="sticky top-0 z-20 border-b border-border bg-[#1b1d20]/95 px-5 py-5 backdrop-blur">
        <div className="flex items-start justify-between gap-4">
          <div className="flex min-w-0 items-center gap-3">
            <ProviderIconTile provider={service.provider} status={serviceStatus(service, config)} large />
            <div className="min-w-0">
              <div className="flex items-center gap-2">
                <h1 className="truncate text-xl font-semibold">{summary.title}</h1>
                <span className={cn("size-2 rounded-full", service.connected ? "bg-ok" : "bg-text-faint")} />
                <span className="text-xs text-text-dim">
                  {service.connected ? "Connected" : "Offline"}
                </span>
              </div>
              <div className="num mt-2 grid gap-x-5 gap-y-1 text-xs text-text-faint sm:grid-cols-2">
                <span>Account ID&nbsp;&nbsp;{summary.accountId}</span>
                <span>Source&nbsp;&nbsp;{summary.sourceLabel}</span>
                <span>Last Refresh&nbsp;&nbsp;{formatUpdatedAgo(fetchedAt, nowMs).replace("Updated ", "")}</span>
                <span>Plan&nbsp;&nbsp;{service.plan ?? "—"}</span>
              </div>
            </div>
          </div>

          <div className="relative flex shrink-0 items-center gap-2">
            <Button variant="ghost" size="sm" onClick={() => onTabChange("settings")}>
              <Edit3 className="size-4" />
              Edit
            </Button>
            <Button
              variant="ghost"
              size="icon"
              onClick={() => onMoreOpenChange(!moreOpen)}
              aria-label="More account actions"
            >
              <MoreHorizontal className="size-4" />
            </Button>
            {moreOpen && (
              <div className="menu-pop absolute right-0 top-10 z-40 w-56 rounded-lg border border-border-strong bg-[#25272b]/95 p-1.5 shadow-2xl shadow-black/40">
                <MenuItem icon={<RefreshCw className={refreshing ? "size-4 animate-spin" : "size-4"} />} onClick={onRefresh}>
                  Refresh account
                </MenuItem>
                <MenuItem icon={<Cloud className="size-4" />} onClick={onOpenAdd}>
                  Re-authenticate
                </MenuItem>
                <MenuItem icon={<Settings className="size-4" />} onClick={onOpenSettings}>
                  App settings
                </MenuItem>
                <MenuItem
                  icon={<Trash2 className="size-4" />}
                  disabled={accountId == null}
                  destructive
                  onClick={onRemove}
                >
                  Remove account
                </MenuItem>
              </div>
            )}
          </div>
        </div>

        <div className="mt-5 flex gap-5 overflow-x-auto">
          {INSPECTOR_TABS.map((item) => (
            <button
              key={item.id}
              type="button"
              onClick={() => onTabChange(item.id)}
              className={cn(
                "border-b-2 pb-3 text-sm font-medium transition-colors",
                tab === item.id
                  ? "border-[#4b9bea] text-text"
                  : "border-transparent text-text-dim hover:text-text",
              )}
            >
              {item.label}
            </button>
          ))}
        </div>
      </div>

      <div className="px-5 py-5">
        {tab === "overview" && (
          <OverviewTab
            service={service}
            summary={summary}
            metrics={summary.metricCards}
            nowMs={nowMs}
            fetchedAt={fetchedAt}
          />
        )}
        {tab === "limits" && (
          <LimitsTab windows={allWindows} nowMs={nowMs} />
        )}
        {tab === "sessions" && (
          <SessionsTab
            service={service}
            fetchedAt={fetchedAt}
            nowMs={nowMs}
            onRefresh={onRefresh}
            onOpenAdd={onOpenAdd}
          />
        )}
        {tab === "settings" && (
          <InspectorSettings
            service={service}
            config={config}
            onConfigChange={onConfigChange}
            onOpenAdd={onOpenAdd}
          />
        )}
      </div>
    </section>
  );
}

function OverviewTab({
  service,
  summary,
  metrics,
  nowMs,
  fetchedAt,
}: {
  service: ServiceUsage;
  summary: ReturnType<typeof buildInspectorSummary>;
  metrics: InspectorMetric[];
  nowMs: number;
  fetchedAt: number | null;
}) {
  const status = serviceStatus(service, null);
  const daily = service.detail_windows[0]?.used_percent ?? service.windows[1]?.used_percent ?? null;
  return (
    <div className="space-y-5">
      <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
        <OverviewStat
          label="Overall Usage"
          value={formatPercent(summary.overallPercent)}
          subvalue={summary.primaryUsedLimit ?? "No limit reported"}
          tone={status}
        />
        <OverviewStat
          label="Resets In"
          value={summary.resetLabel ?? "—"}
          subvalue={summary.resetLabel ? "Current window" : "No reset"}
        />
        <OverviewStat
          label="Daily Usage"
          value={formatPercent(daily)}
          subvalue={daily == null ? "Not reported" : "Latest detail"}
          tone={daily != null && daily > 85 ? "critical" : daily != null && daily >= 60 ? "warning" : "ok"}
        />
        <OverviewStat
          label="Status"
          value={summary.status}
          subvalue={service.connected ? "Daily limit" : "Needs attention"}
          tone={serviceStatus(service, null)}
        />
      </div>

      <div className="grid gap-3 xl:grid-cols-3">
        {metrics.length > 0 ? (
          metrics.slice(0, 3).map((metric) => (
            <MetricCard key={metric.label} metric={metric} />
          ))
        ) : (
          <div className="rounded-lg border border-border bg-surface/60 p-4 text-sm text-text-faint xl:col-span-3">
            This provider did not report detailed quota windows.
          </div>
        )}
      </div>

      <div className="rounded-lg border border-border bg-surface/50">
        <div className="border-b border-border px-4 py-3">
          <h2 className="text-sm font-semibold">Recent Activity</h2>
        </div>
        <div className="divide-y divide-border">
          {recentActivity(service, fetchedAt, nowMs).map((item) => (
            <div key={`${item.time}-${item.event}`} className="grid grid-cols-[70px_minmax(0,1fr)] gap-4 px-4 py-3 text-sm">
              <span className="num text-xs text-text-faint">{item.time}</span>
              <span className="min-w-0 text-text-dim">{item.event}</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function OverviewStat({
  label,
  value,
  subvalue,
  tone = "unknown",
}: {
  label: string;
  value: string;
  subvalue: string;
  tone?: ReturnType<typeof serviceStatus>;
}) {
  return (
    <div className="rounded-lg border border-border bg-surface/50 p-4">
      <div className="mb-3 flex items-center gap-2 text-xs font-medium text-text-faint">
        <ChevronDown className="size-3.5" />
        {label}
      </div>
      <div className={cn("num text-2xl font-semibold", statusTextClass(tone))}>
        {value}
      </div>
      <div className="num mt-1 text-sm text-text-dim">{subvalue}</div>
    </div>
  );
}

function MetricCard({ metric }: { metric: InspectorMetric }) {
  const width = `${clamp(metric.percent ?? 0, 0, 100)}%`;
  const tone =
    metric.percent == null
      ? "unknown"
      : metric.percent > 85
        ? "critical"
        : metric.percent >= 60
          ? "warning"
          : "ok";
  return (
    <div className="rounded-lg border border-border bg-surface/50 p-4">
      <div className="flex items-center justify-between gap-3">
        <h3 className="truncate text-sm font-semibold">{metric.label}</h3>
        {metric.usedLimit && (
          <span className="num shrink-0 text-xs text-text-faint">{metric.usedLimit}</span>
        )}
      </div>
      <div className={cn("num mt-5 text-lg font-semibold", statusTextClass(tone))}>
        {metric.usedLimit?.split(" / ")[0] ?? formatPercent(metric.percent)}
        <span className="ml-1 text-xs font-medium text-text-dim">used</span>
      </div>
      <div className="mt-2 flex items-center gap-2">
        <div className="h-1.5 min-w-0 flex-1 overflow-hidden rounded-full bg-white/[0.10]">
          {metric.percent != null && (
            <div className={cn("h-full rounded-full", statusFillClass(tone))} style={{ width }} />
          )}
        </div>
        <span className={cn("num text-xs", statusTextClass(tone))}>
          {formatPercent(metric.percent)}
        </span>
      </div>
      <div className="num mt-3 text-xs text-text-faint">
        {metric.resetLabel ? `Resets in ${metric.resetLabel}` : "No reset"}
      </div>
    </div>
  );
}

function LimitsTab({
  windows,
  nowMs,
}: {
  windows: LimitWindow[];
  nowMs: number;
}) {
  if (windows.length === 0) {
    return (
      <div className="rounded-lg border border-border bg-surface/50 p-5 text-sm text-text-faint">
        No quota windows were reported for this account.
      </div>
    );
  }

  return (
    <div className="space-y-3">
      {windows.map((window, index) => (
        <WindowRow key={`${window.label}-${index}`} window={window} nowMs={nowMs} />
      ))}
    </div>
  );
}

function WindowRow({
  window,
  nowMs,
}: {
  window: LimitWindow;
  nowMs: number;
}) {
  const percent = window.used_percent;
  const tone =
    percent == null
      ? "unknown"
      : percent > 85
        ? "critical"
        : percent >= 60
          ? "warning"
          : "ok";
  return (
    <div className="rounded-lg border border-border bg-surface/50 p-4">
      <div className="mb-3 flex items-start justify-between gap-4">
        <div className="min-w-0">
          <h3 className="truncate text-sm font-semibold">{window.label}</h3>
          <p className="num mt-1 text-xs text-text-faint">
            {formatUsedLimit(window) ?? "No used / limit values"}
          </p>
        </div>
        <span className={cn("num text-lg font-semibold", statusTextClass(tone))}>
          {formatPercent(percent)}
        </span>
      </div>
      <MiniBar percent={percent} tone={tone} />
      <p className="num mt-3 text-xs text-text-faint">
        {window.resets_at ? `Resets in ${compactReset(window.resets_at, nowMs)}` : "No reset reported"}
      </p>
    </div>
  );
}

function SessionsTab({
  service,
  fetchedAt,
  nowMs,
  onRefresh,
  onOpenAdd,
}: {
  service: ServiceUsage;
  fetchedAt: number | null;
  nowMs: number;
  onRefresh: () => void;
  onOpenAdd: () => void;
}) {
  return (
    <div className="space-y-4">
      <div className="rounded-lg border border-border bg-surface/50 p-4">
        <div className="grid gap-4 sm:grid-cols-2">
          <InfoLine label="Session source" value={service.source === "stored" ? "Stored credential" : "Auto-detected local session"} />
          <InfoLine label="Provider" value={service.provider} mono />
          <InfoLine label="Account" value={service.account ?? "Not reported"} />
          <InfoLine label="Last parsed" value={formatUpdatedAgo(fetchedAt, nowMs).replace("Updated ", "")} mono />
        </div>
      </div>

      <div className="flex flex-wrap gap-2">
        <Button variant="outline" onClick={onRefresh}>
          <RefreshCw className="size-4" />
          Reuse Local Session
        </Button>
        <Button variant="secondary" onClick={onOpenAdd}>
          <Cloud className="size-4" />
          Re-authenticate
        </Button>
      </div>
    </div>
  );
}

function InspectorSettings({
  service,
  config,
  onConfigChange,
  onOpenAdd,
}: {
  service: ServiceUsage;
  config: AppConfig | null;
  onConfigChange: (next: AppConfig) => void;
  onOpenAdd: () => void;
}) {
  const idx = PROVIDER_ORDER.indexOf(service.provider);
  const providerConfig: ProviderConfig | null = config?.providers[idx] ?? null;
  const allWindows = [...(service.windows ?? []), ...(service.detail_windows ?? [])];
  const labels = Array.from(new Set(allWindows.map((window) => window.label)));
  const [nameDraft, setNameDraft] = useState(providerConfig?.custom_name ?? "");
  const [thresholdDraft, setThresholdDraft] = useState("");

  useEffect(() => {
    setNameDraft(providerConfig?.custom_name ?? "");
  }, [providerConfig?.custom_name, service.provider]);

  function patch(patchValue: Partial<ProviderConfig>) {
    if (!config) return;
    const providers = [...config.providers] as AppConfig["providers"];
    providers[idx] = { ...providers[idx], ...patchValue };
    onConfigChange({ ...config, providers });
  }

  function commitName() {
    const trimmed = nameDraft.trim();
    patch({ custom_name: trimmed.length > 0 ? trimmed : null });
  }

  function addThreshold() {
    const parsed = Number.parseInt(thresholdDraft, 10);
    if (Number.isNaN(parsed)) return;
    const next = clamp(parsed, 0, 100);
    const thresholds = providerConfig?.notify_thresholds ?? [];
    patch({ notify_thresholds: Array.from(new Set([...thresholds, next])).sort((a, b) => a - b) });
    setThresholdDraft("");
  }

  return (
    <div className="space-y-5">
      <div className="rounded-lg border border-border bg-surface/50 p-4">
        <h2 className="mb-4 text-sm font-semibold">Provider Display</h2>
        <label className="block">
          <span className="mb-1.5 block text-xs text-text-faint">Display name</span>
          <input
            value={nameDraft}
            onChange={(event) => setNameDraft(event.target.value)}
            onBlur={commitName}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                commitName();
                event.currentTarget.blur();
              }
            }}
            placeholder={providerDisplayName(config, service.provider)}
            className="w-full rounded-md border border-border bg-canvas px-3 py-2 text-sm text-text outline-none focus:border-border-strong"
          />
        </label>
      </div>

      <div className="rounded-lg border border-border bg-surface/50 p-4">
        <h2 className="mb-4 text-sm font-semibold">Primary Window</h2>
        <select
          value={providerConfig?.primary_window ?? ""}
          disabled={config == null || labels.length === 0}
          onChange={(event) => patch({ primary_window: event.target.value || null })}
          className="w-full rounded-md border border-border bg-canvas px-3 py-2 text-sm text-text outline-none focus:border-border-strong"
        >
          <option value="">Auto (first reported window)</option>
          {labels.map((label) => (
            <option key={label} value={label}>
              {label}
            </option>
          ))}
        </select>
      </div>

      <div className="rounded-lg border border-border bg-surface/50 p-4">
        <h2 className="mb-4 text-sm font-semibold">Usage Thresholds</h2>
        <div className="mb-3 flex flex-wrap gap-2">
          {(providerConfig?.notify_thresholds ?? []).map((threshold) => (
            <button
              key={threshold}
              type="button"
              onClick={() =>
                patch({
                  notify_thresholds: (providerConfig?.notify_thresholds ?? []).filter(
                    (item) => item !== threshold,
                  ),
                })
              }
              className="num rounded-md border border-border bg-surface-2 px-2 py-1 text-xs text-text-dim hover:text-text"
            >
              {threshold}% ×
            </button>
          ))}
        </div>
        <div className="flex gap-2">
          <input
            value={thresholdDraft}
            onChange={(event) => setThresholdDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") addThreshold();
            }}
            inputMode="numeric"
            placeholder="90"
            className="num w-24 rounded-md border border-border bg-canvas px-3 py-2 text-sm text-text outline-none focus:border-border-strong"
          />
          <Button variant="outline" onClick={addThreshold} disabled={thresholdDraft.trim() === ""}>
            Add
          </Button>
        </div>
      </div>

      <div className="rounded-lg border border-border bg-surface/50 p-4">
        <h2 className="mb-3 text-sm font-semibold text-crit">Danger Zone</h2>
        <Button variant="destructive" onClick={onOpenAdd}>
          <Cloud className="size-4" />
          Re-authenticate Account
        </Button>
      </div>
    </div>
  );
}

function ProviderIconTile({
  provider,
  status,
  large = false,
}: {
  provider: Provider;
  status: ReturnType<typeof serviceStatus>;
  large?: boolean;
}) {
  return (
    <span
      className={cn(
        "relative flex shrink-0 items-center justify-center rounded-md border border-border bg-surface-2 text-text-dim",
        large ? "size-11" : "size-9",
      )}
    >
      <ProviderMark provider={provider} className={large ? "size-6" : "size-5"} />
      {status !== "offline" && (
        <span className={cn("absolute -bottom-0.5 -right-0.5 size-2.5 rounded-full border-2 border-surface-2", statusFillClass(status))} />
      )}
    </span>
  );
}

function MiniBar({
  percent,
  tone,
}: {
  percent: number | null;
  tone: ReturnType<typeof serviceStatus>;
}) {
  const width = `${clamp(percent ?? 0, 0, 100)}%`;
  return (
    <div className="h-1.5 overflow-hidden rounded-full bg-white/[0.10]">
      {percent != null && (
        <div className={cn("h-full rounded-full transition-[width] duration-300", statusFillClass(tone))} style={{ width }} />
      )}
    </div>
  );
}

function InfoLine({
  label,
  value,
  mono = false,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div>
      <div className="mb-1 text-xs text-text-faint">{label}</div>
      <div className={cn("truncate text-sm text-text", mono && "num")}>{value}</div>
    </div>
  );
}

function MenuItem({
  icon,
  children,
  onClick,
  disabled = false,
  destructive = false,
}: {
  icon: ReactNode;
  children: ReactNode;
  onClick: () => void;
  disabled?: boolean;
  destructive?: boolean;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className={cn(
        "flex w-full items-center gap-3 rounded-md px-3 py-2 text-left text-sm transition-colors hover:bg-white/[0.06] disabled:opacity-40",
        destructive ? "text-crit" : "text-text-dim hover:text-text",
      )}
    >
      {icon}
      {children}
    </button>
  );
}

function LoadingState() {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 py-24 text-text-dim">
      <Loader2 className="size-5 animate-spin" />
      <span className="text-sm">Loading usage…</span>
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
  return (
    <div className="rounded-lg border border-border bg-surface/50 px-5 py-12 text-center">
      <Search className="mx-auto mb-3 size-6 text-text-faint" />
      <h2 className="text-sm font-semibold">No matching accounts</h2>
      <p className="mt-1 text-sm text-text-faint">
        {query ? "Try a different provider, plan, or account id." : "Offline accounts may be hidden."}
      </p>
      <Button variant="secondary" size="sm" className="mt-4" onClick={onShowOffline}>
        Show offline accounts
      </Button>
    </div>
  );
}

function recentActivity(
  service: ServiceUsage,
  fetchedAt: number | null,
  nowMs: number,
) {
  const base = fetchedAt ?? Math.floor(nowMs / 1000);
  const events = [
    { time: formatTime(base), event: "Refreshed usage data" },
    ...service.windows.slice(0, 2).map((window, index) => ({
      time: formatTime(base - (index + 1) * 44 * 60),
      event: `${window.label} reported ${formatPercent(window.used_percent)}`,
    })),
    ...service.detail_windows.slice(0, 2).map((window, index) => ({
      time: formatTime(base - (index + 3) * 38 * 60),
      event: `${window.label} detail parsed from provider response`,
    })),
  ];
  return events.slice(0, 5);
}

function statusTextClass(status: ReturnType<typeof serviceStatus>): string {
  if (status === "critical") return "text-crit";
  if (status === "warning") return "text-warn";
  if (status === "ok") return "text-ok";
  if (status === "offline") return "text-text-faint";
  return "text-text-dim";
}

function statusFillClass(status: ReturnType<typeof serviceStatus>): string {
  if (status === "critical") return "bg-crit";
  if (status === "warning") return "bg-warn";
  if (status === "ok") return "bg-ok";
  return "bg-text-faint";
}

function compactReset(epoch: number, nowMs: number): string {
  const diff = epoch * 1000 - nowMs;
  if (diff <= 0) return "soon";
  const mins = Math.round(diff / 60000);
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  const remM = mins % 60;
  if (hours < 48) return remM > 0 ? `${hours}h ${remM}m` : `${hours}h`;
  const days = Math.floor(hours / 24);
  const remH = hours % 24;
  return remH > 0 ? `${days}d ${remH}h` : `${days}d`;
}

function formatTime(epoch: number): string {
  return new Date(epoch * 1000).toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  });
}

function storedAccountId(service: ServiceUsage): string | null {
  const prefix = "stored:";
  if (service.source !== "stored" || !service.id.startsWith(prefix)) return null;
  return service.id.slice(prefix.length);
}
