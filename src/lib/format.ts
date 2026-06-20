import type { LimitWindow } from "@/lib/types";

export type Severity = "ok" | "warn" | "crit";

/** Severity band for a usage percent. `null` when there is no percent at all. */
export function percentSeverity(p: number | null): Severity | null {
  if (p == null) return null;
  if (p > 85) return "crit";
  if (p >= 60) return "warn";
  return "ok";
}

/** Solid fill utility for a usage percent (bars, dots). */
export function severityBarClass(sev: Severity | null): string {
  switch (sev) {
    case "crit":
      return "bg-crit";
    case "warn":
      return "bg-warn";
    case "ok":
      return "bg-ok";
    default:
      return "bg-text-faint/40";
  }
}

/** "72%" or "—" when there is no value; whole-number percent. */
export function formatPercent(p: number | null): string {
  if (p == null) return "—";
  return `${Math.round(p)}%`;
}

/** "<used> / <limit>" with `$` when the window label looks monetary. */
export function formatUsedLimit(w: LimitWindow): string | null {
  if (w.used == null && w.limit == null) return null;
  const money = /extra|balance|plan usage|credits|spend/i.test(w.label);
  const fmt = (n: number | null) =>
    n == null ? "—" : money ? `$${n.toFixed(2)}` : n.toLocaleString();
  return `${fmt(w.used)} / ${fmt(w.limit)}`;
}

/** Live countdown only (e.g. "resets in 3h 19m"). null when there is no reset. */
export function formatResetCountdown(
  epoch: number | null,
  nowMs: number,
): string | null {
  if (epoch == null) return null;
  const diff = epoch * 1000 - nowMs;
  if (diff <= 0) return "resets soon";
  const mins = Math.round(diff / 60000);
  if (mins < 60) return `resets in ${mins}m`;
  const hours = Math.floor(mins / 60);
  const remM = mins % 60;
  if (hours < 48) return `resets in ${hours}h ${remM}m`;
  const days = Math.floor(hours / 24);
  const remH = hours % 24;
  return `resets in ${days}d ${remH}h`;
}

/** "Updated just now" / "Updated 3m 12s ago" / "Updated 1h 4m ago". */
export function formatUpdatedAgo(
  fetchedAtSec: number | null,
  nowMs: number,
): string {
  if (fetchedAtSec == null) return "Awaiting first update…";
  const elapsed = Math.max(0, Math.floor(nowMs / 1000) - fetchedAtSec);
  if (elapsed < 5) return "Updated just now";
  if (elapsed < 60) return `Updated ${elapsed}s ago`;
  const mins = Math.floor(elapsed / 60);
  const secs = elapsed % 60;
  if (mins < 60) return `Updated ${mins}m ${secs}s ago`;
  const hours = Math.floor(mins / 60);
  const remM = mins % 60;
  return `Updated ${hours}h ${remM}m ago`;
}
