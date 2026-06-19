import { ChevronRight } from "lucide-react";

import { RingGauge } from "@/components/RingGauge";
import { BarGauge } from "@/components/BarGauge";
import { ProviderMark, PROVIDER_LABEL } from "@/components/ProviderMark";
import { CardError } from "@/components/ErrorState";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import { formatResetCountdown } from "@/lib/format";
import type { LimitWindow, Provider, ServiceUsage } from "@/lib/types";

export interface ProviderCardProps {
  service: ServiceUsage;
  nowMs: number;
  onOpen: (provider: Provider) => void;
}

/** Soonest-resetting window (smallest future resets_at), across all windows. */
function soonestReset(
  windows: LimitWindow[],
  nowMs: number,
): string | null {
  let best: number | null = null;
  for (const w of windows) {
    if (w.resets_at == null) continue;
    if (w.resets_at * 1000 <= nowMs) continue;
    if (best == null || w.resets_at < best) best = w.resets_at;
  }
  return formatResetCountdown(best, nowMs);
}

export function ProviderCard({ service, nowMs, onOpen }: ProviderCardProps) {
  const windows = service.windows ?? [];
  const primary = windows[0];
  const secondary = windows.slice(1);
  const reset = soonestReset(windows, nowMs);
  const hasData = service.connected && windows.length > 0;

  return (
    <CardShell onActivate={() => onOpen(service.provider)}>
      {/* Header */}
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 items-center gap-2.5">
          <span className="flex size-7 shrink-0 items-center justify-center rounded-md border border-border bg-surface-2 text-text-dim">
            <ProviderMark provider={service.provider} className="size-4" />
          </span>
          <div className="min-w-0 leading-tight">
            <h3 className="truncate font-semibold tracking-tight text-text" style={{ fontSize: 16 }}>
              {PROVIDER_LABEL[service.provider]}
            </h3>
            {service.account && (
              <p
                className="num truncate text-text-faint"
                style={{ fontSize: 11 }}
                title={service.account}
              >
                {service.account}
              </p>
            )}
          </div>
        </div>
        {service.plan && (
          <Badge
            variant="outline"
            className="shrink-0 border-border-strong bg-surface-2 font-medium text-text-dim"
          >
            {service.plan}
          </Badge>
        )}
      </div>

      {/* Body */}
      <div className="mt-4">
        {hasData ? (
          <>
            <div className="flex justify-center py-1">
              <RingGauge
                percent={primary?.used_percent ?? null}
                label={primary?.label ?? `${PROVIDER_LABEL[service.provider]} usage`}
                size={116}
              />
            </div>

            {secondary.length > 0 && (
              <div className="mt-4 space-y-3">
                {secondary.map((w, i) => (
                  <BarGauge key={`${w.label}-${i}`} window={w} nowMs={nowMs} showMeta={false} />
                ))}
              </div>
            )}
          </>
        ) : (
          <CardError
            error={service.error}
            connected={service.connected}
            provider={service.provider}
          />
        )}
      </div>

      {/* Footer — resets countdown + details affordance */}
      <div className="mt-4 flex items-center justify-between border-t border-border pt-3">
        <span className="num text-text-faint" style={{ fontSize: 11 }}>
          {reset ?? (hasData ? "no reset window" : "")}
        </span>
        <span className="flex items-center gap-1 text-text-dim transition-colors group-hover:text-text" style={{ fontSize: 11, fontWeight: 500 }}>
          Details
          <ChevronRight className="size-3.5 transition-transform duration-150 group-hover:translate-x-0.5" />
        </span>
      </div>
    </CardShell>
  );
}

/**
 * The clickable card surface. A real <button> so Enter/Space activates it and
 * the global focus ring + hover lift apply uniformly.
 */
function CardShell({
  children,
  onActivate,
}: {
  children: React.ReactNode;
  onActivate: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onActivate}
      className={cn(
        "group lift-on-hover relative flex w-full flex-col rounded-xl border border-border bg-surface p-4 text-left",
        "shadow-[0_12px_30px_-18px_rgba(0,0,0,0.7),inset_0_1px_0_rgba(255,255,255,0.03)]",
        "transition-[background-color,border-color,transform,box-shadow] duration-200 ease-out",
        "hover:-translate-y-0.5 hover:border-border-strong hover:bg-surface-2",
        "hover:shadow-[0_22px_42px_-22px_rgba(0,0,0,0.85),inset_0_1px_0_rgba(255,255,255,0.05)]",
      )}
    >
      {children}
    </button>
  );
}
