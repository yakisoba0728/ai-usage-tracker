import { statusFillClass } from "@/components/dashboard/helpers";
import type { ServiceStatus } from "@/lib/status";
import { cn, clamp } from "@/lib/utils";

/**
 * Shared usage progress bar — the single source of truth for the track + fill
 * markup that the cards, popover, and detail modal all render. Exposes proper
 * `role="progressbar"` semantics so screen readers announce the usage value.
 */
export function UsageBar({
  percent,
  tone,
  size = "md",
  label,
}: {
  percent: number | null;
  tone: ServiceStatus;
  size?: "sm" | "md";
  label?: string;
}) {
  return (
    <div
      className={cn(
        "overflow-hidden rounded-full bg-white/[0.10]",
        size === "sm" ? "h-1" : "h-1.5",
      )}
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={percent != null ? Math.round(percent) : undefined}
    >
      {percent != null && (
        <div
          className={cn(
            "h-full rounded-full transition-[width] duration-300",
            statusFillClass(tone),
          )}
          style={{ width: `${clamp(percent, 0, 100)}%` }}
        />
      )}
    </div>
  );
}
