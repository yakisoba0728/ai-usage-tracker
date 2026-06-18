import { useEffect, useState } from "react";

import { Progress } from "@/components/ui/progress";
import { cn } from "@/lib/utils";

export interface GaugeProps {
  label: string;
  used_percent: number | null;
  resets_at: number | null;
}

type Severity = "ok" | "warn" | "crit";

function severityFor(percent: number | null): Severity | null {
  if (percent == null) return null;
  if (percent >= 70) return "crit";
  if (percent >= 40) return "warn";
  return "ok";
}

const SEVERITY_BAR: Record<Severity, string> = {
  ok: "bg-emerald-500",
  warn: "bg-amber-500",
  crit: "bg-red-500",
};

const SEVERITY_TEXT: Record<Severity, string> = {
  ok: "text-emerald-500",
  warn: "text-amber-500",
  crit: "text-red-500",
};

function formatCountdown(resetsAtSeconds: number, nowSeconds: number): string {
  const remaining = resetsAtSeconds - nowSeconds;
  if (remaining <= 0) return "Resets now";
  const h = Math.floor(remaining / 3600);
  const m = Math.floor((remaining % 3600) / 60);
  const s = remaining % 60;
  if (h > 0) return `Resets in ${h}h ${m}m`;
  if (m > 0) return `Resets in ${m}m ${s}s`;
  return `Resets in ${s}s`;
}

export function Gauge({ label, used_percent, resets_at }: GaugeProps) {
  // Tick once a second so the countdown stays live without depending on pushes.
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));
  useEffect(() => {
    const id = setInterval(() => setNow(Math.floor(Date.now() / 1000)), 1000);
    return () => clearInterval(id);
  }, []);

  const severity = severityFor(used_percent);
  const clamped = Math.max(0, Math.min(100, used_percent ?? 0));

  return (
    <div className="space-y-1.5">
      <div className="flex items-baseline justify-between gap-2">
        <span className="truncate text-sm text-muted-foreground" title={label}>
          {label}
        </span>
        <span
          className={cn(
            "shrink-0 text-sm font-semibold tabular-nums",
            severity ? SEVERITY_TEXT[severity] : "text-muted-foreground",
          )}
        >
          {used_percent == null ? "?" : `${Math.round(used_percent)}%`}
        </span>
      </div>

      <Progress
        value={clamped}
        className="h-1.5"
        indicatorClassName={
          severity ? SEVERITY_BAR[severity] : "bg-muted-foreground/40"
        }
      />

      <div className="h-4 text-xs text-muted-foreground">
        {resets_at != null && (
          <span className="tabular-nums">
            {formatCountdown(resets_at, now)}
          </span>
        )}
      </div>
    </div>
  );
}
