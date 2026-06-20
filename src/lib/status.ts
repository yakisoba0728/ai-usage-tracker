import { percentSeverity, type Severity } from "@/lib/format";
import { resolveHeadlineWindow } from "@/lib/providers";
import type { AppConfig, Provider, ServiceUsage } from "@/lib/types";

export type ServiceStatus = "critical" | "warning" | "ok" | "unknown" | "offline";

export interface StatusSummary {
  total: number;
  connected: number;
  offline: number;
  critical: number;
  warning: number;
  ok: number;
  unknown: number;
  maxPercent: number | null;
  averagePercent: number | null;
  maxProvider: Provider | null;
}

export function serviceStatus(
  service: ServiceUsage,
  config: AppConfig | null,
): ServiceStatus {
  if (!service.connected) return "offline";
  const pct = resolveHeadlineWindow(service, config)?.used_percent ?? null;
  const severity = percentSeverity(pct);
  if (severity === "crit") return "critical";
  if (severity === "warn") return "warning";
  if (severity === "ok") return "ok";
  return "unknown";
}

export function severityToStatus(severity: Severity | null): ServiceStatus {
  if (severity === "crit") return "critical";
  if (severity === "warn") return "warning";
  if (severity === "ok") return "ok";
  return "unknown";
}

export function summarizeServices(
  services: ServiceUsage[],
  config: AppConfig | null,
): StatusSummary {
  let connected = 0;
  let offline = 0;
  let critical = 0;
  let warning = 0;
  let ok = 0;
  let unknown = 0;
  let maxPercent: number | null = null;
  let maxProvider: Provider | null = null;
  let totalPercent = 0;
  let percentCount = 0;

  for (const service of services) {
    const status = serviceStatus(service, config);
    if (status === "offline") {
      offline += 1;
    } else {
      connected += 1;
    }
    if (status === "critical") critical += 1;
    if (status === "warning") warning += 1;
    if (status === "ok") ok += 1;
    if (status === "unknown") unknown += 1;

    const pct = resolveHeadlineWindow(service, config)?.used_percent ?? null;
    if (pct != null) {
      totalPercent += pct;
      percentCount += 1;
      if (maxPercent == null || pct > maxPercent) {
        maxPercent = pct;
        maxProvider = service.provider;
      }
    }
  }

  return {
    total: services.length,
    connected,
    offline,
    critical,
    warning,
    ok,
    unknown,
    maxPercent,
    averagePercent: percentCount > 0 ? totalPercent / percentCount : null,
    maxProvider,
  };
}
