import { Loader2, RefreshCw, Settings } from "lucide-react";

import { Button } from "@/components/ui/button";

export interface HeaderProps {
  refreshing: boolean;
  onRefresh: () => void;
  onOpenSettings: () => void;
}

/**
 * Minimal 44px header — just the app name (dim) on the left and two ghost
 * buttons (Refresh, Settings) on the right. No brand mark, no fleet status, no
 * peak pill, no timestamp.
 */
export function Header({ refreshing, onRefresh, onOpenSettings }: HeaderProps) {
  return (
    <header className="sticky top-0 z-30 border-b border-border bg-canvas">
      <div className="mx-auto flex h-11 w-full max-w-[1100px] items-center justify-between px-5">
        <span className="text-sm font-medium text-text-dim">
          AI Usage Tracker
        </span>

        <div className="flex items-center gap-1">
          <Button
            variant="ghost"
            size="icon"
            onClick={onRefresh}
            disabled={refreshing}
            aria-label="Refresh usage"
            title="Refresh"
            className="text-text-faint hover:text-text"
          >
            {refreshing ? (
              <Loader2 className="size-4 animate-spin" />
            ) : (
              <RefreshCw className="size-4" />
            )}
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
