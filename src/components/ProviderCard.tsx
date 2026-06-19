import { BarGauge } from "@/components/BarGauge";
import { ProviderMark, PROVIDER_LABEL } from "@/components/ProviderMark";
import { CardError } from "@/components/ErrorState";
import { cn } from "@/lib/utils";
import type { Provider, ServiceUsage } from "@/lib/types";

export interface ProviderCardProps {
  service: ServiceUsage;
  nowMs: number;
  onOpen: (provider: Provider) => void;
}

/**
 * Flat bars-only card. Primary window headlines with a 6px severity bar + reset
 * countdown; remaining windows are compact 4px bars. The whole card is a button
 * that opens the detail modal. Hover brightens the border + surface only — no
 * lift, no shadow.
 */
export function ProviderCard({ service, nowMs, onOpen }: ProviderCardProps) {
  const windows = service.windows ?? [];
  const primary = windows[0];
  const secondary = windows.slice(1);
  const hasData = service.connected && windows.length > 0 && primary != null;

  return (
    <button
      type="button"
      onClick={() => onOpen(service.provider)}
      className={cn(
        "group flex w-full cursor-pointer flex-col rounded-lg border border-border bg-surface p-4 text-left",
        "transition-[background-color,border-color] duration-100",
        "hover:border-border-strong hover:bg-surface-2",
      )}
    >
      {/* Header row — mark + name (left), plan badge (right) */}
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 items-center gap-2.5">
          <ProviderMark
            provider={service.provider}
            className="size-4 shrink-0 text-text-dim"
          />
          <div className="min-w-0 leading-tight">
            <h3 className="truncate text-md font-semibold text-text">
              {PROVIDER_LABEL[service.provider]}
            </h3>
            {service.account && (
              <p
                className="num mt-0.5 truncate text-text-faint"
                style={{ fontSize: 11 }}
                title={service.account}
              >
                {service.account}
              </p>
            )}
          </div>
        </div>
        {service.plan && (
          <span
            className="shrink-0 rounded border border-border-strong px-1.5 py-0.5 font-medium text-text-faint"
            style={{ fontSize: 11 }}
          >
            {service.plan}
          </span>
        )}
      </div>

      {/* Body — bars, or an inline error */}
      <div className="mt-4">
        {hasData ? (
          <div className="space-y-3">
            <BarGauge window={primary} nowMs={nowMs} variant="primary" />
            {secondary.map((w, i) => (
              <BarGauge
                key={`${w.label}-${i}`}
                window={w}
                nowMs={nowMs}
                variant="secondary"
              />
            ))}
          </div>
        ) : (
          <CardError
            error={service.error}
            connected={service.connected}
            provider={service.provider}
          />
        )}
      </div>
    </button>
  );
}
