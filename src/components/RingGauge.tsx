import { cn } from "@/lib/utils";
import { percentColor, percentStrokeColor } from "@/lib/format";

export interface RingGaugeProps {
  value: number | null;
  size?: number;
  strokeWidth?: number;
  className?: string;
}

/**
 * Circular headline gauge. The arc is colored by usage severity (ok/warn/crit),
 * the center renders the percentage in a mono face. Used as each card's hero.
 */
export function RingGauge({
  value,
  size = 88,
  strokeWidth = 8,
  className,
}: RingGaugeProps) {
  const hasValue = value != null;
  const clamped = Math.max(0, Math.min(100, value ?? 0));
  const radius = (size - strokeWidth) / 2;
  const circumference = 2 * Math.PI * radius;
  const offset = circumference * (1 - clamped / 100);
  const center = size / 2;

  return (
    <div
      className={cn("relative shrink-0", className)}
      style={{ width: size, height: size }}
    >
      <svg
        width={size}
        height={size}
        viewBox={`0 0 ${size} ${size}`}
        className="-rotate-90"
      >
        <circle
          cx={center}
          cy={center}
          r={radius}
          fill="none"
          strokeWidth={strokeWidth}
          className="stroke-white/[0.06]"
        />
        <circle
          cx={center}
          cy={center}
          r={radius}
          fill="none"
          strokeWidth={strokeWidth}
          strokeLinecap="round"
          strokeDasharray={circumference}
          strokeDashoffset={offset}
          className={cn(
            "transition-[stroke-dashoffset] duration-700 ease-out",
            percentStrokeColor(value),
          )}
        />
      </svg>
      <div className="absolute inset-0 flex flex-col items-center justify-center">
        <span
          className={cn(
            "font-mono text-[1.55rem] font-semibold leading-none tabular-nums",
            percentColor(value),
          )}
        >
          {hasValue ? Math.round(value) : "—"}
        </span>
        <span className="mt-0.5 text-[10px] font-medium tracking-wide text-muted-foreground">
          {hasValue ? "%" : "n/a"}
        </span>
      </div>
    </div>
  );
}
