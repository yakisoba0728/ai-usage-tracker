import { clamp } from "@/lib/utils";
import {
  formatPercent,
  percentSeverity,
  severityColor,
} from "@/lib/format";

export interface RingGaugeProps {
  /** 0–100, or null when there is no usage figure at all. */
  percent: number | null;
  /** Accessible label (the window label, e.g. "5-hour usage"). */
  label: string;
  /** Diameter in px. Defaults to 120 (the card headline). */
  size?: number;
  /** Render the center value (default true). False → bare arc (fleet chips). */
  showValue?: boolean;
}

/**
 * The signature ring gauge. A proportional arc fills clockwise from 12 o'clock,
 * colored by severity; the headline percent sits dead center in mono numerics.
 * Animated fill via `.ring-arc` (disabled under prefers-reduced-motion).
 */
export function RingGauge({
  percent,
  label,
  size = 120,
  showValue = true,
}: RingGaugeProps) {
  const r = 50;
  const cx = 60;
  const cy = 60;
  const circumference = 2 * Math.PI * r;
  const value = percent == null ? 0 : clamp(percent, 0, 100);
  const dashoffset = circumference * (1 - value / 100);
  const stroke = severityColor(percentSeverity(percent));
  const shown = percent != null;

  return (
    <div
      className="relative shrink-0"
      style={{ width: size, height: size }}
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={shown ? Math.round(percent as number) : undefined}
      aria-valuetext={shown ? `${Math.round(percent as number)} percent` : "no data"}
    >
      <svg
        viewBox="0 0 120 120"
        width={size}
        height={size}
        fill="none"
        className="block"
      >
        <circle
          cx={cx}
          cy={cy}
          r={r}
          stroke="var(--border-strong)"
          strokeWidth="7"
        />
        <circle
          className="ring-arc"
          cx={cx}
          cy={cy}
          r={r}
          stroke={stroke}
          strokeWidth="7"
          strokeLinecap="round"
          strokeDasharray={circumference}
          strokeDashoffset={shown ? dashoffset : circumference}
          transform="rotate(-90 60 60)"
        />
      </svg>
      {showValue && (
        <div className="pointer-events-none absolute inset-0 flex flex-col items-center justify-center">
          <span className="num leading-none tracking-tight text-text" style={{ fontSize: size * 0.3 }}>
            {formatPercent(percent).replace("%", "")}
            <span className="text-text-faint" style={{ fontSize: size * 0.135 }}>
              %
            </span>
          </span>
        </div>
      )}
    </div>
  );
}
