import type { LimitWindow } from "@/lib/types";

export type Severity = "ok" | "warn" | "crit";

/** Severity band for a usage percent; `null` when there is no percent at all. */
export function percentSeverity(p: number | null): Severity | null {
  if (p == null) return null;
  if (p >= 90) return "crit";
  if (p >= 70) return "warn";
  return "ok";
}

/** Text color class for a usage percent (semantic status tokens). */
export function percentColor(p: number | null): string {
  switch (percentSeverity(p)) {
    case "crit":
      return "text-crit";
    case "warn":
      return "text-warn";
    case "ok":
      return "text-ok";
    default:
      return "text-muted-foreground";
  }
}

/** Fill color class for a usage percent (semantic status tokens). */
export function percentBarColor(p: number | null): string {
  switch (percentSeverity(p)) {
    case "crit":
      return "bg-crit";
    case "warn":
      return "bg-warn";
    case "ok":
      return "bg-ok";
    default:
      return "bg-muted-foreground/40";
  }
}

/** Stroke color class for a usage percent (SVG ring). */
export function percentStrokeColor(p: number | null): string {
  switch (percentSeverity(p)) {
    case "crit":
      return "stroke-crit";
    case "warn":
      return "stroke-warn";
    case "ok":
      return "stroke-ok";
    default:
      return "stroke-muted-foreground/30";
  }
}

/** "72%" or "—" when there is no value; one decimal under 10 for fidelity. */
export function formatPercent(p: number | null): string {
  if (p == null) return "—";
  return `${p < 10 ? p.toFixed(1) : Math.round(p)}%`;
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

/**
 * Live countdown plus a concise absolute time
 * ("resets in 3h 19m · Jun 19, 14:32").
 */
export function formatReset(epoch: number | null, nowMs: number): string | null {
  const cd = formatResetCountdown(epoch, nowMs);
  if (cd == null || epoch == null) return null;
  const abs = new Date(epoch * 1000).toLocaleString([], {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
  return `${cd} · ${abs}`;
}
