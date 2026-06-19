import { Plus, RotateCw, Settings } from "lucide-react";

import { Button } from "@/components/ui/button";
import { formatUpdatedAgo } from "@/lib/format";

export interface HeaderProps {
  /** Fetched-at epoch seconds for the "Updated … ago" label. */
  fetchedAt: number | null;
  nowMs: number;
  refreshing: boolean;
  onRefresh: () => void;
  onAddAccount: () => void;
  onOpenSettings: () => void;
}

/**
 * 44px header. Left: app name (dim). Right: live "Updated … ago" timestamp,
 * then three ghost actions — Add Account (primary text action), Refresh
 * (RotateCw, spins while refreshing), Settings. The timestamp re-renders every
 * second off the single `useNow` timer owned by the Dashboard.
 */
export function Header({
  fetchedAt,
  nowMs,
  refreshing,
  onRefresh,
  onAddAccount,
  onOpenSettings,
}: HeaderProps) {
  return (
    <header className="sticky top-0 z-30 border-b border-border bg-canvas">
      <div className="mx-auto flex h-11 w-full max-w-[1100px] items-center gap-3 px-5">
        <span className="text-sm font-medium text-text-dim">
          AI Usage Tracker
        </span>

        <div className="flex-1" />

        <span
          className="num shrink-0 text-text-faint"
          style={{ fontSize: 11 }}
          title={fetchedAt != null ? new Date(fetchedAt * 1000).toLocaleString() : undefined}
        >
          {formatUpdatedAgo(fetchedAt, nowMs)}
        </span>

        <div className="flex items-center gap-1">
          <Button
            variant="ghost"
            size="sm"
            onClick={onAddAccount}
            className="gap-1.5 text-text-dim hover:text-text"
          >
            <Plus className="size-4" />
            Add Account
          </Button>

          <Button
            variant="ghost"
            size="icon"
            onClick={onRefresh}
            disabled={refreshing}
            aria-label="Refresh usage"
            title="Refresh"
            className="text-text-faint hover:text-text"
          >
            <RotateCw className={refreshing ? "size-4 animate-spin" : "size-4"} />
          </Button>

          <Button
            variant="ghost"
            size="icon"
            onClick={onOpenSettings}
            aria-label="Settings"
            title="Settings"
            className="text-text-faint hover:text-text"
          >
            <Settings className="size-4" />
          </Button>
        </div>
      </div>
    </header>
  );
}
