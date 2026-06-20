import type { TFunction } from "i18next";

import { formatResetShort, formatUsedLimit } from "@/lib/format";
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
  headline: LimitWindow | null;
  headlinePercent: number | null;
  resetLabel: string | null;
  usedLimit: string | null;
}

export interface AccountSection {
  key: "online" | "offline";
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
  overallPercent: number | null;
  resetLabel: string | null;
  primaryUsedLimit: string | null;
  metricCards: InspectorMetric[];
}

const SECTION_ORDER: Record<AccountSection["key"], number> = {
  online: 0,
  offline: 1,
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
      return { key, count: sectionRows.length, rows: sectionRows };
    })
    .sort((a, b) => SECTION_ORDER[a.key] - SECTION_ORDER[b.key]);
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
  t: TFunction,
): InspectorSummary {
  const headline = resolveHeadlineWindow(service, config);
  const detail = service.detail_windows ?? [];
  const metricSource = detail.length > 0 ? detail : service.windows;

  return {
    title: providerDisplayName(config, service.provider),
    accountId: service.account ?? service.id,
    overallPercent: headline?.used_percent ?? null,
    resetLabel: formatResetShort(headline?.resets_at ?? null, nowMs, t),
    primaryUsedLimit: headline ? formatUsedLimit(headline) : null,
    metricCards: metricSource.map((window) => ({
      label: window.label,
      percent: window.used_percent,
      usedLimit: formatUsedLimit(window),
      resetLabel: formatResetShort(window.resets_at, nowMs, t),
    })),
  };
}

function toAccountRow(
  service: ServiceUsage,
  config: AppConfig | null,
): AccountRow {
  const headline = resolveHeadlineWindow(service, config);
  const providerName = providerDisplayName(config, service.provider);
  return {
    id: service.id,
    service,
    title: providerName,
    subtitle: service.account,
    providerName,
    status: serviceStatus(service, config),
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
