import { cn } from "@/lib/utils";
import {
  formatPercent,
  formatReset,
  formatUsedLimit,
  percentBarColor,
  percentColor,
} from "@/lib/format";
import type { LimitWindow } from "@/lib/types";

export interface BarGaugeProps {
  window: LimitWindow;
  nowMs: number;
  /** Show the used/limit + reset meta line (modal); hide for compact card rows. */
  showMeta?: boolean;
}

/**
 * Slim horizontal gauge: label, a big colored percent, a severity bar, and an
 * optional meta line (used/limit + live reset countdown). Powers both the
 * detail modal rows and the compact secondary windows on each card.
 */
export function BarGauge({ window: w, nowMs, showMeta = true }: BarGaugeProps) {
  const pct = w.used_percent;
  const usedLimit = formatUsedLimit(w);
  const reset = showMeta ? formatReset(w.resets_at, nowMs) : null;

  return (
    <div className="min-w-0">
      <div className="flex items-baseline justify-between gap-2">
        <span
          className="truncate text-[13px] font-medium text-card-foreground/90"
          title={w.label}
        >
          {w.label}
        </span>
        <span
          className={cn(
            "shrink-0 font-mono text-[13px] font-semibold tabular-nums",
            percentColor(pct),
          )}
        >
          {formatPercent(pct)}
        </span>
      </div>

      <div className="mt-2 h-1.5 w-full overflow-hidden rounded-full bg-white/[0.06]">
        <div
          className={cn(
            "h-full rounded-full transition-all duration-500 ease-out",
            percentBarColor(pct),
          )}
          style={{ width: `${Math.min(100, Math.max(0, pct ?? 0))}%` }}
        />
      </div>

      {showMeta && (
        <div className="mt-2 flex min-h-[16px] items-center justify-between gap-x-3 text-[11px] text-muted-foreground">
          {usedLimit && <span className="font-mono tabular-nums">{usedLimit}</span>}
          {reset && <span className="tabular-nums">{reset}</span>}
        </div>
      )}
    </div>
  );
}
