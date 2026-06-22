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
  // Reuse the single severity→status mapping (F-6) so the two can't drift.
  return severityToStatus(percentSeverity(pct));
}

export function severityToStatus(severity: Severity | null): ServiceStatus {
  if (severity === "crit") return "critical";
  if (severity === "warn") return "warning";
  if (severity === "ok") return "ok";
  return "unknown";
}

// percent → tone: the inner composition AccountCard/LimitsTab both have a bare
// percent (not a full service), so they share this instead of re-spelling
// `severityToStatus(percentSeverity(...))` (the real F-6-style duplication).
export function toneForPercent(p: number | null): ServiceStatus {
  return severityToStatus(percentSeverity(p));
}
