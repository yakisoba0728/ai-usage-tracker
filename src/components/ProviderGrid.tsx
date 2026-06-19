import { useState, type DragEvent } from "react";

import { ProviderCard } from "@/components/ProviderCard";
import type { SortBy } from "@/components/SettingsDialog";
import type { AppConfig, Provider, ServiceUsage } from "@/lib/types";

export interface ProviderGridProps {
  /** Already filtered + ordered for display. */
  services: ServiceUsage[];
  config: AppConfig | null;
  nowMs: number;
  loadingProviders: Set<Provider>;
  /** Cards are only draggable in custom (manual) sort mode. */
  sortBy: SortBy;
  onOpen: (provider: Provider) => void;
  /** Persist a new ordering: move `from` into `to`'s slot. */
  onReorder: (from: Provider, to: Provider) => void;
}

/**
 * The responsive card grid plus its native HTML5 drag-and-drop state. Dragging
 * is enabled only when `sortBy === "custom"` — auto-sort modes (usage / name)
 * would immediately clobber a manual reorder. The dragged card fades to 50%;
 * the current drop target shows a dashed strong border. On drop the parent
 * rewrites `sort_index` and persists.
 */
export function ProviderGrid({
  services,
  config,
  nowMs,
  loadingProviders,
  sortBy,
  onOpen,
  onReorder,
}: ProviderGridProps) {
  const [dragProvider, setDragProvider] = useState<Provider | null>(null);
  // Tracked via dragEnter (not dragLeave) to avoid flicker between child nodes;
  // cleared on drop / dragEnd.
  const [dropTarget, setDropTarget] = useState<Provider | null>(null);
  const draggable = sortBy === "custom";

  function handleDragStart(provider: Provider, e: DragEvent<HTMLDivElement>) {
    setDragProvider(provider);
    setDropTarget(null);
    e.dataTransfer.effectAllowed = "move";
    e.dataTransfer.setData("text/plain", provider);
  }

  function handleDragEnter(provider: Provider, e: DragEvent<HTMLDivElement>) {
    if (provider === dragProvider) {
      setDropTarget(null);
      return;
    }
    e.preventDefault();
    setDropTarget(provider);
  }

  function handleDrop(provider: Provider, e: DragEvent<HTMLDivElement>) {
    e.preventDefault();
    const from = dragProvider;
    setDragProvider(null);
    setDropTarget(null);
    if (from && from !== provider) onReorder(from, provider);
  }

  function handleDragEnd() {
    setDragProvider(null);
    setDropTarget(null);
  }

  return (
    <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
      {services.map((service) => (
        <ProviderCard
          key={service.provider}
          service={service}
          config={config}
          nowMs={nowMs}
          loading={loadingProviders.has(service.provider)}
          draggable={draggable}
          isDragging={dragProvider === service.provider}
          isDropTarget={dropTarget === service.provider}
          onOpen={onOpen}
          onDragStart={handleDragStart}
          onDragEnter={handleDragEnter}
          onDragLeave={() => {
            /* intentionally no-op — dropTarget cleared on drop/dragEnd */
          }}
          onDrop={handleDrop}
          onDragEnd={handleDragEnd}
        />
      ))}
    </div>
  );
}
