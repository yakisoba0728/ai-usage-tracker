import { cn, clamp } from "@/lib/utils";
import {
  formatPercent,
  formatReset,
  formatUsedLimit,
  percentSeverity,
  severityBarClass,
  severityTextClass,
} from "@/lib/format";
import type { LimitWindow } from "@/lib/types";

export interface BarGaugeProps {
  window: LimitWindow;
  nowMs: number;
  /** Show the used/limit + reset meta line (modal); hide for compact card rows. */
  showMeta?: boolean;
}

/**
 * Slim horizontal gauge: label left, severity percent right, a thin severity
 * bar beneath, and an optional meta line (used/limit + live reset countdown).
 * Powers both the card's secondary windows and the modal rows.
 */
export function BarGauge({ window: w, nowMs, showMeta = true }: BarGaugeProps) {
  const pct = w.used_percent;
  const sev = percentSeverity(pct);
  const usedLimit = formatUsedLimit(w);
  const reset = showMeta ? formatReset(w.resets_at, nowMs) : null;
  const width = `${clamp(pct ?? 0, 0, 100)}%`;

  return (
    <div className="min-w-0">
      <div className="flex items-baseline justify-between gap-2">
        <span
          className="min-w-0 truncate text-text-dim"
          style={{ fontSize: 12 }}
          title={w.label}
        >
          {w.label}
        </span>
        <span
          className={cn(
            "num shrink-0 font-semibold",
            severityTextClass(sev),
          )}
          style={{ fontSize: 13 }}
        >
          {formatPercent(pct)}
        </span>
      </div>

      <div
        className="mt-2 h-1.5 w-full overflow-hidden rounded-full bg-border"
        role="progressbar"
        aria-label={w.label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={pct != null ? Math.round(pct) : undefined}
      >
        <div
          className={cn(
            "h-full rounded-full transition-[width] duration-500 ease-out",
            severityBarClass(sev),
          )}
          style={{ width }}
        />
      </div>

      {showMeta && (usedLimit || reset) && (
        <div className="mt-2 flex min-h-[15px] items-center justify-between gap-x-3 text-text-faint" style={{ fontSize: 11 }}>
          {usedLimit && <span className="num">{usedLimit}</span>}
          {reset && <span className="num">{reset}</span>}
        </div>
      )}
    </div>
  );
}
