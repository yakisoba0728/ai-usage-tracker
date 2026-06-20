import { useEffect, useState, type ReactNode } from "react";
import {
  Cloud,
  Edit3,
  MoreHorizontal,
  RefreshCw,
  Settings,
  Trash2,
} from "lucide-react";
import { useTranslation } from "react-i18next";

import { ProviderIconTile } from "@/components/dashboard/ProviderIconTile";
import {
  statusFillClass,
  statusTextClass,
  storedAccountId,
} from "@/components/dashboard/helpers";
import {
  INSPECTOR_TABS,
  type InspectorTab,
} from "@/components/dashboard/inspectorTabs";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  formatPercent,
  formatResetShort,
  formatUpdatedAgo,
  formatUsedLimit,
} from "@/lib/format";
import {
  buildInspectorSummary,
  type InspectorMetric,
} from "@/lib/inspectorModel";
import {
  PROVIDER_ORDER,
  providerDisplayName,
} from "@/lib/providers";
import { serviceStatus, type ServiceStatus } from "@/lib/status";
import type {
  AppConfig,
  LimitWindow,
  ProviderConfig,
  ServiceUsage,
} from "@/lib/types";
import { cn, clamp } from "@/lib/utils";

export function AccountDetailDialog({
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
  const { t } = useTranslation();
  return (
    <Dialog open={open && service != null} onOpenChange={onOpenChange}>
      <DialogContent className="h-[min(760px,86dvh)] w-[min(860px,94vw)] max-w-none gap-0 overflow-hidden rounded-xl border-border bg-[#1b1d20] p-0 shadow-2xl shadow-black/50">
        {service && (
          <>
            <DialogTitle className="sr-only">
              {t("detail.srTitle", {
                provider: providerDisplayName(config, service.provider),
              })}
            </DialogTitle>
            <DialogDescription className="sr-only">
              {t("detail.srDesc")}
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
  const { t } = useTranslation();
  const summary = buildInspectorSummary(service, config, nowMs, t);
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
                  {service.connected ? t("detail.connected") : t("detail.offline")}
                </span>
              </div>
              <div className="num mt-2 grid gap-x-5 gap-y-1 text-xs text-text-faint sm:grid-cols-2">
                <span>{t("detail.accountId")}&nbsp;&nbsp;{summary.accountId}</span>
                <span>
                  {t("detail.source")}&nbsp;&nbsp;
                  {service.source === "stored"
                    ? t("detail.sessions.storedCredential")
                    : t("detail.sessions.autoDetected")}
                </span>
                <span>
                  {t("detail.lastRefresh")}&nbsp;&nbsp;
                  {formatUpdatedAgo(fetchedAt, nowMs, t, { prefix: false })}
                </span>
                <span>{t("detail.plan")}&nbsp;&nbsp;{service.plan ?? "—"}</span>
              </div>
            </div>
          </div>

          <div className="relative flex shrink-0 items-center gap-2">
            <Button variant="ghost" size="sm" onClick={() => onTabChange("settings")}>
              <Edit3 className="size-4" />
              {t("detail.edit")}
            </Button>
            <Button
              variant="ghost"
              size="icon"
              onClick={() => onMoreOpenChange(!moreOpen)}
              aria-label={t("detail.moreActions")}
            >
              <MoreHorizontal className="size-4" />
            </Button>
            {moreOpen && (
              <div className="menu-pop absolute right-0 top-10 z-40 w-56 rounded-lg border border-border-strong bg-[#25272b]/95 p-1.5 shadow-2xl shadow-black/40">
                <MenuItem icon={<RefreshCw className={refreshing ? "size-4 animate-spin" : "size-4"} />} onClick={onRefresh}>
                  {t("detail.menu.refresh")}
                </MenuItem>
                <MenuItem icon={<Cloud className="size-4" />} onClick={onOpenAdd}>
                  {t("detail.menu.reauth")}
                </MenuItem>
                <MenuItem icon={<Settings className="size-4" />} onClick={onOpenSettings}>
                  {t("detail.menu.settings")}
                </MenuItem>
                <MenuItem
                  icon={<Trash2 className="size-4" />}
                  disabled={accountId == null}
                  destructive
                  onClick={onRemove}
                >
                  {t("detail.menu.remove")}
                </MenuItem>
              </div>
            )}
          </div>
        </div>

        <div className="mt-5 flex gap-5 overflow-x-auto">
          {INSPECTOR_TABS.map((id) => (
            <button
              key={id}
              type="button"
              onClick={() => onTabChange(id)}
              className={cn(
                "border-b-2 pb-3 text-sm font-medium transition-colors",
                tab === id
                  ? "border-[#4b9bea] text-text"
                  : "border-transparent text-text-dim hover:text-text",
              )}
            >
              {t(`detail.tab.${id}`)}
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
        {tab === "raw" && <RawTab service={service} />}
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
}: {
  service: ServiceUsage;
  summary: ReturnType<typeof buildInspectorSummary>;
  metrics: InspectorMetric[];
}) {
  const { t } = useTranslation();
  const status = serviceStatus(service, null);
  return (
    <div className="space-y-5">
      <div className="grid gap-4 sm:grid-cols-2">
        <OverviewStat
          label={t("detail.overview.overallUsage")}
          value={formatPercent(summary.overallPercent)}
          subvalue={summary.primaryUsedLimit ?? t("detail.overview.noLimit")}
          tone={status}
        />
        <OverviewStat
          label={t("detail.overview.resetsIn")}
          value={summary.resetLabel ?? "—"}
          subvalue={
            summary.resetLabel
              ? t("detail.overview.currentWindow")
              : t("detail.overview.noReset")
          }
        />
      </div>

      <div className="grid gap-3 xl:grid-cols-3">
        {metrics.length > 0 ? (
          metrics
            .slice(0, 3)
            .map((metric) => <MetricCard key={metric.label} metric={metric} />)
        ) : (
          <div className="rounded-lg border border-border bg-surface/60 p-4 text-sm text-text-faint xl:col-span-3">
            {t("detail.overview.noDetailWindows")}
          </div>
        )}
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
  tone?: ServiceStatus;
}) {
  return (
    <div className="rounded-lg border border-border bg-surface/50 p-4">
      <div className="mb-3 text-xs font-medium text-text-faint">{label}</div>
      <div className={cn("num text-2xl font-semibold", statusTextClass(tone))}>
        {value}
      </div>
      <div className="num mt-1 text-sm text-text-dim">{subvalue}</div>
    </div>
  );
}

function MetricCard({ metric }: { metric: InspectorMetric }) {
  const { t } = useTranslation();
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
        <span className="ml-1 text-xs font-medium text-text-dim">
          {t("detail.metric.used")}
        </span>
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
        {metric.resetLabel
          ? t("detail.metric.resetsIn", { time: metric.resetLabel })
          : t("detail.metric.noReset")}
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
  const { t } = useTranslation();
  if (windows.length === 0) {
    return (
      <div className="rounded-lg border border-border bg-surface/50 p-5 text-sm text-text-faint">
        {t("detail.limits.noWindows")}
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
  const { t } = useTranslation();
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
            {formatUsedLimit(window) ?? t("detail.limits.noUsedLimit")}
          </p>
        </div>
        <span className={cn("num text-lg font-semibold", statusTextClass(tone))}>
          {formatPercent(percent)}
        </span>
      </div>
      <MiniBar percent={percent} tone={tone} />
      <p className="num mt-3 text-xs text-text-faint">
        {window.resets_at
          ? t("detail.limits.resetsIn", {
              time: formatResetShort(window.resets_at, nowMs, t),
            })
          : t("detail.limits.noResetReported")}
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
  const { t } = useTranslation();
  return (
    <div className="space-y-4">
      <div className="rounded-lg border border-border bg-surface/50 p-4">
        <div className="grid gap-4 sm:grid-cols-2">
          <InfoLine
            label={t("detail.sessions.source")}
            value={
              service.source === "stored"
                ? t("detail.sessions.storedCredential")
                : t("detail.sessions.autoDetected")
            }
          />
          <InfoLine
            label={t("detail.sessions.provider")}
            value={service.provider}
            mono
          />
          <InfoLine
            label={t("detail.sessions.account")}
            value={service.account ?? t("detail.sessions.notReported")}
          />
          <InfoLine
            label={t("detail.sessions.lastParsed")}
            value={formatUpdatedAgo(fetchedAt, nowMs, t, { prefix: false })}
            mono
          />
        </div>
      </div>

      <div className="flex flex-wrap gap-2">
        <Button variant="outline" onClick={onRefresh}>
          <RefreshCw className="size-4" />
          {t("detail.sessions.reuseLocal")}
        </Button>
        <Button variant="secondary" onClick={onOpenAdd}>
          <Cloud className="size-4" />
          {t("detail.sessions.reauth")}
        </Button>
      </div>
    </div>
  );
}

function RawTab({ service }: { service: ServiceUsage }) {
  const { t } = useTranslation();
  if (!service.raw_response) {
    return (
      <div className="rounded-lg border border-border bg-surface/50 p-5 text-sm text-text-faint">
        {t("detail.raw.empty")}
      </div>
    );
  }
  return (
    <pre className="scroll-area max-h-[460px] overflow-auto rounded-lg border border-border bg-canvas p-4 text-xs leading-relaxed text-text-dim">
      <code className="num">{service.raw_response}</code>
    </pre>
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
  const { t } = useTranslation();
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
        <h2 className="mb-4 text-sm font-semibold">
          {t("detail.settings.providerDisplay")}
        </h2>
        <label className="block">
          <span className="mb-1.5 block text-xs text-text-faint">
            {t("detail.settings.displayName")}
          </span>
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
        <h2 className="mb-4 text-sm font-semibold">
          {t("detail.settings.primaryWindow")}
        </h2>
        <select
          value={providerConfig?.primary_window ?? ""}
          disabled={config == null || labels.length === 0}
          onChange={(event) => patch({ primary_window: event.target.value || null })}
          className="w-full rounded-md border border-border bg-canvas px-3 py-2 text-sm text-text outline-none focus:border-border-strong"
        >
          <option value="">{t("detail.settings.autoWindow")}</option>
          {labels.map((label) => (
            <option key={label} value={label}>
              {label}
            </option>
          ))}
        </select>
      </div>

      <div className="rounded-lg border border-border bg-surface/50 p-4">
        <h2 className="mb-4 text-sm font-semibold">
          {t("detail.settings.thresholds")}
        </h2>
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
            {t("detail.settings.add")}
          </Button>
        </div>
      </div>

      <div className="rounded-lg border border-border bg-surface/50 p-4">
        <h2 className="mb-3 text-sm font-semibold text-crit">
          {t("detail.settings.dangerZone")}
        </h2>
        <Button variant="destructive" onClick={onOpenAdd}>
          <Cloud className="size-4" />
          {t("detail.settings.reauthAccount")}
        </Button>
      </div>
    </div>
  );
}

function MiniBar({
  percent,
  tone,
}: {
  percent: number | null;
  tone: ServiceStatus;
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
