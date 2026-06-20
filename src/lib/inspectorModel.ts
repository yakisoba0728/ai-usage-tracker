import { formatUsedLimit } from "@/lib/format";
import {
  providerDisplayName,
  providerIndex,
  resolveHeadlineWindow,
} from "@/lib/providers";
import { serviceStatus, type ServiceStatus } from "@/lib/status";
import type { AppConfig, LimitWindow, ServiceUsage } from "@/lib/types";

export interface AccountSectionOptions {
  query: string;
  showOffline: boolean;
  sortBy?: "custom" | "usage" | "name";
}

export interface AccountRow {
  id: string;
  service: ServiceUsage;
  title: string;
  subtitle: string | null;
  providerName: string;
  status: ServiceStatus;
  statusLabel: string;
  headline: LimitWindow | null;
  headlinePercent: number | null;
  resetLabel: string | null;
  usedLimit: string | null;
}

export interface AccountSection {
  key: "online" | "offline";
  title: string;
  count: number;
  rows: AccountRow[];
}

export interface InspectorMetric {
  label: string;
  percent: number | null;
  usedLimit: string | null;
  resetLabel: string | null;
}

export interface InspectorSummary {
  title: string;
  accountId: string;
  sourceLabel: string;
  status: string;
  overallPercent: number | null;
  resetLabel: string | null;
  primaryUsedLimit: string | null;
  metricCards: InspectorMetric[];
}

const SECTION_META: Record<
  AccountSection["key"],
  { title: string; order: number }
> = {
  online: { title: "Online", order: 0 },
  offline: { title: "Offline", order: 1 },
};

export function buildAccountSections(
  services: ServiceUsage[],
  config: AppConfig | null,
  options: AccountSectionOptions,
): AccountSection[] {
  const query = normalizeQuery(options.query);
  const rows = services
    .filter((service) => options.showOffline || service.connected)
    .map((service) => toAccountRow(service, config))
    .filter((row) => rowMatchesQuery(row, query));

  const grouped = new Map<AccountSection["key"], AccountRow[]>();
  for (const row of rows) {
    const key = sectionKeyForStatus(row.status);
    const sectionRows = grouped.get(key) ?? [];
    sectionRows.push(row);
    grouped.set(key, sectionRows);
  }

  return Array.from(grouped.entries())
    .map(([key, sectionRows]) => {
      sectionRows.sort((a, b) =>
        compareAccountRows(a, b, options.sortBy ?? "usage", config),
      );
      return {
        key,
        title: SECTION_META[key].title,
        count: sectionRows.length,
        rows: sectionRows,
      };
    })
    .sort((a, b) => SECTION_META[a.key].order - SECTION_META[b.key].order);
}

export function selectVisibleServiceId(
  currentId: string | null,
  sections: AccountSection[],
): string | null {
  if (!currentId) return null;
  const rows = sections.flatMap((section) => section.rows);
  return rows.some((row) => row.id === currentId) ? currentId : null;
}

export function buildInspectorSummary(
  service: ServiceUsage,
  config: AppConfig | null,
  nowMs: number,
): InspectorSummary {
  const headline = resolveHeadlineWindow(service, config);
  const detail = service.detail_windows ?? [];
  const metricSource = detail.length > 0 ? detail : service.windows;

  return {
    title: providerDisplayName(config, service.provider),
    accountId: service.account ?? service.id,
    sourceLabel:
      service.source === "stored" ? "Stored credential" : "Local session",
    status: statusLabel(serviceStatus(service, config)),
    overallPercent: headline?.used_percent ?? null,
    resetLabel: formatResetShort(headline?.resets_at ?? null, nowMs),
    primaryUsedLimit: headline ? formatUsedLimit(headline) : null,
    metricCards: metricSource.map((window) => ({
      label: window.label,
      percent: window.used_percent,
      usedLimit: formatUsedLimit(window),
      resetLabel: formatResetShort(window.resets_at, nowMs),
    })),
  };
}

function toAccountRow(
  service: ServiceUsage,
  config: AppConfig | null,
): AccountRow {
  const headline = resolveHeadlineWindow(service, config);
  const providerName = providerDisplayName(config, service.provider);
  const status = serviceStatus(service, config);
  return {
    id: service.id,
    service,
    title: providerName,
    subtitle: service.account,
    providerName,
    status,
    statusLabel: statusLabel(status),
    headline,
    headlinePercent: headline?.used_percent ?? null,
    resetLabel: null,
    usedLimit: headline ? formatUsedLimit(headline) : null,
  };
}

function sectionKeyForStatus(status: ServiceStatus): AccountSection["key"] {
  return status === "offline" ? "offline" : "online";
}

function compareAccountRows(
  a: AccountRow,
  b: AccountRow,
  sortBy: NonNullable<AccountSectionOptions["sortBy"]>,
  config: AppConfig | null,
): number {
  if (sortBy === "name") return a.title.localeCompare(b.title);
  if (sortBy === "custom" && config) {
    const aSort = config.providers[providerIndex(a.service.provider)].sort_index;
    const bSort = config.providers[providerIndex(b.service.provider)].sort_index;
    if (aSort !== bSort) return aSort - bSort;
  }
  const aPct = a.headlinePercent ?? -1;
  const bPct = b.headlinePercent ?? -1;
  if (aPct !== bPct) return bPct - aPct;
  return a.title.localeCompare(b.title);
}

function normalizeQuery(query: string): string {
  return query.trim().toLocaleLowerCase();
}

function rowMatchesQuery(row: AccountRow, query: string): boolean {
  if (!query) return true;
  return [
    row.title,
    row.subtitle,
    row.providerName,
    row.service.provider,
    row.service.plan,
    row.service.id,
  ]
    .filter(Boolean)
    .some((value) => String(value).toLocaleLowerCase().includes(query));
}

function statusLabel(status: ServiceStatus): string {
  switch (status) {
    case "critical":
      return "Critical";
    case "warning":
      return "Warning";
    case "ok":
      return "Healthy";
    case "offline":
      return "Offline";
    default:
      return "Unknown";
  }
}

function formatResetShort(epoch: number | null, nowMs: number): string | null {
  if (epoch == null) return null;
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
