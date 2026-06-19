import { RingGauge } from "@/components/RingGauge";
import { ProviderMark, PROVIDER_LABEL } from "@/components/ProviderMark";
import { cn } from "@/lib/utils";
import {
  formatPercent,
  percentSeverity,
  severityTextClass,
} from "@/lib/format";
import type { Provider, ServiceUsage } from "@/lib/types";

/** The headline percent for a service = its primary window's used_percent. */
function primaryPercent(s: ServiceUsage): number | null {
  return s.windows?.[0]?.used_percent ?? null;
}

export interface FleetBarProps {
  services: ServiceUsage[];
  onSelect: (provider: Provider) => void;
}

/**
 * The signature at-a-glance summary: one compact chip per connected provider,
 * each with a tiny severity ring + mark + label + burn %. The highest-burning
 * chip carries the accent ring so the eye lands there first.
 */
export function FleetBar({ services, onSelect }: FleetBarProps) {
  if (services.length === 0) return null;

  let peakProvider: Provider | null = null;
  let peak: number | null = null;
  for (const s of services) {
    const p = primaryPercent(s);
    if (p != null && (peak == null || p > peak)) {
      peak = p;
      peakProvider = s.provider;
    }
  }

  return (
    <div className="flex flex-wrap items-center gap-2 pb-4">
      {services.map((s) => {
        const pct = primaryPercent(s);
        const isPeak = peakProvider === s.provider;
        return (
          <button
            key={s.provider}
            type="button"
            onClick={() => onSelect(s.provider)}
            aria-label={`${PROVIDER_LABEL[s.provider]} — ${formatPercent(pct)} usage. View details.`}
            className={cn(
              "group flex items-center gap-2 rounded-lg border bg-surface px-2.5 py-1.5",
              "transition-[background-color,border-color,transform] duration-150",
              "hover:-translate-y-px hover:bg-surface-2 lift-on-hover",
              isPeak
                ? "border-signal/50 shadow-[0_0_0_1px_var(--accent-line),0_8px_24px_-12px_rgba(57,208,216,0.5)]"
                : "border-border hover:border-border-strong",
            )}
          >
            <RingGauge
              percent={pct}
              label={`${PROVIDER_LABEL[s.provider]} usage`}
              size={18}
              showValue={false}
            />
            <ProviderMark provider={s.provider} className="size-3.5 text-text-dim group-hover:text-text" />
            <span className="text-text" style={{ fontSize: 12, fontWeight: 500 }}>
              {PROVIDER_LABEL[s.provider]}
            </span>
            <span
              className={cn(
                "num font-semibold",
                severityTextClass(percentSeverity(pct)),
              )}
              style={{ fontSize: 12 }}
            >
              {formatPercent(pct)}
            </span>
          </button>
        );
      })}
    </div>
  );
}
