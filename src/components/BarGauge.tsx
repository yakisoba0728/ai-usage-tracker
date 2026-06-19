import { cn, clamp } from "@/lib/utils";
import {
  formatPercent,
  formatResetCountdown,
  formatUsedLimit,
  percentSeverity,
  severityBarClass,
} from "@/lib/format";
import type { LimitWindow } from "@/lib/types";

export interface BarGaugeProps {
  window: LimitWindow;
  nowMs: number;
  /** Primary = headline (6px, with reset countdown); secondary = compact (4px). */
  variant: "primary" | "secondary";
  /** Show the used/limit meta line (modal rows). Defaults to false. */
  showUsedLimit?: boolean;
}

/**
 * Pure horizontal gauge — the only place severity color appears. Label left,
 * white mono percent right, a flat severity-filled bar beneath. Primary carries
 * a reset countdown (and optional used/limit); secondary is a bare compact bar.
 * If there is no percent, only the "—" reads and no bar renders.
 */
export function BarGauge({
  window: w,
  nowMs,
  variant,
  showUsedLimit = false,
}: BarGaugeProps) {
  const pct = w.used_percent;
  const sev = percentSeverity(pct);
  const isPrimary = variant === "primary";
  const labelText = isPrimary ? "text-sm text-text-dim" : "text-xs text-text-faint";
  const pctText = isPrimary ? "text-md" : "text-xs";
  const barH = isPrimary ? "h-1.5" : "h-1";
  const width = `${clamp(pct ?? 0, 0, 100)}%`;
  const reset = isPrimary ? formatResetCountdown(w.resets_at, nowMs) : null;
  const usedLimit = showUsedLimit ? formatUsedLimit(w) : null;
  const hasValue = pct != null;

  return (
    <div className="min-w-0">
      <div className="flex items-baseline justify-between gap-2">
        <span className={cn("min-w-0 truncate", labelText)} title={w.label}>
          {w.label}
        </span>
        <span className={cn("num shrink-0 font-semibold text-text", pctText)}>
          {formatPercent(pct)}
        </span>
      </div>

      {hasValue && (
        <div
          className={cn("mt-2 w-full overflow-hidden rounded-full bg-surface-2", barH)}
          role="progressbar"
          aria-label={w.label}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={pct != null ? Math.round(pct) : undefined}
        >
          <div
            className={cn(
              "h-full rounded-full transition-[width] duration-200 ease-out",
              severityBarClass(sev),
            )}
            style={{ width }}
          />
        </div>
      )}

      {isPrimary && (reset || usedLimit) && (
        <div className="mt-1.5 flex min-h-[14px] items-center justify-between gap-x-3 text-text-faint text-xs">
          {usedLimit && <span className="num">{usedLimit}</span>}
          {reset && <span className="num">{reset}</span>}
        </div>
      )}
    </div>
  );
}
