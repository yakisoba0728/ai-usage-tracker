import type { DragEvent, KeyboardEvent } from "react";

import { BarGauge } from "@/components/BarGauge";
import { ProviderMark } from "@/components/ProviderMark";
import { CardError } from "@/components/ErrorState";
import { providerDisplayName, cardWindows } from "@/lib/providers";
import { cn } from "@/lib/utils";
import type { AppConfig, Provider, ServiceUsage } from "@/lib/types";

export interface ProviderCardProps {
  service: ServiceUsage;
  nowMs: number;
  config: AppConfig | null;
  /** Show the per-card loading shimmer (provider is mid-fetch). */
  loading?: boolean;
  /** Enable native drag-and-drop reordering (only in custom sort mode). */
  draggable?: boolean;
  /** This card is the one being dragged → fade it. */
  isDragging?: boolean;
  /** A dragged card is hovering over this one → show the drop indicator. */
  isDropTarget?: boolean;
  onOpen: (provider: Provider) => void;
  onDragStart?: (provider: Provider, e: DragEvent<HTMLDivElement>) => void;
  onDragEnter?: (provider: Provider, e: DragEvent<HTMLDivElement>) => void;
  onDragLeave?: (provider: Provider, e: DragEvent<HTMLDivElement>) => void;
  onDrop?: (provider: Provider, e: DragEvent<HTMLDivElement>) => void;
  onDragEnd?: (provider: Provider, e: DragEvent<HTMLDivElement>) => void;
}

/**
 * Flat bars-only card. The resolved headline window (user-pinned
 * `primary_window`, else the first primary window) leads with a 6px severity
 * bar + reset countdown; the rest render as compact 4px bars. The whole card is
 * a role=button that opens the detail modal, and — in custom sort mode — a
 * native HTML5 drag handle for reordering.
 */
export function ProviderCard({
  service,
  nowMs,
  config,
  loading = false,
  draggable = false,
  isDragging = false,
  isDropTarget = false,
  onOpen,
  onDragStart,
  onDragEnter,
  onDragLeave,
  onDrop,
  onDragEnd,
}: ProviderCardProps) {
  const windows = cardWindows(service, config);
  const primary = windows[0];
  const secondary = windows.slice(1);
  const hasData = service.connected && windows.length > 0 && primary != null;
  const name = providerDisplayName(config, service.provider);

  function handleKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      onOpen(service.provider);
    }
  }

  const dragHandlers = draggable
    ? {
        draggable: true as const,
        onDragStart: (e: DragEvent<HTMLDivElement>) => onDragStart?.(service.provider, e),
        onDragEnter: (e: DragEvent<HTMLDivElement>) => onDragEnter?.(service.provider, e),
        onDragOver: (e: DragEvent<HTMLDivElement>) => {
          // Must preventDefault on dragover for a div to accept drops.
          e.preventDefault();
        },
        onDragLeave: (e: DragEvent<HTMLDivElement>) => onDragLeave?.(service.provider, e),
        onDrop: (e: DragEvent<HTMLDivElement>) => onDrop?.(service.provider, e),
        onDragEnd: (e: DragEvent<HTMLDivElement>) => onDragEnd?.(service.provider, e),
      }
    : {};

  return (
    <div
      role="button"
      tabIndex={0}
      aria-label={`${name} usage. Open details.`}
      onClick={() => onOpen(service.provider)}
      onKeyDown={handleKeyDown}
      {...dragHandlers}
      className={cn(
        "group flex w-full cursor-pointer flex-col rounded-lg border bg-surface p-4 text-left",
        "transition-[background-color,border-color,opacity] duration-100",
        "hover:border-border-strong hover:bg-surface-2",
        isDropTarget ? "border-border-strong border-dashed" : "border-border",
        isDragging && "opacity-50",
        loading && "card-loading",
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
            <h3 className="truncate text-md font-semibold text-text">{name}</h3>
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
    </div>
  );
}
