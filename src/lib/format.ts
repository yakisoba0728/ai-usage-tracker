import type { TFunction } from "i18next";

import type { LimitWindow, ServiceError } from "@/lib/types";

export type Severity = "ok" | "warn" | "crit";

/** Severity band for a usage percent. `null` when there is no percent at all. */
export function percentSeverity(p: number | null): Severity | null {
  if (p == null) return null;
  if (p > 85) return "crit";
  if (p >= 60) return "warn";
  return "ok";
}

/** "72%" or "—" when there is no value; whole-number percent. */
export function formatPercent(p: number | null): string {
  if (p == null) return "—";
  return `${Math.round(p)}%`;
}

/**
 * Remaining headroom for a usage percent: `100 − used`, clamped to [0, 100].
 * `null` when there is no value. The UI displays remaining ("how much is left")
 * rather than used — so an unused plan reads as 100%, an exhausted one as 0% —
 * while severity/colors stay keyed off the underlying *used* percent.
 */
export function remainingPercent(usedPercent: number | null): number | null {
  if (usedPercent == null) return null;
  return Math.max(0, Math.min(100, 100 - usedPercent));
}

/** "<used> / <limit>" with `$` when the window label looks monetary. */
export function formatUsedLimit(w: LimitWindow): string | null {
  if (w.used == null && w.limit == null) return null;
  const money = /extra|balance|plan usage|credits|spend/i.test(w.label);
  const fmt = (n: number | null) =>
    n == null ? "—" : money ? `$${n.toFixed(2)}` : n.toLocaleString();
  return `${fmt(w.used)} / ${fmt(w.limit)}`;
}

/**
 * Localized message for a structured service error. Maps the stable `code` to
 * `error.<code>`, falling back to the backend's English `detail` (then a
 * generic string) when no translation key matches that code.
 */
export function formatServiceError(error: ServiceError, t: TFunction): string {
  return t(`error.${error.code}`, {
    defaultValue: error.detail ?? t("error.unknown"),
  });
}

/**
 * Localized "time until reset" (e.g. "3h 19m" / "soon"). Compact form used in
 * cards and the detail modal. `null` when there is no reset epoch.
 */
export function formatResetShort(
  epoch: number | null,
  nowMs: number,
  t: TFunction,
): string | null {
  if (epoch == null) return null;
  const diff = epoch * 1000 - nowMs;
  if (diff <= 0) return t("time.reset.soon");
  const mins = Math.round(diff / 60000);
  // A sub-minute reset rounds to 0; show "soon" rather than a literal "0m" (F-4).
  if (mins < 1) return t("time.reset.soon");
  if (mins < 60) return t("time.reset.minutes", { m: mins });
  const hours = Math.floor(mins / 60);
  const remM = mins % 60;
  if (hours < 48) {
    return remM > 0
      ? t("time.reset.hoursMinutes", { h: hours, m: remM })
      : t("time.reset.hours", { h: hours });
  }
  const days = Math.floor(hours / 24);
  const remH = hours % 24;
  return remH > 0
    ? t("time.reset.daysHours", { d: days, h: remH })
    : t("time.reset.days", { d: days });
}

/**
 * Localized "updated X ago". With `prefix` (default) it includes the "Updated"
 * lead-in (footer/tray); without it, just "X ago" (detail header / sessions).
 */
export function formatUpdatedAgo(
  fetchedAtSec: number | null,
  nowMs: number,
  t: TFunction,
  opts?: { prefix?: boolean },
): string {
  const prefix = opts?.prefix ?? true;
  // null AND the 0 cold-start sentinel (and any bogus negative) mean "no real
  // snapshot yet" — otherwise fetched_at:0 renders "Updated 489000h ago" (F-1).
  if (fetchedAtSec == null || fetchedAtSec <= 0) return t("time.awaiting");
  const elapsed = Math.max(0, Math.floor(nowMs / 1000) - fetchedAtSec);
  if (elapsed < 5) return prefix ? t("time.justNow") : t("time.agoJustNow");
  if (elapsed < 60) {
    return t(prefix ? "time.updatedSeconds" : "time.agoSeconds", {
      count: elapsed,
    });
  }
  const mins = Math.floor(elapsed / 60);
  const secs = elapsed % 60;
  if (mins < 60) {
    return t(prefix ? "time.updatedMinutes" : "time.agoMinutes", {
      m: mins,
      s: secs,
    });
  }
  const hours = Math.floor(mins / 60);
  const remM = mins % 60;
  return t(prefix ? "time.updatedHours" : "time.agoHours", {
    h: hours,
    m: remM,
  });
}
