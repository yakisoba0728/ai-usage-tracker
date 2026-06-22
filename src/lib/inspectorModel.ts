import { formatUsedLimit } from "@/lib/format";
import {
  providerConfigFor,
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
  subtitle: string;
  status: ServiceStatus;
  headline: LimitWindow | null;
  headlinePercent: number | null;
  usedLimit: string | null;
}

export interface AccountSection {
  key: "online" | "offline";
  count: number;
  rows: AccountRow[];
}

export interface InspectorSummary {
  title: string;
  accountId: string;
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
    .filter((service) => providerConfigFor(config, service.provider)?.enabled !== false)
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
): InspectorSummary {
  return {
    title: providerDisplayName(config, service.id, service.provider),
    accountId: accountSubtitle(service, config),
  };
}

/**
 * The subtitle shown under a card / in the inspector for ONE account. Precedence
 * (BUG-6): the provider-supplied account label → the per-account `custom_name`
 * → a synthesized "<provider label> · <short id>". A null-account stored service
 * must never leak its raw `stored:<uuid>` id, so the fallback strips the
 * source prefix and shows only a short tail.
 */
export function accountSubtitle(
  service: ServiceUsage,
  config: AppConfig | null,
): string {
  const account = service.account?.trim();
  if (account) return account;
  const custom = config?.accounts?.[service.id]?.custom_name?.trim();
  if (custom) return custom;
  const label = providerDisplayName(config, service.id, service.provider);
  const bareId = service.id.replace(/^(auto|stored):/, "");
  const shortId = bareId.slice(-6);
  return shortId ? `${label} · ${shortId}` : label;
}

function toAccountRow(
  service: ServiceUsage,
  config: AppConfig | null,
): AccountRow {
  const headline = resolveHeadlineWindow(service, config);
  return {
    id: service.id,
    service,
    title: providerDisplayName(config, service.id, service.provider),
    subtitle: accountSubtitle(service, config),
    status: serviceStatus(service, config),
    headline,
    headlinePercent: headline?.used_percent ?? null,
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
    row.service.provider,
    row.service.plan,
    row.service.id,
  ]
    .filter(Boolean)
    .some((value) => String(value).toLocaleLowerCase().includes(query));
}
