import type { ServiceStatus } from "@/lib/status";
import type { LimitWindow, ServiceUsage } from "@/lib/types";

/** Text color utility for a status/tone band. */
export function statusTextClass(status: ServiceStatus): string {
  if (status === "critical") return "text-crit";
  if (status === "warning") return "text-warn";
  if (status === "ok") return "text-ok";
  if (status === "offline") return "text-text-faint";
  return "text-text-dim";
}

/** Solid fill utility for a status/tone band. */
export function statusFillClass(status: ServiceStatus): string {
  if (status === "critical") return "bg-crit";
  if (status === "warning") return "bg-warn";
  if (status === "ok") return "bg-ok";
  return "bg-text-faint";
}

/** The stored-account id for a `stored:` service, or null for auto-detected. */
export function storedAccountId(service: ServiceUsage): string | null {
  const prefix = "stored:";
  if (service.source !== "stored" || !service.id.startsWith(prefix)) return null;
  return service.id.slice(prefix.length);
}

/** All windows for a service (card primary + detail), in display order. */
export function allServiceWindows(service: ServiceUsage): LimitWindow[] {
  return [...(service.windows ?? []), ...(service.detail_windows ?? [])];
}
