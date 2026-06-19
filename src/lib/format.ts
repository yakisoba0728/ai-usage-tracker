import type { LimitWindow } from "@/lib/types";

/** Color band for a usage percent (text class). Thresholds match the gauges. */
export function percentColor(p: number | null): string {
  const v = p ?? 0;
  if (v >= 90) return "text-red-500";
  if (v >= 70) return "text-amber-500";
  return "text-emerald-500";
}

/** Color band for a usage percent (background class). */
export function percentBarColor(p: number | null): string {
  const v = p ?? 0;
  if (v >= 90) return "bg-red-500";
  if (v >= 70) return "bg-amber-500";
  return "bg-emerald-500";
}

/** "<used> / <limit>" with `$` when the window label looks monetary. */
export function formatUsedLimit(w: LimitWindow): string | null {
  if (w.used == null && w.limit == null) return null;
  const money = /extra|balance|plan usage|credits|spend/i.test(w.label);
  const fmt = (n: number | null) =>
    n == null ? "—" : money ? `$${n.toFixed(2)}` : n.toLocaleString();
  return `${fmt(w.used)} / ${fmt(w.limit)}`;
}

/** Human countdown + absolute time for an epoch-seconds reset, relative to nowMs. */
export function formatReset(epoch: number | null, nowMs: number): string | null {
  if (epoch == null) return null;
  const ms = epoch * 1000;
  const diff = ms - nowMs;
  const abs = new Date(ms).toLocaleString();
  if (diff <= 0) return `resets soon · ${abs}`;
  const mins = Math.round(diff / 60000);
  if (mins < 60) return `resets in ${mins}m · ${abs}`;
  const hours = Math.floor(mins / 60);
  const remM = mins % 60;
  if (hours < 48) return `resets in ${hours}h ${remM}m · ${abs}`;
  const days = Math.floor(hours / 24);
  const remH = hours % 24;
  return `resets in ${days}d ${remH}h · ${abs}`;
}
