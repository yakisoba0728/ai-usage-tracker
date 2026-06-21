import { percentSeverity, type Severity } from "@/lib/format";
import { resolveHeadlineWindow } from "@/lib/providers";
import type { AppConfig, ServiceUsage } from "@/lib/types";

export type ServiceStatus = "critical" | "warning" | "ok" | "unknown" | "offline";

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
